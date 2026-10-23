//! The tool calls Kestrel can make (Ch. 15, A.13). The wire format is a Rust enum, so the enum IS
//! the allowlist: there is no shell or exec variant, and anything else fails to parse. Every
//! argument is validated (`args`, `Sandbox`) before a call exists as a `ValidatedCall`, and
//! `execute` is the single choke point where confirmation is enforced.
//!
//! Changes from A.13: unknown argument fields are rejected; `retrieve_memory` and
//! `update_preference` are tools (Ch. 15.1 lists six); `update_preference` needs confirmation
//! unless the user stated the preference this turn (Ch. 15.1); the backend returns typed output.
//! The tool log is content-free: tool name, argument LENGTHS, status and time. Not hashes: a
//! short query or title can be recovered from its hash by trying a dictionary.

use std::path::PathBuf;

use kestrel_core::KestrelError;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::args::{clamp, clean_text, iso_date};
use crate::sandbox::Sandbox;

pub const MAX_QUERY: usize = 200;
pub const MAX_TITLE: usize = 200;
pub const MAX_PREF_VALUE: usize = 100;
pub const MAX_PREF_KEY: usize = 32;

/// What the generative model (or a template plan) emits.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(
    tag = "tool",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ToolCall {
    SearchNotes { query: String, limit: Option<u32> },
    ReadDocument { path: String },
    CreateTask { title: String, due: Option<String> },
    DeleteTask { id: u64 },
    RetrieveMemory { terms: String },
    UpdatePreference { key: String, value: String },
}

/// A call whose every argument passed validation.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidatedCall {
    SearchNotes { query: String, limit: u32 },
    ReadDocument { path: PathBuf },
    CreateTask { title: String, due: Option<String> },
    DeleteTask { id: u64 },
    RetrieveMemory { terms: String },
    UpdatePreference { key: String, value: String },
}

impl ValidatedCall {
    pub fn name(&self) -> &'static str {
        match self {
            Self::SearchNotes { .. } => "search_notes",
            Self::ReadDocument { .. } => "read_document",
            Self::CreateTask { .. } => "create_task",
            Self::DeleteTask { .. } => "delete_task",
            Self::RetrieveMemory { .. } => "retrieve_memory",
            Self::UpdatePreference { .. } => "update_preference",
        }
    }
}

pub fn parse_and_validate(
    model_output: &str,
    sandbox: &Sandbox,
) -> Result<ValidatedCall, KestrelError> {
    let call: ToolCall = serde_json::from_str(model_output)
        .map_err(|e| KestrelError::Invalid(format!("tool call JSON: {e}")))?;
    validate(call, sandbox)
}

pub fn validate(call: ToolCall, sandbox: &Sandbox) -> Result<ValidatedCall, KestrelError> {
    Ok(match call {
        ToolCall::SearchNotes { query, limit } => ValidatedCall::SearchNotes {
            query: clean_text(&query, MAX_QUERY, "query")?,
            limit: clamp(limit, 10, 1, 50),
        },
        ToolCall::ReadDocument { path } => ValidatedCall::ReadDocument {
            path: sandbox.resolve(&path)?,
        },
        ToolCall::CreateTask { title, due } => ValidatedCall::CreateTask {
            title: clean_text(&title, MAX_TITLE, "title")?,
            due: due.as_deref().map(iso_date).transpose()?,
        },
        ToolCall::DeleteTask { id } => ValidatedCall::DeleteTask { id },
        ToolCall::RetrieveMemory { terms } => ValidatedCall::RetrieveMemory {
            terms: clean_text(&terms, MAX_QUERY, "terms")?,
        },
        ToolCall::UpdatePreference { key, value } => ValidatedCall::UpdatePreference {
            key: pref_key(&key)?,
            value: clean_text(&value, MAX_PREF_VALUE, "value")?,
        },
    })
}

/// A preference key: 1..=32 of `a-z`, `0-9`, `_`, starting with a letter.
pub(crate) fn pref_key(k: &str) -> Result<String, KestrelError> {
    let ok = (1..=MAX_PREF_KEY).contains(&k.len())
        && k.starts_with(|c: char| c.is_ascii_lowercase())
        && k.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(k.to_string())
    } else {
        Err(KestrelError::Invalid(format!("preference key {k:?}")))
    }
}

/// What this turn allows without asking.
#[derive(Debug, Clone, Copy, Default)]
pub struct Policy {
    /// The user asked for this preference change in this turn (a confident `set_preference`).
    pub user_stated_preference: bool,
}

pub fn requires_confirmation(c: &ValidatedCall, policy: Policy) -> bool {
    match c {
        ValidatedCall::DeleteTask { .. } => true,
        ValidatedCall::UpdatePreference { .. } => !policy.user_stated_preference,
        _ => false,
    }
}

pub trait Confirm {
    /// Shown to the user; true = go ahead.
    fn ask(&self, summary: &str) -> bool;
}

/// A note as tools return it: a name (relative to its root) and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutput {
    Notes(Vec<Note>),
    Text(String),
    TaskId(u64),
    Done,
}

pub trait Backend {
    fn run(&mut self, c: &ValidatedCall) -> Result<ToolOutput, KestrelError>;

    /// A call the user said yes to at the prompt. The memory backend stores such a preference
    /// as the user's (ADR-026); by default it is a plain run.
    fn run_confirmed(&mut self, c: &ValidatedCall) -> Result<ToolOutput, KestrelError> {
        self.run(c)
    }
}

/// The single choke point: confirmation first, the backend only if allowed.
pub fn execute(
    c: &ValidatedCall,
    policy: Policy,
    confirm: &dyn Confirm,
    backend: &mut dyn Backend,
) -> Result<ToolOutput, KestrelError> {
    if !requires_confirmation(c, policy) {
        return backend.run(c);
    }
    if !confirm.ask(&summary(c)) {
        return Err(KestrelError::Denied(format!(
            "{} declined by the user",
            c.name()
        )));
    }
    backend.run_confirmed(c)
}

/// What the user is asked to confirm (shown, never logged).
fn summary(c: &ValidatedCall) -> String {
    match c {
        ValidatedCall::DeleteTask { id } => format!("delete task {id}?"),
        ValidatedCall::UpdatePreference { key, value } => format!("set {key} = {value:?}?"),
        other => format!("run {}?", other.name()),
    }
}

/// One content-free log line for a call: name, argument lengths in characters, status, ms.
pub fn log_record(c: &ValidatedCall, status: &str, ms: f64) -> Value {
    let n = |s: &str| s.chars().count();
    let chars = match c {
        ValidatedCall::SearchNotes { query, .. } => json!({"query": n(query)}),
        ValidatedCall::ReadDocument { path } => json!({"path": n(&path.to_string_lossy())}),
        ValidatedCall::CreateTask { title, due } => {
            json!({"title": n(title), "due": due.is_some()})
        }
        ValidatedCall::DeleteTask { .. } => json!({}),
        ValidatedCall::RetrieveMemory { terms } => json!({"terms": n(terms)}),
        ValidatedCall::UpdatePreference { key, value } => json!({"key": n(key), "value": n(value)}),
    };
    json!({"tool": c.name(), "arg_chars": chars, "status": status, "ms": ms})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox(name: &str) -> (Sandbox, PathBuf) {
        let d = std::env::temp_dir().join(format!("kestrel_tools_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        (Sandbox::new(std::slice::from_ref(&d)).unwrap(), d)
    }

    #[test]
    fn rejects_shell_unknown_tools_extra_and_missing_args() {
        let (sb, _) = sandbox("a");
        for bad in [
            r#"{"tool":"shell","args":{"cmd":"rm -rf /"}}"#,
            r#"{"tool":"exec","args":{}}"#,
            "rm -rf /",
            r#"{"tool":"create_task","args":{}}"#,
            r#"{"tool":"create_task","args":{"title":"x","shell":"rm -rf /"}}"#,
            r#"{"tool":"delete_task","args":{"id":"7"}}"#,
            r#"{"tool":"delete_task","args":{"id":-1}}"#,
            r#"{"tool":"search_notes","args":{"query":"q"},"extra":1}"#,
        ] {
            assert!(parse_and_validate(bad, &sb).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn validates_every_argument() {
        let (sb, _) = sandbox("b");
        let ok = parse_and_validate(
            r#"{"tool":"search_notes","args":{"query":"  budget  ","limit":9999}}"#,
            &sb,
        );
        assert_eq!(
            ok.unwrap(),
            ValidatedCall::SearchNotes {
                query: "budget".into(),
                limit: 50
            }
        );
        let ok = parse_and_validate(r#"{"tool":"search_notes","args":{"query":"budget"}}"#, &sb);
        assert_eq!(
            ok.unwrap(),
            ValidatedCall::SearchNotes {
                query: "budget".into(),
                limit: 10
            }
        );
        let ok = parse_and_validate(
            r#"{"tool":"create_task","args":{"title":"call asha","due":"2026-10-23"}}"#,
            &sb,
        );
        assert_eq!(
            ok.unwrap(),
            ValidatedCall::CreateTask {
                title: "call asha".into(),
                due: Some("2026-10-23".into())
            }
        );
        for bad in [
            r#"{"tool":"create_task","args":{"title":"x","due":"tomorrow"}}"#,
            r#"{"tool":"create_task","args":{"title":"x","due":"2026-02-30"}}"#,
            r#"{"tool":"create_task","args":{"title":"a\u0007b"}}"#,
            r#"{"tool":"update_preference","args":{"key":"Answer Length","value":"short"}}"#,
            r#"{"tool":"update_preference","args":{"key":"answer_length","value":""}}"#,
            r#"{"tool":"retrieve_memory","args":{"terms":"   "}}"#,
        ] {
            assert!(parse_and_validate(bad, &sb).is_err(), "should reject {bad}");
        }
        let pref = parse_and_validate(
            r#"{"tool":"update_preference","args":{"key":"answer_length","value":"short"}}"#,
            &sb,
        );
        assert_eq!(
            pref.unwrap(),
            ValidatedCall::UpdatePreference {
                key: "answer_length".into(),
                value: "short".into()
            }
        );
    }

    #[test]
    fn read_document_goes_through_the_sandbox() {
        let (sb, d) = sandbox("c");
        fs::write(d.join("plan.md"), "p").unwrap();
        let ok = parse_and_validate(r#"{"tool":"read_document","args":{"path":"plan.md"}}"#, &sb)
            .unwrap();
        assert_eq!(
            ok,
            ValidatedCall::ReadDocument {
                path: d.canonicalize().unwrap().join("plan.md")
            }
        );
        let escape = parse_and_validate(
            r#"{"tool":"read_document","args":{"path":"../../etc/passwd"}}"#,
            &sb,
        );
        assert!(matches!(escape, Err(KestrelError::Sandbox(_))));
    }

    struct Always(bool);
    impl Confirm for Always {
        fn ask(&self, _: &str) -> bool {
            self.0
        }
    }
    struct Spy(u32);
    impl Backend for Spy {
        fn run(&mut self, _: &ValidatedCall) -> Result<ToolOutput, KestrelError> {
            self.0 += 1;
            Ok(ToolOutput::Done)
        }
    }

    #[test]
    fn destructive_and_durable_calls_need_confirmation() {
        let mut spy = Spy(0);
        let del = ValidatedCall::DeleteTask { id: 7 };
        let none = Policy::default();
        assert!(matches!(
            execute(&del, none, &Always(false), &mut spy),
            Err(KestrelError::Denied(_))
        ));
        assert_eq!(spy.0, 0, "the backend must not run when declined");
        assert!(execute(&del, none, &Always(true), &mut spy).is_ok());
        assert_eq!(spy.0, 1);
        let pref = ValidatedCall::UpdatePreference {
            key: "answer_length".into(),
            value: "short".into(),
        };
        assert!(requires_confirmation(&pref, none));
        let stated = Policy {
            user_stated_preference: true,
        };
        assert!(!requires_confirmation(&pref, stated));
        assert!(execute(&pref, stated, &Always(false), &mut spy).is_ok());
        let create = ValidatedCall::CreateTask {
            title: "t".into(),
            due: None,
        };
        assert!(!requires_confirmation(&create, none));
        assert!(
            requires_confirmation(&del, stated),
            "stating a preference does not cover a delete"
        );
    }

    #[test]
    fn a_yes_reaches_the_backend_as_confirmed() {
        // The memory backend stores a confirmed preference as the user's (ADR-026), so it must
        // know which calls the user said yes to.
        #[derive(Default)]
        struct Seen(Vec<(&'static str, bool)>);
        impl Backend for Seen {
            fn run(&mut self, c: &ValidatedCall) -> Result<ToolOutput, KestrelError> {
                self.0.push((c.name(), false));
                Ok(ToolOutput::Done)
            }
            fn run_confirmed(&mut self, c: &ValidatedCall) -> Result<ToolOutput, KestrelError> {
                self.0.push((c.name(), true));
                Ok(ToolOutput::Done)
            }
        }
        let pref = ValidatedCall::UpdatePreference {
            key: "answer_length".into(),
            value: "short".into(),
        };
        let stated = Policy {
            user_stated_preference: true,
        };
        let mut seen = Seen::default();
        execute(&pref, Policy::default(), &Always(true), &mut seen).unwrap();
        execute(&pref, stated, &Always(true), &mut seen).unwrap();
        let search = ValidatedCall::SearchNotes {
            query: "q".into(),
            limit: 1,
        };
        execute(&search, Policy::default(), &Always(true), &mut seen).unwrap();
        assert_eq!(
            seen.0,
            [
                ("update_preference", true),
                ("update_preference", false),
                ("search_notes", false)
            ]
        );
        // The default hook is the plain run.
        let mut spy = Spy(0);
        execute(&pref, Policy::default(), &Always(true), &mut spy).unwrap();
        assert_eq!(spy.0, 1);
    }

    #[test]
    fn the_log_record_has_lengths_not_content() {
        let c = ValidatedCall::SearchNotes {
            query: "CANARY-x budget".into(),
            limit: 5,
        };
        let r = log_record(&c, "ok", 1.5);
        let s = r.to_string();
        assert!(!s.contains("CANARY") && !s.contains("budget"), "{s}");
        assert_eq!(r["tool"], "search_notes");
        assert_eq!(r["arg_chars"], json!({"query": 15}));
        assert_eq!(
            (r["status"].as_str(), r["ms"].as_f64()),
            (Some("ok"), Some(1.5))
        );
        let names: Vec<_> = [
            ValidatedCall::ReadDocument { path: "/x".into() },
            ValidatedCall::CreateTask {
                title: "t".into(),
                due: None,
            },
            ValidatedCall::DeleteTask { id: 1 },
            ValidatedCall::RetrieveMemory { terms: "t".into() },
            ValidatedCall::UpdatePreference {
                key: "k".into(),
                value: "v".into(),
            },
        ]
        .iter()
        .map(ValidatedCall::name)
        .collect();
        assert_eq!(
            names,
            [
                "read_document",
                "create_task",
                "delete_task",
                "retrieve_memory",
                "update_preference"
            ]
        );
    }
}
