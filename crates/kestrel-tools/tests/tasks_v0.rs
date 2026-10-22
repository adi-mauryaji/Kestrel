//! Kestrel-Tasks-v0 consistency (ADR-023; Week 9 "Done": every malformed fixture rejected).
//! Reads what experiments/data/tasks_v0.py built. Every malformed plan must be rejected with its
//! stated reason. Every gold plan must validate and, run by `eval::run` on a fresh copy of the
//! fixtures, satisfy `eval::judge` exactly. This checks the dataset, not a model.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use kestrel_core::KestrelError;
use kestrel_tools::eval::{self, Fixture};
use kestrel_tools::plan;
use serde_json::Value;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn build(name: &str) -> PathBuf {
    Path::new(ROOT)
        .join("datasets/kestrel_tasks_v0/build")
        .join(name)
}

fn jsonl(name: &str) -> Vec<Value> {
    fs::read_to_string(build(name))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn every_malformed_plan_is_rejected_with_its_reason() {
    let bad = jsonl("malformed.jsonl");
    assert!(bad.len() >= 25, "{}", bad.len());
    for m in bad {
        let (id, text, want) = (
            &m["id"],
            m["plan"].as_str().unwrap(),
            m["error"].as_str().unwrap(),
        );
        match plan::parse(text) {
            Err(KestrelError::Invalid(msg)) => {
                assert!(msg.contains(want), "{id}: {msg:?} should mention {want:?}")
            }
            other => panic!("{id} ({}) was not rejected: {other:?}", m["why"]),
        }
    }
}

#[test]
fn every_gold_plan_reaches_its_expected_end_state() {
    let fixture = Fixture::load(&build("fixture.json"), Path::new(ROOT)).unwrap();
    let mut seen = HashSet::new();
    let mut checked = 0;
    for item in jsonl("items.jsonl") {
        let fam = item["family"].as_str().unwrap();
        let key = format!("{fam}{}{}", item["plan"], item["expect"]);
        if !seen.insert(key) || item["plan"].is_null() {
            continue; // one run per distinct plan: most families share one across languages
        }
        let p = plan::parse(&item["plan"].to_string())
            .unwrap_or_else(|e| panic!("{fam}: gold plan invalid: {e}"));
        let scratch =
            std::env::temp_dir().join(format!("kestrel_tasks_v0_{fam}_{}", std::process::id()));
        let end = eval::run(&p, &fixture, &scratch).unwrap();
        let problems = eval::judge(&item["expect"], &fixture, &end);
        assert!(
            problems.is_empty(),
            "{fam} ({}): {problems:?}",
            item["lang"]
        );
        checked += 1;
    }
    assert_eq!(checked, 51, "every distinct gold plan was run"); // 42 families with a plan + 9 Hinglish titles (ADR-024)
}

#[test]
fn the_fixture_lists_every_note_and_document() {
    let fixture = Fixture::load(&build("fixture.json"), Path::new(ROOT)).unwrap();
    let docs = fixture.documents();
    assert_eq!(docs.len(), 12, "{docs:?}");
    assert_eq!(
        fixture.tags(),
        [
            "books", "budget", "meeting", "personal", "project", "retro", "travel", "vendor"
        ]
    );
    assert!(
        docs.contains(&"docs/project-plan.md".to_string())
            && docs.contains(&"2026-10-18-groceries.md".to_string())
    );
}
