//! A model-proposed plan, validated and executed by deterministic code (Ch. 14, 16, A.10).
//!
//! Wire format: `{"nodes": [{"id", "op", "args"?, "deps"?, "max_retries"?}]}`. `op` + `args`
//! parse into the `Op` enum, so the enum is the allowlist and every argument is typed and checked
//! (ADR-023; A.10 ignored `args`). Data flows only along `deps`: each op says what it needs from
//! its deps (nothing, notes or text, or a list of items) and what it produces, and `validate`
//! checks the graph against that. Two writes to the same resource must be ordered by a
//! dependency path (Ch. 16).
//!
//! Execution is sequential in a deterministic topological order (Kahn; ties keep the original
//! order). A failed node marks every transitive dependent `SkippedDueToDependency`; they never run
//! on partial inputs. Changes from A.10: only transient errors are retried (a refusal, a sandbox
//! violation or bad input would fail the same way again, and asking a user twice is wrong);
//! cancellation and the deadline are checked before every node (Ch. 14.3); the result says what
//! was done, failed, declined, skipped or not run.

use std::collections::{BTreeSet, HashMap, HashSet};

use kestrel_core::KestrelError;
use kestrel_core::resources::{CancelToken, Deadline};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::args::{clamp, clean_text, iso_date};
use crate::tools::{MAX_PREF_VALUE, MAX_QUERY, MAX_TITLE, Note, pref_key};

pub const MAX_NODES: usize = 12;
pub const MAX_RETRIES: u8 = 3;
pub const MAX_ID: usize = 16;
pub const MAX_RECENT: u32 = 20;
pub const MAX_TAG: usize = 32;
pub const MAX_PATH: usize = 260;

/// The allowlisted operations with their arguments as the model writes them.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(
    tag = "op",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Op {
    FetchRecentNotes {
        n: u32,
        tag: Option<String>,
    },
    SearchNotes {
        query: String,
        limit: Option<u32>,
    },
    ReadDocument {
        path: String,
    },
    Summarize,
    ExtractActionItems,
    /// Without a title: one task per item from its dep.
    CreateTask {
        title: Option<String>,
        due: Option<String>,
    },
    DeleteTask {
        id: u64,
    },
    RetrieveMemory {
        terms: String,
    },
    UpdatePreference {
        key: String,
        value: String,
    },
}

/// What flows along a dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Nothing,
    Notes,
    Text,
    Items,
}

/// A node after validation: checked args, deps as indices.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidNode {
    pub id: String,
    pub op: Op,
    pub deps: Vec<usize>,
    pub max_retries: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidPlan {
    pub nodes: Vec<ValidNode>,
    /// Execution order: indices into `nodes`.
    pub order: Vec<usize>,
}

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Self::FetchRecentNotes { .. } => "fetch_recent_notes",
            Self::SearchNotes { .. } => "search_notes",
            Self::ReadDocument { .. } => "read_document",
            Self::Summarize => "summarize",
            Self::ExtractActionItems => "extract_action_items",
            Self::CreateTask { .. } => "create_task",
            Self::DeleteTask { .. } => "delete_task",
            Self::RetrieveMemory { .. } => "retrieve_memory",
            Self::UpdatePreference { .. } => "update_preference",
        }
    }

    /// (what it needs from its deps, what it produces).
    pub fn io(&self) -> (Need, Kind) {
        match self {
            Self::FetchRecentNotes { .. }
            | Self::SearchNotes { .. }
            | Self::ReadDocument { .. } => (Need::Nothing, Kind::Notes),
            Self::Summarize => (Need::Content, Kind::Text),
            Self::ExtractActionItems => (Need::Content, Kind::Items),
            Self::CreateTask { title: None, .. } => (Need::Items, Kind::Nothing),
            Self::CreateTask { .. } | Self::DeleteTask { .. } | Self::UpdatePreference { .. } => {
                (Need::Nothing, Kind::Nothing)
            }
            Self::RetrieveMemory { .. } => (Need::Nothing, Kind::Text),
        }
    }

    /// The same op with every argument checked (and clamped where Ch. 15.2 clamps).
    fn checked(self) -> Result<Self, KestrelError> {
        Ok(match self {
            Self::FetchRecentNotes { n, tag } => {
                if n == 0 {
                    return Err(KestrelError::Invalid(
                        "fetch_recent_notes: n must be at least 1".into(),
                    ));
                }
                let tag = tag.map(|t| clean_text(&t, MAX_TAG, "tag")).transpose()?;
                Self::FetchRecentNotes {
                    n: n.min(MAX_RECENT),
                    tag,
                }
            }
            Self::SearchNotes { query, limit } => Self::SearchNotes {
                query: clean_text(&query, MAX_QUERY, "query")?,
                limit: limit.map(|l| clamp(Some(l), 10, 1, 50)),
            },
            // Resolved through the Sandbox when it runs; here only the text is checked.
            Self::ReadDocument { path } => Self::ReadDocument {
                path: clean_text(&path, MAX_PATH, "path")?,
            },
            Self::CreateTask { title, due } => Self::CreateTask {
                title: title
                    .map(|t| clean_text(&t, MAX_TITLE, "title"))
                    .transpose()?,
                due: due.as_deref().map(iso_date).transpose()?,
            },
            Self::RetrieveMemory { terms } => Self::RetrieveMemory {
                terms: clean_text(&terms, MAX_QUERY, "terms")?,
            },
            Self::UpdatePreference { key, value } => Self::UpdatePreference {
                key: pref_key(&key)?,
                value: clean_text(&value, MAX_PREF_VALUE, "value")?,
            },
            other @ (Self::Summarize | Self::ExtractActionItems | Self::DeleteTask { .. }) => other,
        })
    }

    /// The resource a write op changes, if two of them could conflict (Ch. 16). Creating tasks
    /// makes new resources, so two creates never conflict.
    fn writes(&self) -> Option<String> {
        match self {
            Self::DeleteTask { id } => Some(format!("task:{id}")),
            Self::UpdatePreference { key, .. } => Some(format!("pref:{key}")),
            _ => None,
        }
    }
}

/// What an op needs from its deps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Deps only order it.
    Nothing,
    /// At least one dep, each giving notes or text.
    Content,
    /// Exactly one dep giving items.
    Items,
}

/// Parses and validates a plan; the error text is meant for the one re-prompt (Ch. 16).
pub fn parse(json: &str) -> Result<ValidPlan, KestrelError> {
    let raw: RawPlan =
        serde_json::from_str(json).map_err(|e| invalid(format!("plan JSON: {e}")))?;
    let n = raw.nodes.len();
    if n == 0 || n > MAX_NODES {
        return Err(invalid(format!(
            "a plan has 1..={MAX_NODES} nodes, this one has {n}"
        )));
    }
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, node) in raw.nodes.iter().enumerate() {
        let id = node.id.as_str();
        let ok = (1..=MAX_ID).contains(&id.len())
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !ok {
            return Err(invalid(format!(
                "node id {id:?}: 1..={MAX_ID} of a-z 0-9 _ -"
            )));
        }
        if index.insert(id, i).is_some() {
            return Err(invalid(format!("duplicate node id {id:?}")));
        }
    }
    let mut nodes = Vec::with_capacity(n);
    for (i, node) in raw.nodes.iter().enumerate() {
        let op = node
            .op()
            .and_then(Op::checked)
            .map_err(|e| invalid(format!("node {}: {}", node.id, msg(e))))?;
        if node.max_retries > MAX_RETRIES {
            return Err(invalid(format!(
                "node {}: max_retries {} > {MAX_RETRIES}",
                node.id, node.max_retries
            )));
        }
        let mut deps = Vec::new();
        for d in &node.deps {
            let &j = index.get(d.as_str()).ok_or_else(|| {
                invalid(format!("node {} depends on unknown node {d:?}", node.id))
            })?;
            if j == i {
                return Err(invalid(format!("node {} depends on itself", node.id)));
            }
            if !deps.contains(&j) {
                deps.push(j);
            }
        }
        nodes.push(ValidNode {
            id: node.id.clone(),
            op,
            deps,
            max_retries: node.max_retries,
        });
    }
    let order = topological(&nodes)?;
    for node in &nodes {
        let (need, _) = node.op.io();
        let gives: Vec<Kind> = node.deps.iter().map(|&d| nodes[d].op.io().1).collect();
        let fits = match need {
            Need::Nothing => true,
            Need::Content => {
                !gives.is_empty() && gives.iter().all(|k| matches!(k, Kind::Notes | Kind::Text))
            }
            Need::Items => gives == [Kind::Items],
        };
        if !fits {
            let wants = match need {
                Need::Content => "notes or text from at least one dep",
                _ => "items from exactly one dep (extract_action_items)",
            };
            return Err(invalid(format!(
                "node {}: {} needs {wants}",
                node.id,
                describe(&node.op)
            )));
        }
    }
    let ancestors = ancestors(&nodes, &order);
    for (a, na) in nodes.iter().enumerate() {
        for (b, nb) in nodes.iter().enumerate().skip(a + 1) {
            if let (Some(ra), Some(rb)) = (na.op.writes(), nb.op.writes())
                && ra == rb
                && !ancestors[a].contains(&b)
                && !ancestors[b].contains(&a)
            {
                return Err(invalid(format!(
                    "nodes {} and {} both write {ra} but neither depends on the other",
                    na.id, nb.id
                )));
            }
        }
    }
    Ok(ValidPlan { nodes, order })
}

fn invalid(m: String) -> KestrelError {
    KestrelError::Invalid(m)
}

/// The message of an error, without `Invalid`'s "invalid input: " prefix (re-wrapped above).
fn msg(e: KestrelError) -> String {
    match e {
        KestrelError::Invalid(m) => m,
        other => other.to_string(),
    }
}

fn describe(op: &Op) -> String {
    match op {
        Op::CreateTask { title: None, .. } => "create_task without a title".into(),
        other => other.name().into(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlan {
    nodes: Vec<RawNode>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNode {
    id: String,
    op: String,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    deps: Vec<String>,
    #[serde(default)]
    max_retries: u8,
}

impl RawNode {
    /// `op` + `args` as the `Op` enum. An op without arguments may come with `"args": {}`.
    fn op(&self) -> Result<Op, KestrelError> {
        let parse = |v: Value| serde_json::from_value::<Op>(v).map_err(|e| invalid(e.to_string()));
        match &self.args {
            None => parse(json!({"op": self.op})),
            Some(args) => parse(json!({"op": self.op, "args": args})).or_else(|e| {
                let empty = args.as_object().is_some_and(serde_json::Map::is_empty);
                if empty {
                    parse(json!({"op": self.op})).map_err(|_| e)
                } else {
                    Err(e)
                }
            }),
        }
    }
}

/// Kahn's algorithm; among ready nodes the one listed first goes first.
fn topological(nodes: &[ValidNode]) -> Result<Vec<usize>, KestrelError> {
    let mut indeg: Vec<usize> = nodes.iter().map(|n| n.deps.len()).collect();
    let mut ready: BTreeSet<usize> = (0..nodes.len()).filter(|&i| indeg[i] == 0).collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for (c, node) in nodes.iter().enumerate() {
            if node.deps.contains(&i) {
                indeg[c] -= 1;
                if indeg[c] == 0 {
                    ready.insert(c);
                }
            }
        }
    }
    if order.len() == nodes.len() {
        Ok(order)
    } else {
        Err(invalid("the plan contains a cycle".into()))
    }
}

/// Every node's transitive deps.
fn ancestors(nodes: &[ValidNode], order: &[usize]) -> Vec<HashSet<usize>> {
    let mut anc = vec![HashSet::new(); nodes.len()];
    for &i in order {
        let mut mine = HashSet::new();
        for &d in &nodes[i].deps {
            mine.insert(d);
            mine.extend(anc[d].iter().copied());
        }
        anc[i] = mine;
    }
    anc
}

/// What a node produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Notes(Vec<Note>),
    Text(String),
    Items(Vec<String>),
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Done,
    Failed,
    /// The user declined its confirmation.
    Declined,
    SkippedDueToDependency,
    /// Not started: the plan was cancelled or ran past its deadline.
    NotRun,
}

#[derive(Debug)]
pub struct Execution {
    /// Index-aligned with the plan's nodes.
    pub status: Vec<NodeStatus>,
    pub outputs: Vec<Option<Data>>,
    pub errors: Vec<Option<String>>,
    /// "cancelled" or "deadline" if execution stopped early.
    pub stopped: Option<&'static str>,
}

/// Runs the nodes in order. `run(node, inputs from its deps in dep order, attempt)`.
pub fn execute(
    plan: &ValidPlan,
    cancel: &CancelToken,
    deadline: &Deadline,
    mut run: impl FnMut(&ValidNode, &[&Data], u8) -> Result<Data, KestrelError>,
) -> Execution {
    let n = plan.nodes.len();
    let mut ex = Execution {
        status: vec![NodeStatus::NotRun; n],
        outputs: vec![None; n],
        errors: vec![None; n],
        stopped: None,
    };
    for &i in &plan.order {
        if ex.stopped.is_none() {
            ex.stopped = if cancel.is_cancelled() {
                Some("cancelled")
            } else if deadline.expired() {
                Some("deadline")
            } else {
                None
            };
        }
        if ex.stopped.is_some() {
            continue; // stays NotRun
        }
        let node = &plan.nodes[i];
        if node.deps.iter().any(|&d| ex.status[d] != NodeStatus::Done) {
            ex.status[i] = NodeStatus::SkippedDueToDependency;
            continue;
        }
        let result = {
            let inputs: Vec<&Data> = node
                .deps
                .iter()
                .filter_map(|&d| ex.outputs[d].as_ref())
                .collect();
            let mut attempt = 0;
            loop {
                match run(node, &inputs, attempt) {
                    Err(e) if transient(&e) && attempt < node.max_retries => attempt += 1,
                    other => break other,
                }
            }
        };
        match result {
            Ok(d) => {
                ex.status[i] = NodeStatus::Done;
                ex.outputs[i] = Some(d);
            }
            Err(e) => {
                ex.status[i] = if matches!(e, KestrelError::Denied(_)) {
                    NodeStatus::Declined
                } else {
                    NodeStatus::Failed
                };
                if matches!(e, KestrelError::Cancelled) {
                    ex.stopped = Some("cancelled");
                }
                ex.errors[i] = Some(e.to_string());
            }
        }
    }
    ex
}

/// Worth another attempt: the same call may succeed later. Bad input, a sandbox violation, a
/// refusal or a cancellation would not.
fn transient(e: &KestrelError) -> bool {
    matches!(
        e,
        KestrelError::Timeout { .. } | KestrelError::ModelUnavailable(_) | KestrelError::Storage(_)
    )
}

impl Execution {
    /// The user-facing account (Ch. 16: failure is a visible outcome, never a silent success).
    pub fn summary(&self, plan: &ValidPlan) -> String {
        let ids = |want: NodeStatus| -> Vec<&str> {
            plan.order
                .iter()
                .filter(|&&i| self.status[i] == want)
                .map(|&i| plan.nodes[i].id.as_str())
                .collect()
        };
        let done = ids(NodeStatus::Done).len();
        let mut parts = vec![format!("{done} of {} steps done", plan.nodes.len())];
        for &i in &plan.order {
            let (id, op) = (&plan.nodes[i].id, plan.nodes[i].op.name());
            match self.status[i] {
                NodeStatus::Failed => parts.push(format!(
                    "{id} ({op}) failed: {}",
                    self.errors[i].as_deref().unwrap_or("?")
                )),
                NodeStatus::Declined => parts.push(format!("{id} ({op}) declined")),
                _ => {}
            }
        }
        let skipped = ids(NodeStatus::SkippedDueToDependency);
        if !skipped.is_empty() {
            parts.push(format!("skipped: {}", skipped.join(", ")));
        }
        if let Some(why) = self.stopped {
            parts.push(format!(
                "stopped ({why}): {} not run",
                ids(NodeStatus::NotRun).join(", ")
            ));
        }
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const CH8: &str = r#"{"nodes": [
        {"id": "n1", "op": "fetch_recent_notes", "args": {"n": 3, "tag": "meeting"}},
        {"id": "n2", "op": "summarize", "deps": ["n1"]},
        {"id": "n3", "op": "extract_action_items", "deps": ["n2"]},
        {"id": "n4", "op": "create_task", "args": {}, "deps": ["n3"], "max_retries": 1}]}"#;

    fn err(json: &str) -> String {
        match parse(json) {
            Err(KestrelError::Invalid(m)) => m,
            other => panic!("expected Invalid for {json}, got {other:?}"),
        }
    }

    #[test]
    fn the_chapter_8_plan_validates_in_order() {
        let p = parse(CH8).unwrap();
        assert_eq!(p.order, vec![0, 1, 2, 3]);
        assert_eq!(
            p.nodes[0].op,
            Op::FetchRecentNotes {
                n: 3,
                tag: Some("meeting".into())
            }
        );
        assert_eq!(
            p.nodes[3].op,
            Op::CreateTask {
                title: None,
                due: None
            }
        );
        assert_eq!(
            (p.nodes[3].deps.clone(), p.nodes[3].max_retries),
            (vec![2], 1)
        );
    }

    #[test]
    fn independent_nodes_keep_their_original_order() {
        let p = parse(
            r#"{"nodes": [
            {"id": "s", "op": "summarize", "deps": ["b", "a"]},
            {"id": "b", "op": "search_notes", "args": {"query": "budget"}},
            {"id": "a", "op": "read_document", "args": {"path": "plan.md"}}]}"#,
        )
        .unwrap();
        assert_eq!(p.order, vec![1, 2, 0]);
        assert_eq!(p.nodes[0].deps, vec![1, 2]); // dep order kept: inputs arrive as listed
    }

    #[test]
    fn rejects_malformed_structure() {
        let cases = [
            (r#"{"nodes": []}"#, "1..=12"),
            (
                r#"{"nodes": [{"id": "a", "op": "run_shell", "args": {"cmd": "rm -rf /"}}]}"#,
                "run_shell",
            ),
            (
                r#"{"nodes": [{"id": "a", "op": "summarize", "cmd": "x"}]}"#,
                "cmd",
            ),
            (
                r#"{"nodes": [{"id": "a", "op": "summarize"}], "extra": 1}"#,
                "extra",
            ),
            (
                r#"{"nodes": [{"id": "a", "op": "search_notes", "args": {"query": "q"}},
                           {"id": "a", "op": "search_notes", "args": {"query": "q"}}]}"#,
                "duplicate",
            ),
            (
                r#"{"nodes": [{"id": "A b", "op": "search_notes", "args": {"query": "q"}}]}"#,
                "id",
            ),
            (
                r#"{"nodes": [{"id": "a", "op": "summarize", "deps": ["zzz"]}]}"#,
                "unknown",
            ),
            (
                r#"{"nodes": [{"id": "a", "op": "summarize", "deps": ["a"]}]}"#,
                "itself",
            ),
            (
                r#"{"nodes": [{"id": "a", "op": "summarize", "deps": ["b"]},
                           {"id": "b", "op": "summarize", "deps": ["a"]}]}"#,
                "cycle",
            ),
            (
                r#"{"nodes": [{"id": "a", "op": "search_notes", "args": {"query": "q"}, "max_retries": 4}]}"#,
                "max_retries",
            ),
            ("not json", "plan JSON"),
        ];
        for (json, want) in cases {
            let m = err(json);
            assert!(m.contains(want), "{json}: {m:?} should mention {want:?}");
        }
        let big: Vec<String> = (0..13)
            .map(|i| format!(r#"{{"id": "n{i}", "op": "search_notes", "args": {{"query": "q"}}}}"#))
            .collect();
        assert!(err(&format!(r#"{{"nodes": [{}]}}"#, big.join(","))).contains("1..=12"));
    }

    #[test]
    fn rejects_bad_arguments() {
        for json in [
            r#"{"nodes": [{"id": "a", "op": "create_task", "args": {"title": "x", "due": "kal"}}]}"#,
            r#"{"nodes": [{"id": "a", "op": "create_task", "args": {"title": "a\u0007b"}}]}"#,
            r#"{"nodes": [{"id": "a", "op": "search_notes", "args": {"query": " "}}]}"#,
            r#"{"nodes": [{"id": "a", "op": "search_notes", "args": {"query": "q", "rm": 1}}]}"#,
            r#"{"nodes": [{"id": "a", "op": "fetch_recent_notes", "args": {"n": 0}}]}"#,
            r#"{"nodes": [{"id": "a", "op": "update_preference", "args": {"key": "Bad Key", "value": "v"}}]}"#,
            r#"{"nodes": [{"id": "a", "op": "delete_task", "args": {"id": "seven"}}]}"#,
            r#"{"nodes": [{"id": "a", "op": "create_task"}]}"#,
        ] {
            err(json);
        }
        let p = parse(r#"{"nodes": [{"id": "a", "op": "fetch_recent_notes", "args": {"n": 500}},
                                    {"id": "b", "op": "search_notes", "args": {"query": " q ", "limit": 0}}]}"#)
        .unwrap();
        assert_eq!(
            p.nodes[0].op,
            Op::FetchRecentNotes {
                n: MAX_RECENT,
                tag: None
            }
        ); // clamped
        assert_eq!(
            p.nodes[1].op,
            Op::SearchNotes {
                query: "q".into(),
                limit: Some(1)
            }
        );
    }

    #[test]
    fn data_must_fit_along_each_edge() {
        let cases = [
            (
                r#"{"nodes": [{"id": "s", "op": "summarize"}]}"#,
                "summarize",
            ),
            (
                r#"{"nodes": [{"id": "t", "op": "create_task", "args": {"title": "x"}},
                           {"id": "s", "op": "summarize", "deps": ["t"]}]}"#,
                "summarize",
            ),
            (
                r#"{"nodes": [{"id": "n", "op": "search_notes", "args": {"query": "q"}},
                           {"id": "t", "op": "create_task", "args": {}, "deps": ["n"]}]}"#,
                "items",
            ),
            (
                r#"{"nodes": [{"id": "t", "op": "create_task", "args": {}}]}"#,
                "items",
            ),
        ];
        for (json, want) in cases {
            let m = err(json);
            assert!(m.contains(want), "{json}: {m:?}");
        }
        // Deps of an op that needs nothing only order it; extraction may read notes directly.
        parse(r#"{"nodes": [{"id": "d", "op": "delete_task", "args": {"id": 7}},
                            {"id": "t", "op": "create_task", "args": {"title": "x"}, "deps": ["d"]}]}"#)
        .unwrap();
        parse(
            r#"{"nodes": [{"id": "n", "op": "fetch_recent_notes", "args": {"n": 2}},
                            {"id": "x", "op": "extract_action_items", "deps": ["n"]}]}"#,
        )
        .unwrap();
    }

    #[test]
    fn two_writes_to_one_resource_must_be_ordered() {
        let unordered = r#"{"nodes": [{"id": "a", "op": "delete_task", "args": {"id": 7}},
                                      {"id": "b", "op": "delete_task", "args": {"id": 7}}]}"#;
        assert!(err(unordered).contains("task:7"));
        parse(&unordered.replace(
            r#""args": {"id": 7}}]"#,
            r#""args": {"id": 7}, "deps": ["a"]}]"#,
        ))
        .unwrap();
        let prefs = r#"{"nodes": [{"id": "a", "op": "update_preference", "args": {"key": "lang", "value": "hi"}},
                                  {"id": "b", "op": "update_preference", "args": {"key": "lang", "value": "en"}}]}"#;
        assert!(err(prefs).contains("pref:lang"));
        parse(
            r#"{"nodes": [{"id": "a", "op": "delete_task", "args": {"id": 7}},
                            {"id": "b", "op": "delete_task", "args": {"id": 8}}]}"#,
        )
        .unwrap();
    }

    fn run_all(
        p: &ValidPlan,
        run: impl FnMut(&ValidNode, &[&Data], u8) -> Result<Data, KestrelError>,
    ) -> Execution {
        execute(
            p,
            &CancelToken::new(),
            &Deadline::after(Duration::from_secs(60)),
            run,
        )
    }

    fn ok_data(n: &ValidNode) -> Data {
        match n.op.io().1 {
            Kind::Notes => Data::Notes(vec![Note {
                name: "a.md".into(),
                text: "x".into(),
            }]),
            Kind::Text => Data::Text("summary".into()),
            Kind::Items => Data::Items(vec!["call asha".into()]),
            Kind::Nothing => Data::Done,
        }
    }

    #[test]
    fn a_failure_skips_every_dependent_and_they_never_run() {
        let p = parse(CH8).unwrap();
        let mut calls = vec![];
        let ex = run_all(&p, |n, _, attempt| {
            calls.push((n.id.clone(), attempt));
            if n.id == "n2" {
                Err(KestrelError::Storage("disk".into()))
            } else {
                Ok(ok_data(n))
            }
        });
        use NodeStatus::*;
        assert_eq!(
            ex.status,
            vec![Done, Failed, SkippedDueToDependency, SkippedDueToDependency]
        );
        assert!(
            !calls.iter().any(|(id, _)| id == "n3" || id == "n4"),
            "dependents must never run"
        );
        assert_eq!(
            ex.summary(&p),
            "1 of 4 steps done; n2 (summarize) failed: storage error: disk; skipped: n3, n4"
        );
    }

    #[test]
    fn transient_errors_are_retried_up_to_the_limit_and_refusals_never() {
        let p = parse(CH8).unwrap();
        let ex = run_all(&p, |n, _, attempt| {
            if n.id == "n4" && attempt == 0 {
                Err(KestrelError::Storage("busy".into()))
            } else {
                Ok(ok_data(n))
            }
        });
        assert_eq!(ex.status[3], NodeStatus::Done);
        assert_eq!(ex.summary(&p), "4 of 4 steps done");
        let mut attempts = 0;
        let ex = run_all(&p, |n, _, _| {
            if n.id == "n4" {
                attempts += 1;
                Err(KestrelError::Storage("down".into()))
            } else {
                Ok(ok_data(n))
            }
        });
        assert_eq!((ex.status[3], attempts), (NodeStatus::Failed, 2)); // max_retries 1 = 2 attempts
        let mut asked = 0;
        let ex = run_all(&p, |n, _, _| {
            if n.id == "n4" {
                asked += 1;
                Err(KestrelError::Denied("declined".into()))
            } else {
                Ok(ok_data(n))
            }
        });
        assert_eq!((ex.status[3], asked), (NodeStatus::Declined, 1));
    }

    #[test]
    fn inputs_arrive_from_the_deps_in_order() {
        let p = parse(
            r#"{"nodes": [
            {"id": "a", "op": "search_notes", "args": {"query": "a"}},
            {"id": "b", "op": "search_notes", "args": {"query": "b"}},
            {"id": "s", "op": "summarize", "deps": ["b", "a"]}]}"#,
        )
        .unwrap();
        let mut seen = vec![];
        run_all(&p, |n, inputs, _| {
            if n.id == "s" {
                seen = inputs.iter().map(|d| format!("{d:?}")).collect();
                return Ok(Data::Text("s".into()));
            }
            Ok(Data::Notes(vec![Note {
                name: n.id.clone(),
                text: String::new(),
            }]))
        });
        assert!(
            seen[0].contains("\"b\"") && seen[1].contains("\"a\""),
            "{seen:?}"
        );
    }

    #[test]
    fn cancellation_and_the_deadline_stop_before_the_next_node() {
        let p = parse(CH8).unwrap();
        let cancel = CancelToken::new();
        let deadline = Deadline::after(Duration::from_secs(60));
        let mut ran = vec![];
        let ex = execute(&p, &cancel, &deadline, |n, _, _| {
            ran.push(n.id.clone());
            if n.id == "n2" {
                cancel.cancel(); // e.g. the user pressed Ctrl-C during the summary
            }
            Ok(ok_data(n))
        });
        use NodeStatus::*;
        assert_eq!(ran, ["n1", "n2"]);
        assert_eq!(ex.status, vec![Done, Done, NotRun, NotRun]);
        assert_eq!(ex.stopped, Some("cancelled"));
        let ex = execute(
            &p,
            &CancelToken::new(),
            &Deadline::after(Duration::ZERO),
            |n, _, _| Ok(ok_data(n)),
        );
        assert_eq!((ex.status[0], ex.stopped), (NotRun, Some("deadline")));
        assert!(
            ex.summary(&p).contains("stopped (deadline)"),
            "{}",
            ex.summary(&p)
        );
    }
}
