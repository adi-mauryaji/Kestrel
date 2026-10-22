//! `kestrel plan-eval` (Week 9d, ADR-023): the Tier-4 planner on Kestrel-Tasks-v0. Gemma proposes
//! a plan for each dev compound request under the plan grammar (`kestrel_tools::grammar`); the plan
//! is validated (`plan::parse`; one re-prompt with the error, Ch. 16) and judged on its end state
//! (`kestrel_tools::eval`, deterministic text ops, the user answering yes). Oracle routing: compound
//! requests are the ones the router sends to Tier 4.
//!
//!     kestrel plan-eval [OUT.json]          dev compound items (default experiments/logs/tasks/planner_dev.json)
//!     kestrel plan-eval --check-grammar     every gold plan must be admitted by the grammar
//!     kestrel plan-eval --hybrid [OUT.json] ADR-025: split + routed parts + templates, LM fallback
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

/// The plan prompt with {today}, {weekday}, {calendar} (the next 7 days), {tomorrow}, {day_after},
/// {documents} (the note and document names) and {tags} (the tags in use).
pub fn render_system(template: &str, today: Date, documents: &[String], tags: &[String]) -> String {
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
        .replace("{documents}", &documents.join(", "))
        .replace("{tags}", &tags.join(", "))
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

/// The Tier-4 planner: the plan prompt primed once, grammar-constrained greedy generation, one
/// re-prompt with the validator's message (Ch. 16).
#[cfg(feature = "llama")]
struct LmPlanner<'m> {
    prompt: kestrel_inference::prompt::ChatPrompt,
    prefix: String,
    grammar: String,
    session: kestrel_inference::llama::LlamaSession<'m>,
    prefix_tokens: usize,
    prime_s: f64,
}

#[cfg(feature = "llama")]
struct LmAnswer {
    plan: Result<kestrel_tools::plan::ValidPlan, String>,
    text: String,
    valid_first: bool,
    attempts: Vec<Value>,
}

#[cfg(feature = "llama")]
impl<'m> LmPlanner<'m> {
    fn new(
        slm: &'m kestrel_inference::llama::LlamaSlm,
        fixture: &kestrel_tools::eval::Fixture,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        use kestrel_inference::prompt::{ChatFormat, ChatPrompt};
        let arch = slm.architecture()?;
        let format =
            ChatFormat::for_architecture(&arch).ok_or(format!("no chat format for {arch:?}"))?;
        let (documents, tags) = (fixture.documents(), fixture.tags());
        let system = render_system(
            &std::fs::read_to_string("datasets/contracts/tier4_plan_prompt.txt")?,
            fixture.today,
            &documents,
            &tags,
        );
        let examples = render_examples(
            &std::fs::read_to_string("datasets/contracts/tier4_plan_examples.jsonl")?,
            fixture.today,
        )?;
        let prompt = ChatPrompt::new(format, &system).with_examples(examples);
        let prefix = prompt.prefix();
        let mut session = slm.session()?;
        let t0 = std::time::Instant::now();
        let prefix_tokens = session.prime(&prefix)?;
        let prime_s = t0.elapsed().as_secs_f64();
        eprintln!("primed the plan prompt: {prefix_tokens} tokens in {prime_s:.1} s");
        // The planner may name only existing documents; the open grammar is for template plans.
        let grammar = kestrel_tools::grammar::gbnf_with(&documents);
        Ok(Self {
            prompt,
            prefix,
            grammar,
            session,
            prefix_tokens,
            prime_s,
        })
    }

    fn plan(&mut self, text: &str) -> Result<LmAnswer, kestrel_core::KestrelError> {
        use kestrel_core::resources::{CancelToken, Deadline};
        use kestrel_inference::GenParams;
        use serde_json::json;
        let params = GenParams::greedy(MAX_PLAN_TOKENS);
        let mut generate = |suffix: &str| {
            self.session.generate_constrained(
                &self.prefix,
                suffix,
                &self.grammar,
                kestrel_tools::grammar::ROOT,
                &params,
                &CancelToken::new(),
                &Deadline::after(std::time::Duration::from_secs(180)),
            )
        };
        let suffix = self.prompt.suffix(text);
        let mut answer = generate(&suffix)?;
        let mut attempts =
            vec![json!({"plan": answer.0, "tokens": answer.1.tokens, "ms": answer.1.total_ms})];
        let mut parsed = kestrel_tools::plan::parse(&answer.0);
        let valid_first = parsed.is_ok();
        if let Err(e) = &parsed {
            let again = suffix.clone()
                + &self.prompt.follow_up(
                    &answer.0,
                    &format!("That plan is invalid: {e}. Answer with a corrected plan."),
                );
            if let Ok(r) = generate(&again) {
                answer = r;
                attempts.push(
                    json!({"plan": answer.0, "tokens": answer.1.tokens, "ms": answer.1.total_ms}),
                );
                parsed = kestrel_tools::plan::parse(&answer.0);
            }
        }
        Ok(LmAnswer {
            plan: parsed.map_err(|e| e.to_string()),
            text: answer.0,
            valid_first,
            attempts,
        })
    }
}

#[cfg(feature = "llama")]
fn load_tasks() -> Result<(kestrel_tools::eval::Fixture, Vec<Value>), Box<dyn std::error::Error>> {
    let build = std::path::Path::new("datasets/kestrel_tasks_v0/build");
    let fixture =
        kestrel_tools::eval::Fixture::load(&build.join("fixture.json"), std::path::Path::new("."))?;
    let items = std::fs::read_to_string(build.join("items.jsonl"))?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    Ok((fixture, items))
}

/// Problems with `plan` on `item` (empty = success), judged on its end state.
#[cfg(feature = "llama")]
fn judge(
    plan: &kestrel_tools::plan::ValidPlan,
    item: &Value,
    fixture: &kestrel_tools::eval::Fixture,
) -> Result<Vec<String>, kestrel_core::KestrelError> {
    let scratch = std::env::temp_dir().join(format!("kestrel_plan_eval_{}", std::process::id()));
    Ok(kestrel_tools::eval::judge(
        &item["expect"],
        fixture,
        &kestrel_tools::eval::run(plan, fixture, &scratch)?,
    ))
}

#[cfg(feature = "llama")]
fn write_report(
    out: &std::path::Path,
    summary: Value,
    rows: Vec<Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        out,
        format!(
            "{:#}\n",
            serde_json::json!({"summary": summary, "items": rows})
        ),
    )?;
    println!("{summary:#}");
    Ok(())
}

fn dev_compound(items: &[Value]) -> impl Iterator<Item = &Value> {
    items
        .iter()
        .filter(|i| i["split"] == "dev" && i["category"] == "compound")
}

fn rate(rows: &[Value], key: &str) -> f64 {
    rows.iter().filter(|r| r[key] == true).count() as f64 / rows.len().max(1) as f64
}

/// The LM-only planner on the dev compound items (or `check_grammar`).
#[cfg(feature = "llama")]
pub fn run(check_grammar: bool, out: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use kestrel_inference::llama::{LlamaConfig, LlamaSlm};
    use kestrel_tools::grammar::{ROOT, gbnf, gbnf_with};
    use serde_json::json;

    let (fixture, items) = load_tasks()?;
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

    if check_grammar {
        let (open, grammar) = (gbnf(), gbnf_with(&fixture.documents()));
        let (mut seen, mut bad) = (HashSet::new(), Vec::new());
        for item in items.iter().filter(|i| !i["plan"].is_null()) {
            let text = canonical(&item["plan"])?;
            let compound = item["category"] == "compound";
            if seen.insert(text.clone())
                && (!slm.grammar_admits(&open, ROOT, &text)?
                    || (compound && !slm.grammar_admits(&grammar, ROOT, &text)?))
            {
                bad.push(item["id"].clone());
            }
        }
        let outside =
            r#"{"nodes": [{"id": "r", "op": "read_document", "args": {"path": "/etc/passwd"}}]}"#;
        if slm.grammar_admits(&grammar, ROOT, outside)? {
            return Err("the planner grammar admits a path outside the documents".into());
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
            .filter(|t| slm.grammar_admits(&open, ROOT, t).unwrap_or(true))
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

    let mut planner = LmPlanner::new(&slm, &fixture)?;
    let mut rows = Vec::new();
    for item in dev_compound(&items) {
        let text = item["text"].as_str().unwrap_or_default();
        let a = match planner.plan(text) {
            Ok(a) => a,
            Err(e) => {
                rows.push(json!({"id": item["id"], "lang": item["lang"], "error": e.to_string(), "success": false}));
                continue;
            }
        };
        let (problems, gold_match) = match &a.plan {
            Ok(p) => (
                judge(p, item, &fixture)?,
                serde_json::from_str::<Value>(&a.text).ok() == Some(item["plan"].clone()),
            ),
            Err(e) => (vec![format!("invalid after the re-prompt: {e}")], false),
        };
        eprintln!(
            "{} {}",
            item["id"],
            if problems.is_empty() { "ok" } else { "FAIL" }
        );
        rows.push(json!({"id": item["id"], "family": item["family"], "lang": item["lang"], "text": text,
                         "valid_first": a.valid_first, "valid": a.plan.is_ok(), "success": problems.is_empty(),
                         "gold_match": gold_match, "problems": problems, "attempts": a.attempts}));
    }
    let ms: Vec<f64> = rows
        .iter()
        .filter_map(|r| r["attempts"][0]["ms"].as_f64())
        .collect();
    let summary = json!({
        "split": "dev", "category": "compound", "planner": "LM only", "items": rows.len(),
        "model": model.display().to_string(),
        "valid_first_rate": rate(&rows, "valid_first"), "valid_rate": rate(&rows, "valid"),
        "success_rate": rate(&rows, "success"), "gold_match_rate": rate(&rows, "gold_match"),
        "first_attempt_ms_mean": ms.iter().sum::<f64>() / ms.len().max(1) as f64,
        "prefix_tokens": planner.prefix_tokens, "prime_s": planner.prime_s,
        "note": "dev timing on the laptop as found; text ops are the deterministic MarkedItems",
    });
    write_report(out, summary, rows)
}

/// ADR-025: split each dev compound request, route every part through the Intents-v0 cascade,
/// join the template plans; the LM planner (the cascade's own Gemma) for whatever they cannot do.
#[cfg(feature = "llama")]
pub fn run_hybrid(out: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Instant;

    use kestrel_core::Request;
    use kestrel_router::Outcome;
    use kestrel_tools::compound::{plan_parts, split};
    use kestrel_tools::templates::{Context, Proposal};
    use serde_json::json;

    use crate::labels::Intents;
    use crate::route_items::Models;

    let (fixture, items) = load_tasks()?;
    let config = std::path::Path::new("experiments/configs/router_intents_v0.json");
    eprintln!("loading the Intents-v0 cascade ({}) ...", config.display());
    let models = Models::load(config)?;
    let cascade = models.cascade(&Intents, |t| t)?;
    let slm = models.slm().ok_or("the cascade has no SLM")?;
    let mut planner = LmPlanner::new(slm, &fixture)?;
    let documents = fixture.documents();
    let ctx = Context {
        today: fixture.today,
        documents: &documents,
    };

    let mut rows = Vec::new();
    let mut n = 0u64;
    for item in dev_compound(&items) {
        let text = item["text"].as_str().unwrap_or_default();
        let t0 = Instant::now();
        let mut parts = Vec::new();
        let mut routed = Vec::new();
        for part in split(text) {
            n += 1;
            let (outcome, trace) = cascade.route(&Request::new(n, &part));
            let tier = trace.tiers_consulted.last().map(|&t| t as u8);
            match outcome {
                Outcome::Accepted(p) => {
                    parts.push(json!({"text": part, "intent": p.intent.label(), "tier": tier, "confidence": p.confidence}));
                    routed.push((p.intent, part));
                }
                Outcome::Abstained { .. } => {
                    parts.push(json!({"text": part, "intent": null, "tier": tier}))
                }
            }
        }
        let proposal = if routed.len() == parts.len() {
            plan_parts(&routed, &ctx)
        } else {
            Proposal::NeedsModel
        };
        let (path, plan, attempts) = match proposal {
            Proposal::Plan(v) => (
                "templates",
                kestrel_tools::plan::parse(&v.to_string()).map_err(|e| e.to_string()),
                vec![],
            ),
            Proposal::Clarify(slot) => ("clarify", Err(format!("asked for {slot}")), vec![]),
            Proposal::NeedsModel | Proposal::NoAction => match planner.plan(text) {
                Ok(a) => ("planner", a.plan, a.attempts),
                Err(e) => ("planner", Err(e.to_string()), vec![]),
            },
        };
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        let problems = match &plan {
            Ok(p) => judge(p, item, &fixture)?,
            Err(e) => vec![e.clone()],
        };
        eprintln!(
            "{} {} via {path}",
            item["id"],
            if problems.is_empty() { "ok" } else { "FAIL" }
        );
        rows.push(json!({"id": item["id"], "family": item["family"], "lang": item["lang"], "text": text,
                         "parts": parts, "path": path, "success": problems.is_empty(), "problems": problems,
                         "ms": ms, "attempts": attempts}));
    }
    let by_path = |p: &str| rows.iter().filter(|r| r["path"] == p).count();
    let mean_ms = |p: &str| {
        let v: Vec<f64> = rows
            .iter()
            .filter(|r| r["path"] == p)
            .filter_map(|r| r["ms"].as_f64())
            .collect();
        if v.is_empty() {
            None
        } else {
            Some(v.iter().sum::<f64>() / v.len() as f64)
        }
    };
    let summary = json!({
        "split": "dev", "category": "compound", "planner": "split + templates, LM fallback (ADR-025)",
        "items": rows.len(), "success_rate": rate(&rows, "success"),
        "paths": {"templates": by_path("templates"), "planner": by_path("planner"), "clarify": by_path("clarify")},
        "ms_mean": {"templates": mean_ms("templates"), "planner": mean_ms("planner")},
        "router_config": config.display().to_string(),
        "note": "dev timing on the laptop as found; parts routed by the Intents-v0 dev cascade (Tier 1/3/4)",
    });
    write_report(out, summary, rows)
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
        let t = "Today is {today} ({weekday}). Coming days: {calendar}. Kal = {tomorrow}, parso = {day_after}. \
                 Docs: {documents}. Tags: {tags}.";
        let docs = ["docs/a.md".to_string(), "b.md".to_string()];
        let tags = ["meeting".to_string(), "travel".to_string()];
        assert_eq!(
            render_system(t, today(), &docs, &tags),
            "Today is 2026-10-22 (thursday). Coming days: friday 2026-10-23, saturday 2026-10-24, \
             sunday 2026-10-25, monday 2026-10-26, tuesday 2026-10-27, wednesday 2026-10-28, \
             thursday 2026-10-29. Kal = 2026-10-23, parso = 2026-10-24. Docs: docs/a.md, b.md. \
             Tags: meeting, travel."
        );
    }

    #[test]
    fn the_committed_contract_renders_completely() {
        let t = std::fs::read_to_string(format!("{ROOT}/datasets/contracts/tier4_plan_prompt.txt"))
            .unwrap();
        let s = render_system(&t, today(), &["x.md".into()], &["meeting".into()]);
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
