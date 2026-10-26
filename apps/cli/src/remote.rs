//! Tier 4 in its own process (ADR-034): the always-on service keeps rules + encoder loaded and
//! starts `kestrel tier4-worker CONFIG` (Gemma) only when a request reaches Tier 4. After `idle`
//! without a request the worker is stopped, so the OS frees all of Gemma's memory. A worker that
//! dies is a failed tier (the cascade carries on as for any Tier-4 failure); the next request
//! starts a new one.
//!
//! Protocol, one JSON object per line: the worker says `{"ready": true}` once loaded; then for each
//! `{"id": N, "text": "..."}` it answers `{"probs": [["create_task", 0.97], ...]}`, and for each
//! `{"id": N, "summarize": ["note text", ...]}` (feature 5, src/summary.rs) `{"summary": "..."}`;
//! either may be `{"error": "..."}`.

use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};
use serde_json::{Value, json};

pub struct RemoteTier {
    program: PathBuf,
    args: Vec<String>,
    cost: u32,
    idle: Duration,
    worker: RefCell<Option<Worker>>,
    last: Cell<Option<Instant>>,
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl RemoteTier {
    pub fn new(program: PathBuf, args: Vec<String>, cost: u32, idle: Duration) -> Self {
        Self {
            program,
            args,
            cost,
            idle,
            worker: RefCell::new(None),
            last: Cell::new(None),
        }
    }

    /// Spawns the worker and waits for its ready line.
    fn start(&self) -> Result<Worker, KestrelError> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| unavailable())?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(unavailable());
        };
        let mut w = Worker {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        match read_json(&mut w.stdout) {
            Some(v) if v["ready"] == true => Ok(w),
            _ => Err(unavailable()),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.worker.borrow().is_some()
    }

    /// Stops the worker if it has had no request for longer than `idle` at `now`; true if it did.
    pub fn reap_if_idle(&self, now: Instant) -> bool {
        let idle = self
            .last
            .get()
            .is_some_and(|t| now.saturating_duration_since(t) > self.idle);
        if idle && self.is_loaded() {
            *self.worker.borrow_mut() = None; // Drop kills and waits for the process
            return true;
        }
        false
    }
}

impl RemoteTier {
    /// Sends `msg` to the worker (started if it is not running) and returns its answer; the
    /// worker's own error is `Invalid`.
    fn exchange(&self, msg: &Value) -> Result<Value, KestrelError> {
        if !self.is_loaded() {
            let w = self.start()?;
            *self.worker.borrow_mut() = Some(w);
        }
        self.last.set(Some(Instant::now()));
        let answer = {
            let mut guard = self.worker.borrow_mut();
            let w = guard.as_mut().ok_or_else(unavailable)?;
            writeln!(w.stdin, "{msg}")
                .and_then(|()| w.stdin.flush())
                .ok()
                .and_then(|()| read_json(&mut w.stdout))
        };
        let Some(v) = answer else {
            *self.worker.borrow_mut() = None; // dead or speaking garbage: the next request starts anew
            return Err(unavailable());
        };
        match v["error"].as_str() {
            Some(e) => Err(KestrelError::Invalid(e.to_string())),
            None => Ok(v),
        }
    }
}

impl crate::summary::Summarizer for RemoteTier {
    fn summarize(&self, texts: &[&str]) -> Result<String, KestrelError> {
        let v = self.exchange(&json!({"id": 0, "summarize": texts}))?;
        v["summary"]
            .as_str()
            .map(String::from)
            .ok_or_else(unavailable)
    }
}

impl crate::kinds::Writer for RemoteTier {
    fn write(&self, kind: crate::kinds::Kind, request: &str) -> Result<String, KestrelError> {
        let v =
            self.exchange(&json!({"id": 0, "write": {"kind": kind.label(), "text": request}}))?;
        v["text"].as_str().map(String::from).ok_or_else(unavailable)
    }
}

fn unavailable() -> KestrelError {
    KestrelError::ModelUnavailable(Tier::Generative)
}

/// One JSON line, or None at the end of the stream or on garbage.
fn read_json(r: &mut impl BufRead) -> Option<Value> {
    let mut line = String::new();
    match r.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => serde_json::from_str(&line).ok(),
    }
}

impl TierModel for RemoteTier {
    fn tier(&self) -> Tier {
        Tier::Generative
    }

    fn est_cost_ms(&self) -> u32 {
        self.cost
    }

    fn classify(&self, req: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
        let v = self.exchange(&json!({"id": req.id, "text": req.text}))?;
        let probs = v["probs"].as_array().ok_or_else(unavailable)?;
        Ok(probs
            .iter()
            .filter_map(|pair| {
                let label = Intent::from_label(pair.get(0)?.as_str()?)?;
                Some((label, pair.get(1)?.as_f64()? as f32))
            })
            .collect())
    }
}

/// The service's Tier 4: the cascade owns one handle, the service keeps another to stop the
/// worker when it is idle, and the summaries and the v3 skills use others.
pub struct SharedRemote(pub std::rc::Rc<RemoteTier>);

impl TierModel for SharedRemote {
    fn tier(&self) -> Tier {
        self.0.tier()
    }

    fn est_cost_ms(&self) -> u32 {
        self.0.est_cost_ms()
    }

    fn classify(&self, req: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
        self.0.classify(req)
    }
}

impl crate::kinds::Writer for SharedRemote {
    fn write(&self, kind: crate::kinds::Kind, request: &str) -> Result<String, KestrelError> {
        self.0.write(kind, request)
    }
}

impl crate::summary::Summarizer for SharedRemote {
    fn summarize(&self, texts: &[&str]) -> Result<String, KestrelError> {
        self.0.summarize(texts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in worker: ready at once, then the same answer to every request.
    fn fake(answer: &str) -> RemoteTier {
        let script =
            format!("echo '{{\"ready\": true}}'; while read -r l; do echo '{answer}'; done");
        RemoteTier::new(
            "sh".into(),
            vec!["-c".into(), script],
            900,
            Duration::from_secs(600),
        )
    }

    fn req(text: &str) -> Request {
        Request::new(1, text)
    }

    #[test]
    fn the_worker_starts_on_the_first_request_and_answers_in_intents() {
        let t = fake(
            r#"{"probs": [["create_task", 0.9], ["no_such_label", 0.05], ["chitchat", 0.05]]}"#,
        );
        assert!(!t.is_loaded());
        assert_eq!(
            t.classify(&req("remind me")).unwrap(),
            [(Intent::CreateTask, 0.9), (Intent::Chitchat, 0.05)]
        );
        assert!(t.is_loaded());
        assert_eq!(t.classify(&req("again")).unwrap()[0].0, Intent::CreateTask);
    }

    #[test]
    fn an_idle_worker_is_stopped_and_restarted_when_needed() {
        let t = fake(r#"{"probs": [["search_notes", 1.0]]}"#);
        t.classify(&req("find x")).unwrap();
        let soon = Instant::now() + Duration::from_secs(60);
        assert!(!t.reap_if_idle(soon) && t.is_loaded(), "not idle yet");
        let later = Instant::now() + Duration::from_secs(601);
        assert!(t.reap_if_idle(later) && !t.is_loaded());
        assert!(!t.reap_if_idle(later), "nothing left to stop");
        assert_eq!(
            t.classify(&req("find y")).unwrap(),
            [(Intent::SearchNotes, 1.0)]
        );
    }

    #[test]
    fn the_same_worker_writes_skill_drafts() {
        use crate::kinds::{Kind, Writer};
        let script = r#"echo '{"ready": true}'; while read -r l; do case "$l" in
            *'"write":{"kind":"email","text":"leave mail"}'*) echo '{"text": "Subject: Leave"}';;
            *) echo '{"error": "unexpected"}';; esac; done"#;
        let t = RemoteTier::new(
            "sh".into(),
            vec!["-c".into(), script.into()],
            900,
            Duration::from_secs(600),
        );
        assert_eq!(
            t.write(Kind::Email, "leave mail").unwrap(),
            "Subject: Leave"
        );
        assert!(matches!(
            t.write(Kind::Chat, "hi"),
            Err(KestrelError::Invalid(_))
        ));
    }

    #[test]
    fn the_same_worker_writes_summaries() {
        use crate::summary::Summarizer;
        let script = r#"echo '{"ready": true}'; while read -r l; do case "$l" in
            *'"summarize":["standup notes"]'*) echo '{"summary": "The budget review moved."}';;
            *) echo '{"probs": [["chitchat", 1.0]]}';; esac; done"#;
        let t = RemoteTier::new(
            "sh".into(),
            vec!["-c".into(), script.into()],
            900,
            Duration::from_secs(600),
        );
        assert_eq!(
            t.summarize(&["standup notes"]).unwrap(),
            "The budget review moved."
        );
        assert_eq!(t.classify(&req("hi")).unwrap(), [(Intent::Chitchat, 1.0)]);
        let failing = fake(r#"{"error": "the notes do not fit"}"#);
        assert!(matches!(
            failing.summarize(&["x"]),
            Err(KestrelError::Invalid(_))
        ));
    }

    #[test]
    fn a_worker_error_or_a_dead_worker_is_a_failed_tier() {
        let t = fake(r#"{"error": "control token in the request"}"#);
        assert!(matches!(
            t.classify(&req("x")),
            Err(KestrelError::Invalid(_))
        ));
        let dead = RemoteTier::new(
            "sh".into(),
            vec!["-c".into(), "echo '{\"ready\": true}'".into()],
            900,
            Duration::from_secs(600),
        );
        assert!(matches!(
            dead.classify(&req("x")),
            Err(KestrelError::ModelUnavailable(Tier::Generative))
        ));
        assert!(
            !dead.is_loaded(),
            "a dead worker is dropped, so the next request starts a new one"
        );
        let never = RemoteTier::new(
            "/no/such/kestrel".into(),
            vec![],
            900,
            Duration::from_secs(600),
        );
        assert!(matches!(
            never.classify(&req("x")),
            Err(KestrelError::ModelUnavailable(Tier::Generative))
        ));
    }
}
