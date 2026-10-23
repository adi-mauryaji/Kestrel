//! Runs a validated plan's nodes against the tools (Week 9a, ADR-023): the bridge between
//! `plan::execute` and `tools::execute`. Every tool call is re-validated as a `ToolCall` (paths go
//! through the sandbox here, at run time) and passes the confirmation choke point.
//!
//! - `create_task` without a title creates one task per item from its dep. All items are checked
//!   as titles before any task exists, and a batch larger than `batch_confirm_above` asks once
//!   (Ch. 15.1). A retried batch creates no duplicates: task creation is idempotent per request.
//! - `summarize` and `extract_action_items` come from a `TextOps`. `MarkedItems` is the
//!   deterministic one (no model): the test double and the baseline until the SLM version (9d).
//! - The log is content-free: tool name, argument lengths, status, ms. An error is logged by its
//!   kind only, because its message can quote the input.

use std::time::Instant;

use kestrel_core::KestrelError;
use serde_json::{Value, json};

use crate::args::clean_text;
use crate::plan::{Data, Op, ValidNode};
use crate::sandbox::Sandbox;
use crate::tools::{self, Backend, Confirm, MAX_TITLE, Note, Policy, ToolCall, ToolOutput};

pub const MAX_SUMMARY: usize = 2000;
pub const MAX_ITEMS: usize = 20;

/// A backend that can also list recent notes and resolve paths.
pub trait Store: Backend {
    fn sandbox(&self) -> &Sandbox;
    fn recent(&self, n: u32, tag: Option<&str>) -> Result<Vec<Note>, KestrelError>;
}

pub trait TextOps {
    fn summarize(&mut self, inputs: &[&Data]) -> Result<String, KestrelError>;
    fn extract_action_items(&mut self, inputs: &[&Data]) -> Result<Vec<String>, KestrelError>;
}

/// Deterministic text ops: the summary is the first non-empty line of every note or text; the
/// action items are lines written `- [ ] ...`, `action: ...` or `todo: ...` (any case).
pub struct MarkedItems;

impl TextOps for MarkedItems {
    fn summarize(&mut self, inputs: &[&Data]) -> Result<String, KestrelError> {
        let firsts: Vec<&str> = texts(inputs)
            .into_iter()
            .filter_map(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
            .collect();
        if firsts.is_empty() {
            return Err(KestrelError::Invalid("nothing to summarize".into()));
        }
        Ok(firsts.join(" "))
    }

    fn extract_action_items(&mut self, inputs: &[&Data]) -> Result<Vec<String>, KestrelError> {
        let mut items: Vec<String> = Vec::new();
        for text in texts(inputs) {
            for line in text.lines().map(str::trim) {
                let item = ["- [ ]", "action:", "todo:"].iter().find_map(|m| {
                    let head = line.get(..m.len())?;
                    head.eq_ignore_ascii_case(m).then(|| line[m.len()..].trim())
                });
                if let Some(i) = item.filter(|i| !i.is_empty())
                    && !items.iter().any(|x| x == i)
                {
                    items.push(i.to_string());
                }
            }
        }
        Ok(items)
    }
}

/// The texts of notes and text inputs, in input order.
fn texts<'d>(inputs: &[&'d Data]) -> Vec<&'d str> {
    let mut out = Vec::new();
    for d in inputs {
        match d {
            Data::Notes(ns) => out.extend(ns.iter().map(|n| n.text.as_str())),
            Data::Text(t) => out.push(t.as_str()),
            Data::Items(_) | Data::Done => {}
        }
    }
    out
}

pub struct Runner<'a> {
    pub store: &'a mut dyn Store,
    pub text: &'a mut dyn TextOps,
    pub confirm: &'a dyn Confirm,
    pub policy: Policy,
    /// A `create_task` batch larger than this asks once.
    pub batch_confirm_above: usize,
    /// Content-free records, one per tool call or text op.
    pub log: Vec<Value>,
}

impl Runner<'_> {
    /// For `plan::execute`: one node, its inputs, the attempt number.
    pub fn run(
        &mut self,
        node: &ValidNode,
        inputs: &[&Data],
        _attempt: u8,
    ) -> Result<Data, KestrelError> {
        let name = node.op.name();
        match &node.op {
            Op::FetchRecentNotes { n, tag } => {
                let t0 = Instant::now();
                let r = self.store.recent(*n, tag.as_deref());
                let chars = json!({"tag": tag.as_ref().map_or(0, |t| t.chars().count())});
                self.log.push(
                    json!({"tool": name, "arg_chars": chars, "status": status(&r), "ms": ms(t0)}),
                );
                Ok(Data::Notes(r?))
            }
            Op::SearchNotes { query, limit } => self.tool(
                name,
                ToolCall::SearchNotes {
                    query: query.clone(),
                    limit: *limit,
                },
            ),
            Op::ReadDocument { path } => {
                self.tool(name, ToolCall::ReadDocument { path: path.clone() })
            }
            Op::Summarize => {
                let t0 = Instant::now();
                let r = self.text.summarize(inputs).and_then(check_summary);
                self.log
                    .push(json!({"tool": name, "status": status(&r), "ms": ms(t0)}));
                Ok(Data::Text(r?))
            }
            Op::ExtractActionItems => {
                let t0 = Instant::now();
                let r = self.text.extract_action_items(inputs).and_then(check_items);
                let n = r.as_ref().map_or(0, Vec::len);
                self.log
                    .push(json!({"tool": name, "items": n, "status": status(&r), "ms": ms(t0)}));
                Ok(Data::Items(r?))
            }
            Op::CreateTask {
                title: Some(title),
                due,
            } => self.tool(
                name,
                ToolCall::CreateTask {
                    title: title.clone(),
                    due: due.clone(),
                },
            ),
            Op::CreateTask { title: None, due } => self.batch(inputs, due),
            Op::DeleteTask { id } => self.tool(name, ToolCall::DeleteTask { id: *id }),
            Op::RetrieveMemory { terms } => self.tool(
                name,
                ToolCall::RetrieveMemory {
                    terms: terms.clone(),
                },
            ),
            Op::UpdatePreference { key, value } => self.tool(
                name,
                ToolCall::UpdatePreference {
                    key: key.clone(),
                    value: value.clone(),
                },
            ),
        }
    }

    /// Validate (the sandbox resolves paths here), confirm if needed, run, log.
    fn tool(&mut self, name: &str, call: ToolCall) -> Result<Data, KestrelError> {
        let t0 = Instant::now();
        let v = match tools::validate(call, self.store.sandbox()) {
            Ok(v) => v,
            Err(e) => {
                self.log
                    .push(json!({"tool": name, "status": kind(&e), "ms": ms(t0)}));
                return Err(e);
            }
        };
        let r = tools::execute(&v, self.policy, self.confirm, self.store);
        self.log.push(tools::log_record(&v, &status(&r), ms(t0)));
        r.map(to_data)
    }

    /// One task per item: every title checked first, one question for a large batch.
    fn batch(&mut self, inputs: &[&Data], due: &Option<String>) -> Result<Data, KestrelError> {
        let Some(Data::Items(items)) = inputs.first() else {
            return Err(KestrelError::Invalid(
                "create_task without a title needs items".into(),
            ));
        };
        let calls = items
            .iter()
            .map(|t| {
                tools::validate(
                    ToolCall::CreateTask {
                        title: t.clone(),
                        due: due.clone(),
                    },
                    self.store.sandbox(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        if calls.len() > self.batch_confirm_above
            && !self.confirm.ask(&format!("create {} tasks?", calls.len()))
        {
            self.log.push(json!({"tool": "create_task", "batch": calls.len(), "status": "declined", "ms": 0.0}));
            return Err(KestrelError::Denied(format!(
                "a batch of {} tasks declined by the user",
                calls.len()
            )));
        }
        for c in &calls {
            let t0 = Instant::now();
            let r = tools::execute(c, self.policy, self.confirm, self.store);
            self.log.push(tools::log_record(c, &status(&r), ms(t0)));
            r?;
        }
        Ok(Data::Done)
    }
}

fn to_data(out: ToolOutput) -> Data {
    match out {
        ToolOutput::Notes(n) => Data::Notes(n),
        ToolOutput::Text(t) => Data::Text(t),
        ToolOutput::TaskId(_) | ToolOutput::Done => Data::Done,
    }
}

/// A summary is text the user reads: non-empty, bounded, no control characters but newlines.
fn check_summary(s: String) -> Result<String, KestrelError> {
    let t = s.trim();
    if t.is_empty()
        || t.chars().count() > MAX_SUMMARY
        || t.chars().any(|c| c.is_control() && c != '\n')
    {
        return Err(KestrelError::Invalid(format!(
            "summary: empty, over {MAX_SUMMARY} characters or control characters"
        )));
    }
    Ok(t.to_string())
}

/// Action items become task titles, so each must already be a valid title (Ch. 14.1).
fn check_items(items: Vec<String>) -> Result<Vec<String>, KestrelError> {
    if items.len() > MAX_ITEMS {
        return Err(KestrelError::Invalid(format!(
            "more than {MAX_ITEMS} action items"
        )));
    }
    let mut out: Vec<String> = Vec::new();
    for i in &items {
        let t = clean_text(i, MAX_TITLE, "action item")?;
        if !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}

fn status<T>(r: &Result<T, KestrelError>) -> String {
    match r {
        Ok(_) => "ok".into(),
        Err(KestrelError::Denied(_)) => "declined".into(),
        Err(e) => kind(e),
    }
}

/// An error's kind for the log, never its message.
fn kind(e: &KestrelError) -> String {
    let k = match e {
        KestrelError::Timeout { .. } => "timeout",
        KestrelError::ModelUnavailable(_) => "model_unavailable",
        KestrelError::Invalid(_) => "invalid",
        KestrelError::Sandbox(_) => "sandbox",
        KestrelError::Storage(_) => "storage",
        KestrelError::Denied(_) => "denied",
        KestrelError::Cancelled => "cancelled",
    };
    format!("error:{k}")
}

fn ms(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1e3
}

impl Store for crate::backend::FixtureBackend {
    fn sandbox(&self) -> &Sandbox {
        crate::backend::FixtureBackend::sandbox(self)
    }

    fn recent(&self, n: u32, tag: Option<&str>) -> Result<Vec<Note>, KestrelError> {
        crate::backend::FixtureBackend::recent(self, n, tag)
    }
}

impl Store for crate::backend::MemoryBackend {
    fn sandbox(&self) -> &Sandbox {
        crate::backend::MemoryBackend::sandbox(self)
    }

    fn recent(&self, n: u32, tag: Option<&str>) -> Result<Vec<Note>, KestrelError> {
        crate::backend::MemoryBackend::recent(self, n, tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{FixtureBackend, Task};
    use crate::plan::{self, NodeStatus};
    use crate::tools::ValidatedCall;
    use kestrel_core::resources::{CancelToken, Deadline};
    use std::cell::Cell;
    use std::fs;
    use std::time::Duration;

    fn meeting_notes(name: &str) -> FixtureBackend {
        let d = std::env::temp_dir().join(format!("kestrel_runner_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("2026-10-20-standup.md"),
            "tags: meeting\nStandup.\n- [ ] email the budget to asha\nAction: book the room",
        )
        .unwrap();
        fs::write(
            d.join("2026-10-21-retro.md"),
            "tags: meeting\nRetro went well.\nTODO: fix the build",
        )
        .unwrap();
        fs::write(
            d.join("2026-10-22-planning.md"),
            "tags: meeting\nPlanning.\n- [ ] email the budget to asha",
        )
        .unwrap();
        fs::write(
            d.join("2026-10-23-shopping.md"),
            "Groceries.\n- [ ] buy milk",
        )
        .unwrap();
        FixtureBackend::new(Sandbox::new(&[d]).unwrap())
    }

    struct Answer {
        yes: bool,
        asked: Cell<u32>,
    }
    impl Confirm for Answer {
        fn ask(&self, _: &str) -> bool {
            self.asked.set(self.asked.get() + 1);
            self.yes
        }
    }
    fn answer(yes: bool) -> Answer {
        Answer {
            yes,
            asked: Cell::new(0),
        }
    }

    const CH8: &str = r#"{"nodes": [
        {"id": "n1", "op": "fetch_recent_notes", "args": {"n": 3, "tag": "meeting"}},
        {"id": "n2", "op": "summarize", "deps": ["n1"]},
        {"id": "n3", "op": "extract_action_items", "deps": ["n1"]},
        {"id": "n4", "op": "create_task", "args": {"due": "2026-10-24"}, "deps": ["n3"]}]}"#;

    fn go(
        store: &mut dyn Store,
        confirm: &dyn Confirm,
        above: usize,
        json: &str,
    ) -> (plan::Execution, Vec<Value>) {
        let p = plan::parse(json).unwrap();
        let mut text = MarkedItems;
        let mut r = Runner {
            store,
            text: &mut text,
            confirm,
            policy: Policy::default(),
            batch_confirm_above: above,
            log: vec![],
        };
        let ex = plan::execute(
            &p,
            &CancelToken::new(),
            &Deadline::after(Duration::from_secs(60)),
            |n, i, a| r.run(n, i, a),
        );
        (ex, r.log)
    }

    fn titles(b: &FixtureBackend) -> Vec<(String, Option<String>)> {
        b.tasks()
            .iter()
            .map(|t| (t.title.clone(), t.due.clone()))
            .collect()
    }

    #[test]
    fn the_chapter_8_flow_creates_one_task_per_action_item() {
        let mut b = meeting_notes("a");
        let (ex, log) = go(&mut b, &answer(true), 10, CH8);
        assert!(
            ex.status.iter().all(|s| *s == NodeStatus::Done),
            "{:?}",
            ex.errors
        );
        let due = Some("2026-10-24".to_string());
        assert_eq!(
            titles(&b),
            [
                ("email the budget to asha".into(), due.clone()),
                ("fix the build".into(), due.clone()),
                ("book the room".into(), due)
            ]
        ); // newest notes first, one task per distinct item; the shopping note is not a meeting
        assert_eq!(
            ex.outputs[1],
            Some(Data::Text("Planning. Retro went well. Standup.".into()))
        );
        let s = serde_json::to_string(&log).unwrap();
        assert!(
            !s.contains("budget") && !s.contains("asha") && !s.contains("meeting"),
            "{s}"
        );
        assert_eq!(log.iter().filter(|r| r["tool"] == "create_task").count(), 3);
    }

    #[test]
    fn a_large_batch_asks_once_and_a_refusal_creates_nothing() {
        let mut b = meeting_notes("b");
        let no = answer(false);
        let (ex, _) = go(&mut b, &no, 2, CH8);
        assert_eq!((ex.status[3], no.asked.get()), (NodeStatus::Declined, 1));
        assert!(b.tasks().is_empty());
        let yes = answer(true);
        let (ex, _) = go(&mut b, &yes, 2, CH8);
        assert_eq!(
            (ex.status[3], yes.asked.get(), b.tasks().len()),
            (NodeStatus::Done, 1, 3)
        );
    }

    #[test]
    fn a_declined_delete_skips_what_depends_on_it() {
        let mut b = meeting_notes("c").with_tasks(vec![Task {
            id: 7,
            title: "old".into(),
            due: None,
        }]);
        let json = r#"{"nodes": [{"id": "d", "op": "delete_task", "args": {"id": 7}},
                                 {"id": "t", "op": "create_task", "args": {"title": "new"}, "deps": ["d"]}]}"#;
        let (ex, log) = go(&mut b, &answer(false), 10, json);
        assert_eq!(
            ex.status,
            [NodeStatus::Declined, NodeStatus::SkippedDueToDependency]
        );
        assert_eq!(titles(&b), [("old".to_string(), None)]);
        assert_eq!(log[0]["status"], "declined");
    }

    #[test]
    fn a_path_escape_fails_at_run_time_and_is_not_retried() {
        let mut b = meeting_notes("d");
        let json = r#"{"nodes": [{"id": "r", "op": "read_document", "args": {"path": "../../../etc/passwd"}, "max_retries": 3}]}"#;
        let p = plan::parse(json).unwrap(); // the plan only checks the text; the sandbox decides at run time
        let mut text = MarkedItems;
        let yes = answer(true);
        let mut r = Runner {
            store: &mut b,
            text: &mut text,
            confirm: &yes,
            policy: Policy::default(),
            batch_confirm_above: 10,
            log: vec![],
        };
        let mut attempts = 0;
        let ex = plan::execute(
            &p,
            &CancelToken::new(),
            &Deadline::after(Duration::from_secs(60)),
            |n, i, a| {
                attempts += 1;
                r.run(n, i, a)
            },
        );
        assert_eq!((ex.status[0], attempts), (NodeStatus::Failed, 1));
        assert!(ex.errors[0].as_deref().unwrap().contains("sandbox"));
        assert_eq!(r.log[0]["status"], "error:sandbox");
    }

    struct BadItems;
    impl TextOps for BadItems {
        fn summarize(&mut self, _: &[&Data]) -> Result<String, KestrelError> {
            Ok("s".into())
        }
        fn extract_action_items(&mut self, _: &[&Data]) -> Result<Vec<String>, KestrelError> {
            Ok(vec!["fine".into(), "bad\u{7}item".into()])
        }
    }

    #[test]
    fn one_invalid_item_fails_the_batch_before_any_task_exists() {
        let mut b = meeting_notes("e");
        let p = plan::parse(CH8).unwrap();
        let mut text = BadItems;
        let yes = answer(true);
        let mut r = Runner {
            store: &mut b,
            text: &mut text,
            confirm: &yes,
            policy: Policy::default(),
            batch_confirm_above: 10,
            log: vec![],
        };
        let ex = plan::execute(
            &p,
            &CancelToken::new(),
            &Deadline::after(Duration::from_secs(60)),
            |n, i, a| r.run(n, i, a),
        );
        assert_eq!(
            ex.status[2],
            NodeStatus::Failed,
            "the title check runs at extraction"
        );
        assert_eq!(ex.status[3], NodeStatus::SkippedDueToDependency);
        assert!(b.tasks().is_empty());
    }

    /// Fails the second create once (a transient storage error), then works.
    struct Flaky {
        inner: FixtureBackend,
        creates: u32,
    }
    impl Backend for Flaky {
        fn run(&mut self, c: &ValidatedCall) -> Result<ToolOutput, KestrelError> {
            if matches!(c, ValidatedCall::CreateTask { .. }) {
                self.creates += 1;
                if self.creates == 2 {
                    return Err(KestrelError::Storage("disk busy".into()));
                }
            }
            self.inner.run(c)
        }
    }
    impl Store for Flaky {
        fn sandbox(&self) -> &Sandbox {
            Store::sandbox(&self.inner)
        }
        fn recent(&self, n: u32, tag: Option<&str>) -> Result<Vec<Note>, KestrelError> {
            self.inner.recent(n, tag)
        }
    }

    #[test]
    fn a_retried_batch_creates_no_duplicates() {
        let mut inner = meeting_notes("f");
        inner.begin_request(1);
        let mut f = Flaky { inner, creates: 0 };
        let json = CH8.replace(r#""deps": ["n3"]}"#, r#""deps": ["n3"], "max_retries": 1}"#);
        let (ex, _) = go(&mut f, &answer(true), 10, &json);
        assert_eq!(ex.status[3], NodeStatus::Done);
        assert_eq!(
            f.inner.tasks().len(),
            3,
            "the first item was created once, not twice"
        );
    }

    #[test]
    fn marked_items_reads_notes_and_text() {
        let notes = Data::Notes(vec![Note {
            name: "a".into(),
            text: "\n  Title line\n- [ ] one\nACTION: two\nnot an item\ntodo:three".into(),
        }]);
        let text = Data::Text("Summary first.\n- [ ] one".into());
        let mut m = MarkedItems;
        assert_eq!(
            m.extract_action_items(&[&notes, &text]).unwrap(),
            ["one", "two", "three"]
        );
        assert_eq!(
            m.summarize(&[&notes, &text]).unwrap(),
            "Title line Summary first."
        );
        assert!(
            m.summarize(&[&Data::Notes(vec![])]).is_err(),
            "nothing to summarize"
        );
    }
}
