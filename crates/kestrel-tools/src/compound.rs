//! Compound requests without a model where possible (ADR-025): split the request at the router's
//! conjunctions, let the cascade route each part, plan each part with its template, and join the
//! plans. The cascade idea applied to planning: deterministic steps first, the SLM planner only
//! for what they cannot do.
//!
//! - Split at `kestrel_core::features::CONJUNCTIONS` (and, then, aur, phir, fir). In Hinglish a
//!   shared predicate after "dono" / "both" belongs to every part ("task 3 aur task 2 dono
//!   delete karo" -> "task 3 delete karo", "task 2 delete karo").
//! - A part that asks for tasks from what came before ("create a task list", "unse tasks bana
//!   do") becomes extract_action_items on the previous part's notes, then create_task.
//! - A missing slot in any part makes the whole request ask (`Clarify`); a part without a
//!   template (`NeedsModel`, `NoAction`, or a link with no notes before it) hands the whole
//!   request to the planner (`NeedsModel`).

use kestrel_core::Intent;
use kestrel_core::features::CONJUNCTIONS;
use serde_json::{Value, json};

use crate::plan::MAX_NODES;
use crate::templates::{Context, Proposal, propose, take_due, words};

/// The request's parts, in order.
pub fn split(text: &str) -> Vec<String> {
    let mut parts: Vec<Vec<String>> = vec![Vec::new()];
    for w in words(text) {
        if CONJUNCTIONS.contains(&w.as_str()) {
            parts.push(Vec::new());
        } else if let Some(last) = parts.last_mut() {
            last.push(w);
        }
    }
    parts.retain(|p| !p.is_empty());
    let n = parts.len();
    if n > 1
        && let Some(i) = parts[n - 1].iter().position(|w| w == "dono" || w == "both")
    {
        let shared: Vec<String> = parts[n - 1][i + 1..].to_vec();
        parts[n - 1].remove(i);
        for p in &mut parts[..n - 1] {
            p.extend(shared.iter().cloned());
        }
    }
    parts.into_iter().map(|p| p.join(" ")).collect()
}

/// Words that ask for tasks without saying what they are; anything else is a title.
const LINK_STOP: &[&str] = &[
    "create", "make", "add", "turn", "a", "an", "the", "task", "tasks", "list", "todo", "todos",
    "bana", "banao", "do", "karo", "se", "from", "them", "unse", "inse", "isse", "usse", "those",
    "of", "out", "all", "for", "me", "please", "action", "items", "into",
];
const REFERS_BACK: &[&str] = &["them", "those", "unse", "inse", "isse", "usse"];

/// "create a task list", "make tasks from them", "unse tasks bana do": tasks from what the parts
/// before produced. Returns the due date it names, if any.
fn tasks_from_before(text: &str, ctx: &Context) -> Option<Option<String>> {
    let mut w = words(text);
    let due = take_due(&mut w, ctx.today);
    let tasky = w
        .iter()
        .any(|x| matches!(x.as_str(), "task" | "tasks" | "todo" | "todos"));
    let rest: Vec<&String> = w
        .iter()
        .filter(|x| !LINK_STOP.contains(&x.as_str()))
        .collect();
    let refers = w.iter().any(|x| REFERS_BACK.contains(&x.as_str()));
    (tasky && (rest.is_empty() || refers)).then(|| due.map(|d| d.to_string()))
}

/// One plan for the routed parts, or why there is none.
pub fn plan_parts(parts: &[(Intent, String)], ctx: &Context) -> Proposal {
    let mut nodes: Vec<Value> = Vec::new();
    let mut notes_from: Option<String> = None; // the latest notes-producing node
    for (k, (intent, text)) in parts.iter().enumerate() {
        let tag = format!("p{}", k + 1);
        if let Some(due) = tasks_from_before(text, ctx) {
            let Some(src) = notes_from.clone() else {
                return Proposal::NeedsModel; // nothing to take the tasks from
            };
            let x = format!("{tag}x");
            let mut args = json!({});
            if let Some(d) = due {
                args["due"] = json!(d);
            }
            nodes.push(json!({"id": x, "op": "extract_action_items", "deps": [src]}));
            nodes.push(
                json!({"id": format!("{tag}t"), "op": "create_task", "args": args, "deps": [x]}),
            );
            continue;
        }
        match propose(*intent, text, ctx) {
            Proposal::Plan(p) => {
                for mut n in p["nodes"].as_array().cloned().unwrap_or_default() {
                    let id = format!("{tag}{}", n["id"].as_str().unwrap_or_default());
                    n["id"] = json!(id);
                    if let Some(deps) = n.get_mut("deps").and_then(Value::as_array_mut) {
                        for d in deps.iter_mut() {
                            *d = json!(format!("{tag}{}", d.as_str().unwrap_or_default()));
                        }
                    }
                    if matches!(
                        n["op"].as_str(),
                        Some("fetch_recent_notes" | "search_notes" | "read_document")
                    ) {
                        notes_from = Some(id);
                    }
                    nodes.push(n);
                }
            }
            Proposal::Clarify(slot) => return Proposal::Clarify(slot),
            Proposal::NeedsModel | Proposal::NoAction => return Proposal::NeedsModel,
        }
    }
    if nodes.is_empty() || nodes.len() > MAX_NODES {
        return Proposal::NeedsModel;
    }
    Proposal::Plan(json!({"nodes": nodes}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Date;

    #[test]
    fn requests_split_at_conjunctions() {
        assert_eq!(
            split("summarize my last three meeting notes and create a task list"),
            [
                "summarize my last three meeting notes",
                "create a task list"
            ]
        );
        assert_eq!(
            split("keep answers short and then summarize my latest note"),
            ["keep answers short", "summarize my latest note"]
        );
        assert_eq!(
            split("jawab chhote rakho aur mera latest note summarize karo"),
            ["jawab chhote rakho", "mera latest note summarize karo"]
        );
        assert_eq!(split("open the project plan"), ["open the project plan"]);
        assert_eq!(split("and"), Vec::<String>::new());
    }

    #[test]
    fn a_shared_hinglish_predicate_goes_to_every_part() {
        assert_eq!(
            split("task 3 aur task 2 dono delete karo"),
            ["task 3 delete karo", "task 2 delete karo"]
        );
        assert_eq!(
            split("delete task 3 and task 2 both"),
            ["delete task 3", "task 2"]
        ); // nothing after "both"
    }

    fn ctx_docs() -> Vec<String> {
        vec![
            "docs/project-plan.md".into(),
            "2026-10-18-groceries.md".into(),
        ]
    }

    fn go(parts: &[(Intent, &str)]) -> Proposal {
        let docs = ctx_docs();
        let ctx = Context {
            today: Date::parse("2026-10-22").unwrap(),
            documents: &docs,
        };
        let parts: Vec<(Intent, String)> = parts.iter().map(|(i, t)| (*i, t.to_string())).collect();
        plan_parts(&parts, &ctx)
    }

    fn ops(p: &Proposal) -> Vec<(String, Value, Value)> {
        let Proposal::Plan(v) = p else {
            panic!("{p:?}")
        };
        crate::plan::parse(&v.to_string()).unwrap_or_else(|e| panic!("joined plan invalid: {e}"));
        v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                (
                    n["op"].as_str().unwrap().to_string(),
                    n["args"].clone(),
                    n["deps"].clone(),
                )
            })
            .collect()
    }

    #[test]
    fn a_task_list_part_extracts_from_the_notes_before_it() {
        let p = go(&[
            (
                Intent::SummarizeNotes,
                "summarize my last three meeting notes",
            ),
            (Intent::CreateTask, "create a task list"),
        ]);
        let o = ops(&p);
        assert_eq!(
            o.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
            [
                "fetch_recent_notes",
                "summarize",
                "extract_action_items",
                "create_task"
            ]
        );
        assert_eq!(o[0].1, json!({"n": 3, "tag": "meeting"}));
        let Proposal::Plan(v) = &p else {
            unreachable!()
        };
        let fetch_id = v["nodes"][0]["id"].clone();
        assert_eq!(o[2].2, json!([fetch_id]));
        assert_eq!(o[3].2, json!([v["nodes"][2]["id"]]));
        let p = go(&[
            (
                Intent::SummarizeNotes,
                "meri last teen meeting notes summarize karo",
            ),
            (Intent::CreateTask, "unse task list bana do"),
        ]);
        assert_eq!(ops(&p).len(), 4);
    }

    #[test]
    fn independent_parts_are_planned_side_by_side() {
        let o = ops(&go(&[
            (Intent::SetPreference, "keep answers short"),
            (Intent::SummarizeNotes, "summarize my latest note"),
        ]));
        assert_eq!(
            o[0],
            (
                "update_preference".into(),
                json!({"key": "answer_length", "value": "short"}),
                Value::Null
            )
        );
        assert_eq!(o[1].1, json!({"n": 1}));
        let o = ops(&go(&[
            (Intent::RetrieveMemory, "what do you remember about ravi"),
            (Intent::ReadDocument, "open the project plan"),
        ]));
        assert_eq!(
            o.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
            ["retrieve_memory", "read_document"]
        );
        let o = ops(&go(&[
            (Intent::DeleteTask, "task 3 delete karo"),
            (Intent::DeleteTask, "task 2 delete karo"),
        ]));
        assert_eq!(
            (o[0].1.clone(), o[1].1.clone()),
            (json!({"id": 3}), json!({"id": 2}))
        );
    }

    #[test]
    fn a_missing_slot_asks_and_anything_else_goes_to_the_planner() {
        assert_eq!(
            go(&[
                (Intent::SearchNotes, "find my budget notes"),
                (Intent::CreateTask, "remind me")
            ]),
            Proposal::Clarify("title")
        );
        assert_eq!(
            go(&[
                (Intent::Chitchat, "hi"),
                (Intent::CreateTask, "remind me to call asha")
            ]),
            Proposal::NeedsModel
        );
        assert_eq!(
            go(&[
                (Intent::CreateTask, "create a task list"),
                (Intent::SearchNotes, "find my budget notes")
            ]),
            Proposal::NeedsModel
        ); // nothing to extract from yet
    }
}
