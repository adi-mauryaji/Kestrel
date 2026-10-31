//! The page's speed test (v4 A3): `kestrel capabilities --measure` in its own process, so the
//! service keeps answering and the model the test loads leaves memory when it ends. The page asks
//! how far it is; the speeds land in the data folder, where the next `GET /api/capabilities` reads
//! them.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

/// Where the page's speed test is: none yet, running since, done (its speed line), or failed (why).
#[derive(Clone, Debug, PartialEq)]
enum State {
    Idle,
    Measuring(Instant),
    Done(String),
    Failed(String),
}

/// The page's speed test (one at a time).
#[derive(Clone)]
pub struct Job(Arc<Mutex<State>>);

impl Default for Job {
    fn default() -> Self {
        Job(Arc::new(Mutex::new(State::Idle)))
    }
}

impl Job {
    /// Runs `test` (the speed test) in its own process; refused while one runs.
    pub fn start(&self, mut test: Command) -> Result<(), String> {
        let mut now = self
            .0
            .lock()
            .map_err(|_| "the speed test's state was lost")?;
        if matches!(*now, State::Measuring(_)) {
            return Err("the speed test is running already".into());
        }
        let child = test
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("the speed test could not start: {e}"))?;
        *now = State::Measuring(Instant::now());
        let state = self.0.clone();
        std::thread::spawn(move || {
            let end = match child.wait_with_output() {
                Ok(o) => ended(
                    o.status.success(),
                    &String::from_utf8_lossy(&o.stdout),
                    &String::from_utf8_lossy(&o.stderr),
                ),
                Err(e) => State::Failed(format!("the speed test was lost: {e}")),
            };
            if let Ok(mut now) = state.lock() {
                *now = end;
            }
        });
        Ok(())
    }

    /// What `GET /api/speed` answers.
    pub fn json(&self) -> Value {
        let now = self.0.lock().map(|s| s.clone());
        match now.unwrap_or(State::Failed("the speed test's state was lost".into())) {
            State::Idle => json!({"state": "idle"}),
            State::Measuring(since) => {
                json!({"state": "measuring", "seconds": since.elapsed().as_secs()})
            }
            State::Done(said) => json!({"state": "done", "said": said}),
            State::Failed(why) => json!({"state": "failed", "why": why}),
        }
    }
}

/// How a finished speed test ended, from its exit and output: the line with the speeds, or why not.
fn ended(success: bool, stdout: &str, stderr: &str) -> State {
    if !success {
        let why = stderr.lines().rev().find(|l| !l.trim().is_empty());
        return State::Failed(
            why.map_or("the speed test stopped without saying why".into(), |w| {
                w.trim().trim_start_matches("kestrel: ").into()
            }),
        );
    }
    // `kestrel capabilities --measure` prints the speeds, then what the device can do with them.
    match stdout.lines().find(|l| l.contains(" tokens/s, reads ")) {
        Some(line) => State::Done(line.trim().into()),
        None => State::Failed("the speed test gave no speed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAID: &str = "measuring gemma-4-E2B-it-Q4_0 (about a minute)...\n\
        gemma-4-E2B-it-Q4_0: writes 12.5 tokens/s, reads 91 tokens/s (6 threads)\n\
        14.9 GB RAM, no GPU; a model may use 11.9 GB\n✅ Chat\n";

    #[test]
    fn a_finished_test_says_its_speeds_or_why_it_stopped() {
        assert_eq!(
            ended(true, SAID, ""),
            State::Done(
                "gemma-4-E2B-it-Q4_0: writes 12.5 tokens/s, reads 91 tokens/s (6 threads)".into()
            )
        );
        assert_eq!(
            ended(
                false,
                "measuring x...\n",
                "kestrel: models/x.gguf is missing\n"
            ),
            State::Failed("models/x.gguf is missing".into())
        );
        assert!(matches!(ended(false, "", ""), State::Failed(w) if w.contains("stopped")));
        assert!(
            matches!(ended(true, "no speeds\n", ""), State::Failed(w) if w.contains("no speed"))
        );
    }

    /// Waits until the test has ended (or 10 s).
    fn finished(job: &Job) -> Value {
        let until = Instant::now() + std::time::Duration::from_secs(10);
        while job.json()["state"] == "measuring" && Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        job.json()
    }

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script]);
        c
    }

    #[test]
    fn the_test_runs_in_its_own_process_and_says_how_it_ended() {
        let job = Job::default();
        assert_eq!(job.json(), json!({"state": "idle"}));
        job.start(sh(
            "echo 'tiny: writes 9.0 tokens/s, reads 50 tokens/s (2 threads)'",
        ))
        .unwrap();
        assert_eq!(
            finished(&job),
            json!({"state": "done", "said": "tiny: writes 9.0 tokens/s, reads 50 tokens/s (2 threads)"})
        );
        job.start(sh(
            "echo 'kestrel: no writer model here to measure' >&2; exit 1",
        ))
        .unwrap();
        assert_eq!(
            finished(&job),
            json!({"state": "failed", "why": "no writer model here to measure"})
        );
        let e = job.start(Command::new("/no/such/kestrel")).unwrap_err();
        assert!(e.contains("could not start"), "{e}");
    }

    #[test]
    fn one_test_at_a_time_and_how_long_it_has_run() {
        let job = Job::default();
        *job.0.lock().unwrap() = State::Measuring(Instant::now());
        assert_eq!(job.json(), json!({"state": "measuring", "seconds": 0}));
        let e = job.start(sh("true")).unwrap_err();
        assert!(e.contains("already"), "{e}");
    }
}
