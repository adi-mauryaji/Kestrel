//! Kestrel-Tasks-v0 consistency (ADR-023; Week 9 "Done": every malformed fixture rejected).
//! Reads what experiments/data/tasks_v0.py built. Every malformed plan must be rejected with its
//! stated reason. Every gold plan must validate and, run by the deterministic runner (MarkedItems,
//! the user answering yes) on a fresh copy of the fixture notes and seed state, reach exactly the
//! expected end state. This checks the dataset, not a model.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kestrel_core::KestrelError;
use kestrel_core::resources::{CancelToken, Deadline};
use kestrel_tools::backend::{FixtureBackend, Task};
use kestrel_tools::plan::{self, Data, Kind, NodeStatus};
use kestrel_tools::runner::{MarkedItems, Runner};
use kestrel_tools::sandbox::Sandbox;
use kestrel_tools::tools::{Confirm, Policy};
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

struct Yes;
impl Confirm for Yes {
    fn ask(&self, _: &str) -> bool {
        true
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let dest = to.join(p.file_name().unwrap());
        if p.is_dir() {
            copy_dir(&p, &dest);
        } else {
            fs::copy(&p, &dest).unwrap();
        }
    }
}

fn seed_tasks(fixture: &Value) -> Vec<Task> {
    fixture["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| Task {
            id: t["id"].as_u64().unwrap(),
            title: t["title"].as_str().unwrap().into(),
            due: t["due"].as_str().map(Into::into),
        })
        .collect()
}

type Pair = (String, Option<String>);

#[test]
fn every_gold_plan_reaches_its_expected_end_state() {
    let fixture: Value =
        serde_json::from_str(&fs::read_to_string(build("fixture.json")).unwrap()).unwrap();
    let notes = Path::new(ROOT).join(fixture["notes"].as_str().unwrap());
    let memories: Vec<String> = fixture["memories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap().into())
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut checked = 0;
    for item in jsonl("items.jsonl") {
        let fam = item["family"].as_str().unwrap().to_string();
        if !seen.insert(fam.clone()) || item["plan"].is_null() {
            continue; // one run per family: the plan does not depend on the language
        }
        let exp = &item["expect"];
        let p = plan::parse(&item["plan"].to_string())
            .unwrap_or_else(|e| panic!("{fam}: gold plan invalid: {e}"));

        let dir =
            std::env::temp_dir().join(format!("kestrel_tasks_v0_{fam}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        copy_dir(&notes, &dir);
        let mut backend = FixtureBackend::new(Sandbox::new(std::slice::from_ref(&dir)).unwrap())
            .with_tasks(seed_tasks(&fixture))
            .with_memories(memories.clone());
        backend.begin_request(1);
        let mut text = MarkedItems;
        let policy = Policy {
            user_stated_preference: true,
        };
        let mut r = Runner {
            store: &mut backend,
            text: &mut text,
            confirm: &Yes,
            policy,
            batch_confirm_above: 10,
            log: vec![],
        };
        let ex = plan::execute(
            &p,
            &CancelToken::new(),
            &Deadline::after(Duration::from_secs(60)),
            |n, i, a| r.run(n, i, a),
        );

        let outcome = exp["outcome"].as_str().unwrap();
        match outcome {
            "done" => assert!(
                ex.status.iter().all(|s| *s == NodeStatus::Done),
                "{fam}: {}",
                ex.summary(&p)
            ),
            "refused" => assert!(
                ex.errors.iter().flatten().any(|e| e.contains("sandbox")),
                "{fam}: expected a sandbox refusal, got {}",
                ex.summary(&p)
            ),
            other => panic!("{fam}: outcome {other} with a plan"),
        }

        let deleted: Vec<u64> = exp["tasks_deleted"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        let mut want: Vec<Pair> = seed_tasks(&fixture)
            .into_iter()
            .filter(|t| !deleted.contains(&t.id))
            .map(|t| (t.title, t.due))
            .collect();
        want.extend(exp["tasks_added"].as_array().unwrap().iter().map(|t| {
            (
                t["title"].as_str().unwrap().to_string(),
                t["due"].as_str().map(Into::into),
            )
        }));
        let got: Vec<Pair> = backend
            .tasks()
            .iter()
            .map(|t| (t.title.clone(), t.due.clone()))
            .collect();
        assert_eq!(got, want, "{fam}: tasks");

        let prefs: BTreeMap<String, String> = exp["prefs_set"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().into()))
            .collect();
        assert_eq!(backend.prefs(), &prefs, "{fam}: preferences");

        let read: Vec<String> = p
            .order
            .iter()
            .filter(|&&i| p.nodes[i].op.io().1 == Kind::Notes)
            .filter_map(|&i| match &ex.outputs[i] {
                Some(Data::Notes(ns)) => {
                    Some(ns.iter().map(|n| n.name.clone()).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();
        let want_read: Vec<String> = exp["read"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().into())
            .collect();
        assert_eq!(read, want_read, "{fam}: notes read");

        if let Some(want) = exp["answer_contains"].as_str() {
            let texts: Vec<&str> = ex
                .outputs
                .iter()
                .flatten()
                .filter_map(|d| match d {
                    Data::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                texts.iter().any(|t| t.contains(want)),
                "{fam}: {want:?} not in {texts:?}"
            );
        }
        fs::remove_dir_all(&dir).unwrap();
        checked += 1;
    }
    assert_eq!(checked, 42, "every family with a plan was run once"); // 44 families, 2 clarify
}
