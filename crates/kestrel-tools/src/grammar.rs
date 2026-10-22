//! The GBNF grammar a generative model must follow when it proposes a plan (ADR-023): one
//! alternative per allowlisted op with that op's argument shape, so the model can only write
//! well-formed plans with known ops; `plan::parse` still validates everything else (ids, deps,
//! cycles, input kinds, dates, lengths). Keys come in a fixed order (id, op, args, deps; each
//! op's arguments in schema order) and `canonical` writes any plan in that order, so a gold plan
//! can be checked against the grammar token by token.
//!
//! Strings are 1..=200 characters without control characters; the only escapes are `\"`, `\\`
//! and `\/`. Up to 12 nodes (`plan::MAX_NODES`).

use kestrel_core::KestrelError;
use serde_json::Value;

pub const ROOT: &str = "root";

/// Argument keys per op, in the grammar's order (an op without arguments has none).
pub const ARGS: [(&str, &[&str]); 9] = [
    ("fetch_recent_notes", &["n", "tag"]),
    ("search_notes", &["query", "limit"]),
    ("read_document", &["path"]),
    ("summarize", &[]),
    ("extract_action_items", &[]),
    ("create_task", &["title", "due"]),
    ("delete_task", &["id"]),
    ("retrieve_memory", &["terms"]),
    ("update_preference", &["key", "value"]),
];

/// Which arguments an op must have (the rest are optional), and each argument's value rule.
fn required(op: &str, key: &str) -> bool {
    !matches!(
        (op, key),
        ("fetch_recent_notes", "tag") | ("search_notes", "limit") | ("create_task", _)
    )
}

fn value_rule(key: &str) -> &'static str {
    match key {
        "n" | "limit" | "id" => "num",
        "due" => "date",
        "key" => "key",
        "path" => "path",
        _ => "str",
    }
}

/// The GBNF literal for the JSON string `"s"`.
fn lit(s: &str) -> String {
    format!(r#""\"{s}\"""#)
}

/// The open grammar: any path (the sandbox decides at run time).
pub fn gbnf() -> String {
    gbnf_with(&[])
}

/// With `documents` (the names under the sandbox roots), `read_document` can only name one of
/// them, so a model cannot make up a path; names that are not plain path characters are left out.
pub fn gbnf_with(documents: &[String]) -> String {
    let plain = |d: &&String| {
        !d.is_empty()
            && d.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ' '))
    };
    let names: Vec<String> = documents.iter().filter(plain).map(|d| lit(d)).collect();
    let path = if names.is_empty() {
        "path ::= str".to_string()
    } else {
        format!("path ::= {}", names.join(" | "))
    };
    let mut rules = vec![
        format!(
            r#"root ::= "{{" ws {} ws ":" ws "[" ws node (ws "," ws node){{0,{}}} ws "]" ws "}}""#,
            lit("nodes"),
            crate::plan::MAX_NODES - 1
        ),
        format!(
            r#"node ::= "{{" ws {} ws ":" ws id ws "," ws op deps? ws "}}""#,
            lit("id")
        ),
        format!(
            "op ::= {}",
            ARGS.iter()
                .map(|(op, _)| op.replace('_', "-"))
                .collect::<Vec<_>>()
                .join(" | ")
        ),
    ];
    for (op, keys) in ARGS {
        let mut rule = format!(
            r#"{} ::= {} ws ":" ws {}"#,
            op.replace('_', "-"),
            lit("op"),
            lit(op)
        );
        if !keys.is_empty() {
            // Every in-order subset of the keys that has all the required ones.
            let alts: Vec<String> = (0u32..1 << keys.len())
                .map(|mask| {
                    keys.iter()
                        .enumerate()
                        .filter(|(i, _)| mask & (1 << i) != 0)
                        .map(|(_, k)| *k)
                        .collect::<Vec<_>>()
                })
                .filter(|sub| {
                    keys.iter().all(|k| !required(op, k) || sub.contains(k)) && !sub.is_empty()
                })
                .map(|sub| {
                    sub.iter()
                        .map(|k| format!(r#"{} ws ":" ws {}"#, lit(k), value_rule(k)))
                        .collect::<Vec<_>>()
                        .join(r#" ws "," ws "#)
                })
                .collect();
            let empty_ok = keys.iter().all(|k| !required(op, k));
            let body = format!("({}){}", alts.join(" | "), if empty_ok { "?" } else { "" });
            rule += &format!(
                r#" ws "," ws {} ws ":" ws "{{" ws {body} ws "}}""#,
                lit("args")
            );
        }
        rules.push(rule);
    }
    rules.extend([
        format!(
            r#"deps ::= ws "," ws {} ws ":" ws "[" ws (id (ws "," ws id){{0,{}}})? ws "]""#,
            lit("deps"),
            crate::plan::MAX_NODES - 1
        ),
        format!(
            r#"id ::= "\"" [a-z0-9_-]{{1,{}}} "\"""#,
            crate::plan::MAX_ID
        ),
        r#"num ::= [0-9]{1,9}"#.to_string(),
        r#"key ::= "\"" [a-z] [a-z0-9_]{0,31} "\"""#.to_string(),
        r#"date ::= "\"" [0-9]{4} "-" [0-9]{2} "-" [0-9]{2} "\"""#.to_string(),
        r#"str ::= "\"" char{1,200} "\"""#.to_string(),
        path,
        r#"char ::= [^"\\\x7F\x00-\x1F] | [\\] ["\\/]"#.to_string(),
        r#"ws ::= " "?"#.to_string(),
    ]);
    rules.join("\n") + "\n"
}

fn bad(m: String) -> KestrelError {
    KestrelError::Invalid(format!("not expressible in the plan grammar: {m}"))
}

/// The plan's JSON in the grammar's key order and spacing (`", "`, `": "`).
pub fn canonical(plan: &Value) -> Result<String, KestrelError> {
    let obj = plan
        .as_object()
        .filter(|o| o.len() == 1)
        .ok_or_else(|| bad("a plan is {\"nodes\": [...]}".into()))?;
    let nodes = obj
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("no nodes list".into()))?;
    let j = |v: &Value| serde_json::to_string(v).unwrap_or_default();
    let mut out = Vec::new();
    for n in nodes {
        let n = n
            .as_object()
            .ok_or_else(|| bad("a node is an object".into()))?;
        if let Some(k) = n
            .keys()
            .find(|k| !matches!(k.as_str(), "id" | "op" | "args" | "deps"))
        {
            return Err(bad(format!("node field {k}")));
        }
        let op = n
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("node without op".into()))?;
        let (_, keys) = ARGS
            .iter()
            .find(|(o, _)| *o == op)
            .ok_or_else(|| bad(format!("op {op}")))?;
        let empty = serde_json::Map::new();
        let args = match n.get("args") {
            None => &empty,
            Some(a) => a
                .as_object()
                .ok_or_else(|| bad("args is an object".into()))?,
        };
        if let Some(k) = args.keys().find(|k| !keys.contains(&k.as_str())) {
            return Err(bad(format!("{op} argument {k}")));
        }
        let mut s = format!(
            r#"{{"id": {}, "op": {}"#,
            j(n.get("id").unwrap_or(&Value::Null)),
            j(&Value::from(op))
        );
        if !keys.is_empty() {
            let fields: Vec<String> = keys
                .iter()
                .filter_map(|k| args.get(*k).map(|v| format!(r#""{k}": {}"#, j(v))))
                .collect();
            s += &format!(r#", "args": {{{}}}"#, fields.join(", "));
        }
        if let Some(deps) = n
            .get("deps")
            .and_then(Value::as_array)
            .filter(|d| !d.is_empty())
        {
            s += &format!(
                r#", "deps": [{}]"#,
                deps.iter().map(j).collect::<Vec<_>>().join(", ")
            );
        }
        out.push(s + "}");
    }
    Ok(format!(r#"{{"nodes": [{}]}}"#, out.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_grammar_names_every_op_and_nothing_else() {
        let g = gbnf();
        for (op, _) in ARGS {
            assert!(g.contains(&format!("\\\"{op}\\\"")), "{op}");
        }
        assert!(!g.contains("shell") && !g.contains("exec"));
        assert!(g.starts_with("root ::= "), "{}", &g[..40]);
        assert_eq!(g, gbnf(), "deterministic");
        for line in g.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (name, _) = line
                .split_once(" ::= ")
                .unwrap_or_else(|| panic!("not a rule: {line}"));
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "rule name {name}"
            );
        }
    }

    #[test]
    fn with_known_documents_a_path_can_only_name_one_of_them() {
        assert!(
            gbnf().contains("\npath ::= str\n"),
            "the open grammar takes any path"
        );
        let docs = [
            "docs/project-plan.md".to_string(),
            "2026-10-18-groceries.md".to_string(),
        ];
        let g = gbnf_with(&docs);
        assert!(
            g.contains(r#"path ::= "\"docs/project-plan.md\"" | "\"2026-10-18-groceries.md\"""#),
            "{g}"
        );
        // A name the grammar could not spell safely is left out, never written raw.
        let odd = gbnf_with(&["a\"b.md".to_string(), "ok.md".to_string()]);
        assert!(
            odd.contains(r#"path ::= "\"ok.md\"""#) && !odd.contains("a\\\"b"),
            "{odd}"
        );
    }

    #[test]
    fn canonical_orders_keys_like_the_grammar_and_round_trips() {
        // Sorted keys, as the dataset build writes them.
        let sorted = json!({"nodes": [
            {"args": {"tag": "meeting", "n": 3}, "id": "f", "op": "fetch_recent_notes"},
            {"deps": ["f"], "id": "x", "op": "extract_action_items"},
            {"args": {"due": "2026-10-23"}, "deps": ["x"], "id": "t", "op": "create_task"}]});
        let c = canonical(&sorted).unwrap();
        assert_eq!(
            c,
            r#"{"nodes": [{"id": "f", "op": "fetch_recent_notes", "args": {"n": 3, "tag": "meeting"}}, {"id": "x", "op": "extract_action_items", "deps": ["f"]}, {"id": "t", "op": "create_task", "args": {"due": "2026-10-23"}, "deps": ["x"]}]}"#
        );
        let a = crate::plan::parse(&c).unwrap();
        let b = crate::plan::parse(&sorted.to_string()).unwrap();
        assert_eq!(a, b);
        let quoted = json!({"nodes": [{"id": "s", "op": "search_notes", "args": {"query": "say \"hi\" / bye"}}]});
        assert_eq!(
            canonical(&quoted).unwrap(),
            r#"{"nodes": [{"id": "s", "op": "search_notes", "args": {"query": "say \"hi\" / bye"}}]}"#
        );
    }

    #[test]
    fn canonical_refuses_what_the_grammar_cannot_express() {
        for bad in [
            json!({"nodes": [{"id": "a", "op": "run_shell"}]}),
            json!({"nodes": [{"id": "a", "op": "search_notes", "args": {"query": "q", "rm": 1}}]}),
            json!({"nodes": [{"id": "a", "op": "summarize", "max_retries": 2}]}),
            json!({"steps": []}),
        ] {
            assert!(canonical(&bad).is_err(), "{bad}");
        }
    }
}
