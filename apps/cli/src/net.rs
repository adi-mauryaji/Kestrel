//! The way to `kestrel-net` (v4 C, blueprint §4.8). The core links no network client: a request goes to
//! that program over JSON lines on stdio. It starts for the request, beside this program, and is gone
//! after it, so nothing network-shaped stays running. The key for approval tokens lives here and
//! reaches it as its first line.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use kestrel_net_proto::{Op, Request};
use serde_json::Value;

use crate::send::Link;

/// The longest a request may take; its own limits (30 s for a mail) are inside this.
const LIMIT: Duration = Duration::from_secs(90);
/// How often a request with no time limit is asked whether it should go on.
const WATCH_EVERY: Duration = Duration::from_secs(1);
/// "No time limit", as a time.
const NO_LIMIT: Duration = Duration::from_secs(60 * 60 * 24 * 365);

pub struct Net {
    command: Box<dyn Fn() -> Command>,
    key: String,
    limit: Duration,
}

impl Net {
    /// `kestrel-net` beside this program, with a new key.
    pub fn beside_this_program() -> std::io::Result<Self> {
        let name = format!("kestrel-net{}", std::env::consts::EXE_SUFFIX);
        let program = std::env::current_exe()?.with_file_name(name);
        let bytes = kestrel_platform::random_bytes(kestrel_platform::Os::current(), 32)?;
        let key = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Ok(Self::with(
            Box::new(move || Command::new(&program)),
            key,
            LIMIT,
        ))
    }

    fn with(command: Box<dyn Fn() -> Command>, key: String, limit: Duration) -> Self {
        Self {
            command,
            key,
            limit,
        }
    }
}

impl Link for Net {
    fn key(&self) -> String {
        self.key.clone()
    }

    fn ask(&self, op: Op) -> Result<Value, String> {
        self.run(op, Instant::now() + self.limit, &mut || true)
    }

    fn ask_while(&self, op: Op, still: &mut dyn FnMut() -> bool) -> Result<Value, String> {
        self.run(op, Instant::now() + NO_LIMIT, still)
    }
}

impl Net {
    fn run(
        &self,
        op: Op,
        until: Instant,
        still: &mut dyn FnMut() -> bool,
    ) -> Result<Value, String> {
        let mut child = (self.command)()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("kestrel-net could not start: {e}"))?;
        let (Some(to), Some(from)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("kestrel-net could not start".into());
        };
        // Read on a thread of its own, so a program that hangs cannot hold this one past the limit.
        let (lines, heard) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(from).lines() {
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        let answer = talk(to, &heard, &self.key, op, until, still);
        let _ = child.kill();
        let _ = child.wait();
        answer
    }
}

/// One request: the key, its ready line, the request, its answer.
fn talk(
    mut to: impl Write,
    heard: &mpsc::Receiver<std::io::Result<String>>,
    key: &str,
    op: Op,
    until: Instant,
    still: &mut dyn FnMut() -> bool,
) -> Result<Value, String> {
    const ID: u64 = 1;
    const STOPPED: &str = "kestrel-net stopped";
    let mut next = || loop {
        let left = until.saturating_duration_since(Instant::now());
        match heard.recv_timeout(left.min(WATCH_EVERY)) {
            Ok(Ok(line)) => return Ok(serde_json::from_str::<Value>(&line).unwrap_or(Value::Null)),
            Err(RecvTimeoutError::Timeout) if left <= WATCH_EVERY => {
                return Err("kestrel-net gave no answer in time".to_string());
            }
            Err(RecvTimeoutError::Timeout) if !still() => {
                return Err("kestrel-net was stopped".to_string());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => return Err(STOPPED.to_string()),
        }
    };
    let mut say = |line: &str| {
        writeln!(to, "{line}")
            .and_then(|()| to.flush())
            .map_err(|_| STOPPED.to_string())
    };
    say(key)?;
    if next()?["ready"] != true {
        return Err("kestrel-net did not start properly".into());
    }
    let request = serde_json::to_string(&Request { id: ID, op })
        .map_err(|e| format!("kestrel-net: the request could not be written: {e}"))?;
    say(&request)?;
    let reply = next()?;
    if reply["id"] != ID {
        return Err(reply["error"]
            .as_str()
            .unwrap_or("kestrel-net answered something else")
            .to_string());
    }
    match reply["error"].as_str() {
        Some(why) => Err(why.to_string()),
        None => Ok(reply),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_net_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A stand-in for `kestrel-net`: a shell script, run by `sh` so no file is executed while written.
    fn fake(d: &std::path::Path, script: &str, limit: Duration) -> Net {
        let file = d.join("fake.sh");
        std::fs::write(&file, script).unwrap();
        Net::with(
            Box::new(move || {
                let mut c = Command::new("sh");
                c.arg(&file);
                c
            }),
            "the-key".into(),
            limit,
        )
    }

    #[test]
    fn the_key_goes_first_then_the_request_and_the_answer_comes_back() {
        let d = folder("answer");
        let seen = d.join("seen");
        let script = format!(
            "read key\necho '{{\"ready\":true}}'\nread line\nprintf '%s\\n%s\\n' \"$key\" \"$line\" > {}\necho '{{\"id\":1,\"sent\":true}}'\n",
            seen.display()
        );
        let net = fake(&d, &script, LIMIT);
        let got = net.ask(Op::SecretHas).unwrap();
        assert_eq!(got["sent"], true);
        let seen = std::fs::read_to_string(&seen).unwrap();
        let mut lines = seen.lines();
        assert_eq!(lines.next(), Some("the-key"));
        let asked: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(
            (asked["id"].as_u64(), asked["op"].as_str()),
            (Some(1), Some("secret_has"))
        );
        // Each request starts it again.
        assert_eq!(net.ask(Op::SecretHas).unwrap()["sent"], true);
    }

    #[test]
    fn its_refusal_is_an_error_with_its_reason() {
        let d = folder("refused");
        let script = "read key\necho '{\"ready\":true}'\nread line\necho '{\"id\":1,\"error\":\"Gmail did not take the mail\"}'\n";
        let net = fake(&d, script, LIMIT);
        assert_eq!(
            net.ask(Op::SecretHas).unwrap_err(),
            "Gmail did not take the mail"
        );
    }

    #[test]
    fn an_answer_to_another_request_is_not_taken() {
        let d = folder("other");
        let script =
            "read key\necho '{\"ready\":true}'\nread line\necho '{\"id\":9,\"sent\":true}'\n";
        let net = fake(&d, script, LIMIT);
        assert!(net.ask(Op::SecretHas).unwrap_err().contains("kestrel-net"));
    }

    #[test]
    fn a_program_that_stops_or_is_missing_says_so() {
        let d = folder("stops");
        let net = fake(&d, "exit 3\n", LIMIT);
        assert!(net.ask(Op::SecretHas).unwrap_err().contains("kestrel-net"));
        let net = Net::with(
            Box::new(|| Command::new("/nowhere/kestrel-net")),
            "k".into(),
            LIMIT,
        );
        assert!(net.ask(Op::SecretHas).unwrap_err().contains("kestrel-net"));
    }

    #[test]
    fn a_long_request_goes_on_until_it_is_told_to_stop() {
        let d = folder("while");
        let pid = d.join("pid");
        let script = format!(
            "read key\necho '{{\"ready\":true}}'\necho $$ > {}\nexec sleep 30\n",
            pid.display()
        );
        let net = fake(&d, &script, Duration::from_millis(1));
        let mut asked = 0;
        let got = net.ask_while(Op::SecretHas, &mut || {
            asked += 1;
            asked < 2
        });
        assert!(got.unwrap_err().contains("stopped"));
        assert_eq!(asked, 2, "past the short limit, which is not for this kind");
        let pid = std::fs::read_to_string(&pid).unwrap();
        assert!(
            !std::path::Path::new(&format!("/proc/{}", pid.trim())).exists()
                || cfg!(target_os = "macos")
        );
    }

    #[test]
    fn a_long_request_ends_with_its_answer() {
        let d = folder("while_ok");
        let script = "read key\necho '{\"ready\":true}'\nread line\nsleep 1.2\necho '{\"id\":1,\"bytes\":6}'\n";
        let net = fake(&d, script, LIMIT);
        let got = net.ask_while(Op::SecretHas, &mut || true).unwrap();
        assert_eq!(got["bytes"], 6);
    }

    #[test]
    fn a_program_that_hangs_is_stopped_at_the_limit() {
        let d = folder("hangs");
        let pid = d.join("pid");
        let script = format!(
            "read key\necho '{{\"ready\":true}}'\necho $$ > {}\nexec sleep 30\n",
            pid.display()
        );
        let net = fake(&d, &script, Duration::from_millis(400));
        let t = Instant::now();
        assert!(net.ask(Op::SecretHas).unwrap_err().contains("no answer"));
        assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
        let pid = std::fs::read_to_string(&pid).unwrap();
        assert!(
            !std::path::Path::new(&format!("/proc/{}", pid.trim())).exists()
                || cfg!(target_os = "macos")
        );
    }
}
