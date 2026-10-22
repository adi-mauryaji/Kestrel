//! Week 9 "E" for the template plans: every Kestrel-Tasks-v0 item of the dev split through
//! `templates::propose`, with the gold category as the intent (oracle routing, so the templates are
//! judged alone), and each plan judged by `eval` on its end state. A clarify item succeeds when the
//! template asks; a plan item when its plan validates and reaches the expected end state. Compound
//! items are Tier 4's (9d) and only counted. Also recorded: whether the plan equals the gold one.
//!
//!     cargo run -p kestrel-tools --example template_eval [-- OUT.json]
//!
//! Only dev: the test split is evaluated once, after the freeze (Ch. 34.6).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use kestrel_core::Intent;
use kestrel_tools::eval::{self, Fixture};
use kestrel_tools::plan;
use kestrel_tools::templates::{Context, Proposal, propose};
use serde_json::{Value, json};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn intent(category: &str) -> Option<Intent> {
    Some(match category {
        "create_task" => Intent::CreateTask,
        "search_notes" => Intent::SearchNotes,
        "summarize_notes" => Intent::SummarizeNotes,
        "delete_task" => Intent::DeleteTask,
        "read_document" => Intent::ReadDocument,
        "set_preference" => Intent::SetPreference,
        "retrieve_memory" => Intent::RetrieveMemory,
        _ => return None, // compound: several intents, planned by Tier 4
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| format!("{ROOT}/experiments/logs/tasks/templates_dev.json"));
    let build = Path::new(ROOT).join("datasets/kestrel_tasks_v0/build");
    let fixture = Fixture::load(&build.join("fixture.json"), Path::new(ROOT))?;
    let documents = fixture.documents();
    let ctx = Context {
        today: fixture.today,
        documents: &documents,
    };
    let items: Vec<Value> = fs::read_to_string(build.join("items.jsonl"))?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;

    let mut rows = Vec::new();
    let mut compound = 0;
    for item in items.iter().filter(|i| i["split"] == "dev") {
        let (id, text) = (
            item["id"].as_str().unwrap_or_default(),
            item["text"].as_str().unwrap_or_default(),
        );
        let Some(intent) = intent(item["category"].as_str().unwrap_or_default()) else {
            compound += 1;
            continue;
        };
        let wants_clarify = item["expect"]["outcome"] == "clarify";
        let (got, valid, gold_match, problems) = match propose(intent, text, &ctx) {
            Proposal::Plan(p) => match plan::parse(&p.to_string()) {
                Ok(vp) => {
                    let scratch = std::env::temp_dir()
                        .join(format!("kestrel_template_eval_{}", std::process::id()));
                    let problems = if wants_clarify {
                        vec!["invented a plan where a slot was missing".to_string()]
                    } else {
                        eval::judge(
                            &item["expect"],
                            &fixture,
                            &eval::run(&vp, &fixture, &scratch)?,
                        )
                    };
                    ("plan".to_string(), true, p == item["plan"], problems)
                }
                Err(e) => (
                    "plan".into(),
                    false,
                    false,
                    vec![format!("invalid plan: {e}")],
                ),
            },
            Proposal::Clarify(slot) => {
                let problems = if wants_clarify {
                    vec![]
                } else {
                    vec![format!("asked for {slot} instead of acting")]
                };
                (format!("clarify:{slot}"), true, wants_clarify, problems)
            }
            other => (
                format!("{other:?}"),
                false,
                false,
                vec![format!("no template plan: {other:?}")],
            ),
        };
        rows.push(json!({"id": id, "family": item["family"], "category": item["category"], "lang": item["lang"],
                         "got": got, "valid": valid, "gold_match": gold_match, "success": problems.is_empty(),
                         "problems": problems}));
    }

    let mut by: BTreeMap<String, (u32, u32, u32)> = BTreeMap::new();
    for r in &rows {
        for key in [
            r["category"].as_str().unwrap_or_default().to_string(),
            format!("lang:{}", r["lang"].as_str().unwrap_or_default()),
        ] {
            let e = by.entry(key).or_default();
            e.0 += 1;
            e.1 += u32::from(r["success"] == true);
            e.2 += u32::from(r["gold_match"] == true);
        }
    }
    let n = rows.len() as f64;
    let rate = |k: &str| rows.iter().filter(|r| r[k] == true).count() as f64 / n;
    let summary = json!({
        "split": "dev", "items": rows.len(), "compound_items_left_to_tier4": compound,
        "valid_rate": rate("valid"), "success_rate": rate("success"), "gold_match_rate": rate("gold_match"),
        "by": by.iter().map(|(k, (n, s, g))| (k.clone(), json!({"n": n, "success": s, "gold_match": g}))).collect::<serde_json::Map<_, _>>(),
        "routing": "oracle: the gold category is the intent",
        "note": "rules written from the dev requests; the test split is evaluated once after the freeze",
    });
    if let Some(dir) = Path::new(&out).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(
        &out,
        format!("{:#}\n", json!({"summary": summary, "items": rows})),
    )?;
    println!("{summary:#}");
    for r in rows.iter().filter(|r| r["success"] != true) {
        println!("FAIL {} -> {}: {}", r["id"], r["got"], r["problems"]);
    }
    Ok(())
}
