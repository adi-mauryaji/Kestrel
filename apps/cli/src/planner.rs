//! `kestrel plan-eval` (Week 9d, ADR-023): the Tier-4 planner on Kestrel-Tasks-v0. Gemma proposes
//! a plan for each dev compound request under the plan grammar (`kestrel_tools::grammar`); the plan
//! is validated (`plan::parse`; one re-prompt with the error, Ch. 16) and judged on its end state
//! (`kestrel_tools::eval`, deterministic text ops, the user answering yes). Oracle routing: compound
//! requests are the ones the router sends to Tier 4.
//!
//!     kestrel plan-eval [OUT.json]          dev compound items (default experiments/logs/tasks/planner_dev.json)
//!     kestrel plan-eval --check-grammar     every gold plan must be admitted by the grammar
//!
//! The prompt is datasets/contracts/tier4_plan_prompt.txt with today's date and the coming days
//! filled in, plus the examples in tier4_plan_examples.jsonl as earlier turns (written in the
//! grammar's canonical form). Only dev: the test split is evaluated once, after the freeze.

use kestrel_core::KestrelError;
use kestrel_tools::date::Date;
use kestrel_tools::grammar::canonical;
use serde_json::Value;

pub const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// The plan prompt with {today}, {weekday}, {calendar} (the next 7 days), {tomorrow}, {day_after}.
pub fn render_system(template: &str, today: Date) -> String {
    let name = |d: Date| WEEKDAYS[d.weekday() as usize];
    let calendar: Vec<String> = (1..=7)
        .map(|k| today.add_days(k))
        .map(|d| format!("{} {d}", name(d)))
        .collect();
    template
        .replace("{today}", &today.to_string())
        .replace("{weekday}", name(today))
        .replace("{calendar}", &calendar.join(", "))
        .replace("{tomorrow}", &today.add_days(1).to_string())
        .replace("{day_after}", &today.add_days(2).to_string())
}

/// (request, plan in canonical form) pairs, with {tomorrow} filled in.
pub fn render_examples(jsonl: &str, today: Date) -> Result<Vec<(String, String)>, KestrelError> {
    let tomorrow = today.add_days(1).to_string();
    jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let v: Value = serde_json::from_str(&line.replace("{tomorrow}", &tomorrow))
                .map_err(|e| KestrelError::Invalid(format!("plan example: {e}")))?;
            let request = v["request"]
                .as_str()
                .ok_or_else(|| KestrelError::Invalid("plan example without a request".into()))?;
            Ok((request.to_string(), canonical(&v["plan"])?))
        })
        .collect()
}

/// Tokens a plan may take (12 small nodes fit).
pub const MAX_PLAN_TOKENS: usize = 400;

#[cfg(feature = "llama")]
pub fn run(check_grammar: bool, out: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use kestrel_core::resources::{CancelToken, Deadline};
    use kestrel_inference::GenParams;
    use kestrel_inference::llama::{LlamaConfig, LlamaSlm};
    use kestrel_inference::prompt::{ChatFormat, ChatPrompt};
    use kestrel_tools::eval::{self, Fixture};
    use kestrel_tools::grammar::{ROOT, gbnf};
    use kestrel_tools::plan;
    use serde_json::json;

    let build = Path::new("datasets/kestrel_tasks_v0/build");
    let fixture = Fixture::load(&build.join("fixture.json"), Path::new("."))?;
    let items: Vec<Value> = std::fs::read_to_string(build.join("items.jsonl"))?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let model = std::env::var_os("KESTREL_SLM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("models/gemma-4-E2B-it-Q4_0.gguf"));
    eprintln!("loading {} ...", model.display());
    let slm = LlamaSlm::load(
        &model,
        LlamaConfig {
            n_ctx: 4096,
            ..LlamaConfig::default()
        },
    )?;
    let grammar = gbnf();

    if check_grammar {
        let (mut seen, mut bad) = (HashSet::new(), Vec::new());
        for item in items.iter().filter(|i| !i["plan"].is_null()) {
            let text = canonical(&item["plan"])?;
            if seen.insert(text.clone()) && !slm.grammar_admits(&grammar, ROOT, &text)? {
                bad.push(item["id"].clone());
            }
        }
        // And the check is not vacuous: plans the grammar must refuse.
        let refuse = [
            r#"{"nodes": [{"id": "a", "op": "run_shell", "args": {"cmd": "rm -rf ~"}}]}"#,
            r#"{"nodes": [{"id": "a", "op": "search_notes", "args": {"query": "q", "cmd": "x"}}]}"#,
            r#"{"nodes": [{"id": "A", "op": "summarize"}]}"#,
            r#"{"nodes": [{"id": "t", "op": "create_task", "args": {"title": "x", "due": "kal"}}]}"#,
            "delete all my tasks",
        ];
        let admitted: Vec<&str> = refuse
            .iter()
            .copied()
            .filter(|t| slm.grammar_admits(&grammar, ROOT, t).unwrap_or(true))
            .collect();
        println!(
            "{} distinct gold plans, {} not admitted by the grammar: {bad:?}; {} of {} bad plans admitted: {admitted:?}",
            seen.len(),
            bad.len(),
            admitted.len(),
            refuse.len()
        );
        return if bad.is_empty() && admitted.is_empty() {
            Ok(())
        } else {
            Err("the grammar rejects a gold plan or admits a bad one".into())
        };
    }

    let arch = slm.architecture()?;
    let format =
        ChatFormat::for_architecture(&arch).ok_or(format!("no chat format for {arch:?}"))?;
    let system = render_system(
        &std::fs::read_to_string("datasets/contracts/tier4_plan_prompt.txt")?,
        fixture.today,
    );
    let examples = render_examples(
        &std::fs::read_to_string("datasets/contracts/tier4_plan_examples.jsonl")?,
        fixture.today,
    )?;
    let prompt = ChatPrompt::new(format, &system).with_examples(examples);
    let prefix = prompt.prefix();
    let mut session = slm.session()?;
    let t0 = Instant::now();
    let prefix_tokens = session.prime(&prefix)?;
    let prime_s = t0.elapsed().as_secs_f64();
    eprintln!("primed the plan prompt: {prefix_tokens} tokens in {prime_s:.1} s");

    let params = GenParams::greedy(MAX_PLAN_TOKENS);
    let mut rows = Vec::new();
    for item in items
        .iter()
        .filter(|i| i["split"] == "dev" && i["category"] == "compound")
    {
        let text = item["text"].as_str().unwrap_or_default();
        let mut generate = |suffix: &str| {
            session.generate_constrained(
                &prefix,
                suffix,
                &grammar,
                ROOT,
                &params,
                &CancelToken::new(),
                &Deadline::after(Duration::from_secs(180)),
            )
        };
        let suffix = prompt.suffix(text);
        let mut attempts = Vec::new();
        let mut answer = match generate(&suffix) {
            Ok(r) => r,
            Err(e) => {
                rows.push(json!({"id": item["id"], "lang": item["lang"], "error": e.to_string(), "success": false}));
                continue;
            }
        };
        attempts
            .push(json!({"plan": answer.0, "tokens": answer.1.tokens, "ms": answer.1.total_ms}));
        let mut parsed = plan::parse(&answer.0);
        let valid_first = parsed.is_ok();
        if let Err(e) = &parsed {
            // One re-prompt with the validator's message (Ch. 16).
            let again = suffix.clone()
                + &prompt.follow_up(
                    &answer.0,
                    &format!("That plan is invalid: {e}. Answer with a corrected plan."),
                );
            if let Ok(r) = generate(&again) {
                answer = r;
                attempts.push(
                    json!({"plan": answer.0, "tokens": answer.1.tokens, "ms": answer.1.total_ms}),
                );
                parsed = plan::parse(&answer.0);
            }
        }
        let (success, problems, gold_match) = match &parsed {
            Ok(p) => {
                let scratch =
                    std::env::temp_dir().join(format!("kestrel_plan_eval_{}", std::process::id()));
                let problems = eval::judge(
                    &item["expect"],
                    &fixture,
                    &eval::run(p, &fixture, &scratch)?,
                );
                let as_value: Value = serde_json::from_str(&answer.0).unwrap_or(Value::Null);
                (problems.is_empty(), problems, as_value == item["plan"])
            }
            Err(e) => (
                false,
                vec![format!("invalid after the re-prompt: {e}")],
                false,
            ),
        };
        eprintln!("{} {}", item["id"], if success { "ok" } else { "FAIL" });
        rows.push(
            json!({"id": item["id"], "family": item["family"], "lang": item["lang"], "text": text,
                         "valid_first": valid_first, "valid": parsed.is_ok(), "success": success,
                         "gold_match": gold_match, "problems": problems, "attempts": attempts}),
        );
    }

    let n = rows.len() as f64;
    let rate = |k: &str| rows.iter().filter(|r| r[k] == true).count() as f64 / n;
    let ms: Vec<f64> = rows
        .iter()
        .filter_map(|r| r["attempts"][0]["ms"].as_f64())
        .collect();
    let summary = json!({
        "split": "dev", "category": "compound", "items": rows.len(), "model": model.display().to_string(),
        "valid_first_rate": rate("valid_first"), "valid_rate": rate("valid"), "success_rate": rate("success"),
        "gold_match_rate": rate("gold_match"), "first_attempt_ms_mean": ms.iter().sum::<f64>() / ms.len().max(1) as f64,
        "prefix_tokens": prefix_tokens, "prime_s": prime_s,
        "note": "dev timing on the laptop as found; text ops are the deterministic MarkedItems",
    });
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        out,
        format!("{:#}\n", json!({"summary": summary, "items": rows})),
    )?;
    println!("{summary:#}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

    fn today() -> Date {
        Date::parse("2026-10-22").unwrap()
    }

    #[test]
    fn the_system_prompt_gets_the_date_and_the_coming_days() {
        let t = "Today is {today} ({weekday}). Coming days: {calendar}. Kal = {tomorrow}, parso = {day_after}.";
        assert_eq!(
            render_system(t, today()),
            "Today is 2026-10-22 (thursday). Coming days: friday 2026-10-23, saturday 2026-10-24, \
             sunday 2026-10-25, monday 2026-10-26, tuesday 2026-10-27, wednesday 2026-10-28, \
             thursday 2026-10-29. Kal = 2026-10-23, parso = 2026-10-24."
        );
    }

    #[test]
    fn the_committed_contract_renders_completely() {
        let t = std::fs::read_to_string(format!("{ROOT}/datasets/contracts/tier4_plan_prompt.txt"))
            .unwrap();
        let s = render_system(&t, today());
        // A placeholder is "{" + lowercase letters or "_" + "}"; the JSON example is not one.
        let left: Vec<&str> = s
            .split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name))
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
            .collect();
        assert!(left.is_empty(), "placeholders left: {left:?}");
        assert!(s.contains("Today is 2026-10-22 (thursday)"));
    }

    #[test]
    fn examples_are_canonical_valid_plans_with_dates_filled_in() {
        let jsonl = std::fs::read_to_string(format!(
            "{ROOT}/datasets/contracts/tier4_plan_examples.jsonl"
        ))
        .unwrap();
        let ex = render_examples(&jsonl, today()).unwrap();
        assert_eq!(ex.len(), 3);
        for (request, plan) in &ex {
            assert!(
                !request.contains('{') && !plan.contains("{tomorrow}"),
                "{request} / {plan}"
            );
            kestrel_tools::plan::parse(plan).unwrap_or_else(|e| panic!("{request}: {e}"));
            let v: Value = serde_json::from_str(plan).unwrap();
            assert_eq!(
                &canonical(&v).unwrap(),
                plan,
                "written in the grammar's order"
            );
        }
        assert!(ex[1].0.ends_with("due 2026-10-23") && ex[1].1.contains("\"due\": \"2026-10-23\""));
        assert!(render_examples("{\"request\": \"x\"}", today()).is_err());
    }
}
