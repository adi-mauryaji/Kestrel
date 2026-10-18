//! Tier-1 rule outcomes for a JSONL file of items: the per-item Tier-1 log for offline router
//! analysis (Week 7). Text is normalized the way `Request::new` does it.
//!
//!     cargo run --release -p kestrel-nlu --example rules_items -- ITEMS.jsonl OUT.jsonl
//!
//! In: one {"id", "text"} per line. Out: one line per item, in order:
//! {"id", "outcome": "match"|"ambiguous"|"negated"|"no_match", "intent", "rule", "intents",
//!  "conjunction", "us"} (intent and rule only for a match; intents only when ambiguous; `us` is
//!  the time of `classify`, the Tier-1 dev cost).

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::time::Instant;

use kestrel_core::Request;
use kestrel_nlu::rules::{RuleOutcome, RuleSet, has_conjunction};
use serde_json::{Value, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, items, out] = args.as_slice() else {
        return Err("usage: rules_items ITEMS.jsonl OUT.jsonl".into());
    };
    let rules = RuleSet::default_set();
    let mut w = BufWriter::new(std::fs::File::create(out)?);
    for line in BufReader::new(std::fs::File::open(items)?).lines() {
        let item: Value = serde_json::from_str(&line?)?;
        let id = item["id"].clone();
        let text = item["text"].as_str().ok_or("item without text")?;
        let req = Request::new(0, text);
        let t0 = Instant::now();
        let outcome = rules.classify(&req.text);
        let us = t0.elapsed().as_secs_f64() * 1e6;
        let mut rec = json!({"id": id, "conjunction": has_conjunction(&req.text), "us": us});
        match outcome {
            RuleOutcome::Match { intent, rule } => {
                rec["outcome"] = json!("match");
                rec["intent"] = json!(intent);
                rec["rule"] = json!(rule);
            }
            RuleOutcome::Ambiguous(intents) => {
                rec["outcome"] = json!("ambiguous");
                rec["intents"] = json!(intents);
            }
            RuleOutcome::Negated => rec["outcome"] = json!("negated"),
            RuleOutcome::NoMatch => rec["outcome"] = json!("no_match"),
        }
        writeln!(w, "{rec}")?;
    }
    w.flush()?;
    Ok(())
}
