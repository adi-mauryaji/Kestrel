//! Judging a plan on Kestrel-Tasks-v0 (ADR-023): run it on a fresh copy of the fixture notes and
//! the seed state, with the deterministic text ops and a user who answers yes, then compare the end
//! state with the item's expectation. Used by the dataset check (gold plans) and by the template
//! and planner evaluations, so every plan is judged the same way.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kestrel_core::KestrelError;
use kestrel_core::resources::{CancelToken, Deadline};
use serde_json::Value;

use crate::backend::{FixtureBackend, Task};
use crate::date::Date;
use crate::plan::{self, Data, Kind, NodeStatus, ValidPlan};
use crate::runner::{MarkedItems, Runner};
use crate::sandbox::Sandbox;
use crate::tools::{Confirm, Policy};

/// build/fixture.json, with the notes folder resolved against the repository root.
pub struct Fixture {
    pub today: Date,
    pub notes: PathBuf,
    pub tasks: Vec<Task>,
    pub memories: Vec<String>,
}

fn bad(m: String) -> KestrelError {
    KestrelError::Invalid(m)
}

impl Fixture {
    pub fn load(fixture_json: &Path, repo_root: &Path) -> Result<Self, KestrelError> {
        let text =
            fs::read_to_string(fixture_json).map_err(|e| KestrelError::Storage(e.to_string()))?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| bad(format!("fixture.json: {e}")))?;
        let s = |k: &str| {
            v[k].as_str()
                .ok_or_else(|| bad(format!("fixture.json: no {k}")))
        };
        let tasks = v["tasks"]
            .as_array()
            .ok_or_else(|| bad("fixture.json: no tasks".into()))?
            .iter()
            .map(|t| Task {
                id: t["id"].as_u64().unwrap_or_default(),
                title: t["title"].as_str().unwrap_or_default().into(),
                due: t["due"].as_str().map(Into::into),
            })
            .collect();
        let memories = v["memories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_str().map(Into::into))
            .collect();
        Ok(Self {
            today: Date::parse(s("today")?).ok_or_else(|| bad("fixture.json: today".into()))?,
            notes: repo_root.join(s("notes")?),
            tasks,
            memories,
        })
    }

    /// Every note and document name under the notes folder (relative, '/'-separated, sorted).
    pub fn documents(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk(&self.notes, &self.notes, &mut out);
        out.sort();
        out
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let p = e?.path();
        let dest = to.join(p.file_name().unwrap_or_default());
        if p.is_dir() {
            copy_dir(&p, &dest)?;
        } else {
            fs::copy(&p, &dest)?;
        }
    }
    Ok(())
}

struct Yes;
impl Confirm for Yes {
    fn ask(&self, _: &str) -> bool {
        true
    }
}

/// What a run left behind.
#[derive(Debug)]
pub struct EndState {
    pub status: Vec<NodeStatus>,
    pub errors: Vec<Option<String>>,
    pub summary: String,
    pub tasks: Vec<(String, Option<String>)>,
    pub prefs: BTreeMap<String, String>,
    /// Note names from the notes-producing steps, in execution order.
    pub read: Vec<String>,
    pub texts: Vec<String>,
}

/// Runs `plan` on a fresh copy of the fixture notes made under `scratch` (removed afterwards).
pub fn run(plan: &ValidPlan, fixture: &Fixture, scratch: &Path) -> Result<EndState, KestrelError> {
    let _ = fs::remove_dir_all(scratch);
    copy_dir(&fixture.notes, scratch).map_err(|e| KestrelError::Storage(e.to_string()))?;
    let mut backend = FixtureBackend::new(Sandbox::new(&[scratch.to_path_buf()])?)
        .with_tasks(fixture.tasks.clone())
        .with_memories(fixture.memories.clone());
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
        plan,
        &CancelToken::new(),
        &Deadline::after(Duration::from_secs(60)),
        |n, i, a| r.run(n, i, a),
    );
    let read = plan
        .order
        .iter()
        .filter(|&&i| plan.nodes[i].op.io().1 == Kind::Notes)
        .filter_map(|&i| match &ex.outputs[i] {
            Some(Data::Notes(ns)) => Some(ns.iter().map(|n| n.name.clone()).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    let texts = ex.outputs.iter().flatten().filter_map(|d| match d {
        Data::Text(t) => Some(t.clone()),
        _ => None,
    });
    let end = EndState {
        summary: ex.summary(plan),
        status: ex.status.clone(),
        errors: ex.errors.clone(),
        tasks: backend
            .tasks()
            .iter()
            .map(|t| (t.title.clone(), t.due.clone()))
            .collect(),
        prefs: backend.prefs().clone(),
        read,
        texts: texts.collect(),
    };
    let _ = fs::remove_dir_all(scratch);
    Ok(end)
}

/// Every way `end` differs from the item's expectation (empty = the item succeeded).
pub fn judge(expect: &Value, fixture: &Fixture, end: &EndState) -> Vec<String> {
    let mut problems = Vec::new();
    match expect["outcome"].as_str().unwrap_or("done") {
        "done" if end.status.iter().all(|s| *s == NodeStatus::Done) => {}
        "done" => problems.push(format!("not every step done: {}", end.summary)),
        "refused" if end.errors.iter().flatten().any(|e| e.contains("sandbox")) => {}
        "refused" => problems.push(format!("expected a sandbox refusal: {}", end.summary)),
        other => problems.push(format!("outcome {other} cannot be judged on a run")),
    }
    let ids = |k: &str| -> Vec<u64> {
        expect[k]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect()
    };
    let deleted = ids("tasks_deleted");
    let mut want: Vec<(String, Option<String>)> = fixture
        .tasks
        .iter()
        .filter(|t| !deleted.contains(&t.id))
        .map(|t| (t.title.clone(), t.due.clone()))
        .collect();
    want.extend(
        expect["tasks_added"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                (
                    t["title"].as_str().unwrap_or_default().to_string(),
                    t["due"].as_str().map(Into::into),
                )
            }),
    );
    if end.tasks != want {
        problems.push(format!("tasks {:?}, expected {want:?}", end.tasks));
    }
    let prefs: BTreeMap<String, String> = expect["prefs_set"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().into()))
        .collect();
    if end.prefs != prefs {
        problems.push(format!("preferences {:?}, expected {prefs:?}", end.prefs));
    }
    let read: Vec<String> = expect["read"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(Into::into))
        .collect();
    if end.read != read {
        problems.push(format!("read {:?}, expected {read:?}", end.read));
    }
    if let Some(want) = expect["answer_contains"].as_str()
        && !end.texts.iter().any(|t| t.contains(want))
    {
        problems.push(format!("no answer contains {want:?}"));
    }
    problems
}
