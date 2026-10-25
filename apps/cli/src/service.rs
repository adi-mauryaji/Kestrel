//! The always-on assistant (ADR-034). `kestrel serve --notes DIR CONFIG` keeps rules + encoder
//! loaded and answers on a Unix socket only this user can open; Tier 4 (Gemma) runs in a
//! `kestrel tier4-worker` process started on demand and stopped after `--idle-minutes` (default 10)
//! without a request (src/remote.rs). `kestrel chat` and `kestrel say TEXT...` are its clients:
//! they answer instantly because nothing loads. Every answer comes from the same `Assistant` as
//! `kestrel ask`.
//!
//! Protocol, one JSON object per line. Client: `{"say": "..."}`, and `{"answer": true|false}`
//! when asked. Server: `{"line": "..."}` per line of the answer, `{"ask": "..."}` for a y/N
//! question, `{"done": true}` at the end of an answer.

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde_json::{Value, json};

/// $XDG_RUNTIME_DIR/kestrel.sock (a per-user 0700 directory), else /tmp/kestrel-$USER.sock.
pub fn socket_path(var: &dyn Fn(&str) -> Option<OsString>) -> PathBuf {
    match var("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("kestrel.sock"),
        None => {
            let user = var("USER")
                .and_then(|u| u.into_string().ok())
                .unwrap_or_else(|| "user".into());
            PathBuf::from(format!("/tmp/kestrel-{user}.sock"))
        }
    }
}

/// What answers a request: (text, a y/N question asker) -> the answer's lines.
pub type Handler<'h> = dyn FnMut(&str, &mut dyn FnMut(&str) -> bool) -> Vec<String> + 'h;

/// Serves one client: each `{"say"}` goes to `handle`, whose y/N questions travel to the client.
pub fn handle_connection(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    handle: &mut Handler<'_>,
) -> io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let Some(text) = serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|v| v["say"].as_str().map(String::from))
        else {
            continue;
        };
        let lines = {
            // A question goes to the client; no readable answer counts as no.
            let mut confirm = |q: &str| -> bool {
                if writeln!(writer, "{}", json!({"ask": q}))
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    return false;
                }
                let mut a = String::new();
                match reader.read_line(&mut a) {
                    Ok(n) if n > 0 => serde_json::from_str::<Value>(&a)
                        .ok()
                        .and_then(|v| v["answer"].as_bool())
                        .unwrap_or(false),
                    _ => false,
                }
            };
            handle(&text, &mut confirm)
        };
        for l in lines {
            writeln!(writer, "{}", json!({"line": l}))?;
        }
        writeln!(writer, "{}", json!({"done": true}))?;
        writer.flush()?;
    }
}

/// One request from the client's side: send it, print its lines, answer its questions.
pub fn client_exchange(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    text: &str,
    answer: &mut dyn FnMut(&str) -> bool,
    print: &mut dyn FnMut(&str),
) -> io::Result<()> {
    writeln!(writer, "{}", json!({"say": text}))?;
    writer.flush()?;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the assistant closed the connection",
            ));
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(l) = v["line"].as_str() {
            print(l);
        } else if let Some(q) = v["ask"].as_str() {
            let a = answer(q);
            writeln!(writer, "{}", json!({"answer": a}))?;
            writer.flush()?;
        } else if v["done"] == true {
            return Ok(());
        }
    }
}

/// `kestrel chat` (one request per line) or `kestrel say TEXT` (`once`): the service's client.
pub fn chat(once: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::net::UnixStream;

    let path = socket_path(&|k| std::env::var_os(k));
    let stream = UnixStream::connect(&path).map_err(|_| {
        format!(
            "Kestrel is not running ({}): start it with scripts/install_service.sh (always on), \
             or run `kestrel serve --notes DIR CONFIG`",
            path.display()
        )
    })?;
    let mut reader = io::BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let stdin = io::stdin();
    let mut answer = |q: &str| {
        print!("{q} [y/N] ");
        let _ = io::stdout().flush();
        let mut a = String::new();
        if stdin.lock().read_line(&mut a).unwrap_or(0) == 0 {
            return false;
        }
        matches!(
            a.trim().to_lowercase().as_str(),
            "y" | "yes" | "haan" | "ha"
        )
    };
    let mut print = |l: &str| println!("{l}");
    if let Some(text) = once {
        client_exchange(&mut reader, &mut writer, text, &mut answer, &mut print)?;
        return Ok(());
    }
    eprintln!(
        "connected to Kestrel; type a request (English or Hinglish); \"quit\" or Ctrl-D ends"
    );
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 || matches!(line.trim(), "quit" | "exit") {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }
        client_exchange(
            &mut reader,
            &mut writer,
            line.trim(),
            &mut answer,
            &mut print,
        )?;
        println!();
    }
}

/// `kestrel tier4-worker CONFIG`: Gemma as the config's Tier 4, one request per line (src/remote.rs).
#[cfg(feature = "llama")]
pub fn worker(config: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use kestrel_core::Request;

    let models = crate::route_items::Models::load_for(config, &[4])?;
    let tiers = models.tiers(&crate::labels::Intents, |t| t, &[4])?;
    let tier = tiers.first().ok_or("the config has no Tier 4")?;
    let mut out = io::stdout().lock();
    writeln!(out, "{}", json!({"ready": true}))?;
    out.flush()?;
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line?).unwrap_or(Value::Null);
        let req = Request::new(
            v["id"].as_u64().unwrap_or(0),
            v["text"].as_str().unwrap_or(""),
        );
        let reply = match kestrel_core::TierModel::classify(tier.as_ref(), &req) {
            Ok(probs) => {
                json!({"probs": probs.iter().map(|(i, p)| json!([i.label(), p])).collect::<Vec<_>>()})
            }
            Err(e) => json!({"error": e.to_string()}),
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}

/// `kestrel serve --notes DIR [--idle-minutes N] CONFIG`.
#[cfg(feature = "llama")]
pub fn serve(
    config: &std::path::Path,
    notes: &std::path::Path,
    memory: &std::path::Path,
    idle: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::rc::Rc;
    use std::time::Instant;

    use crate::ask::{Log, Recorder};
    use crate::remote::{RemoteTier, SharedRemote};

    let (router, cfg) = crate::config::load(&std::fs::read_to_string(config)?)?;
    let list = std::path::Path::new("datasets/contracts").join(&cfg.models.tier4.labels);
    let intents =
        crate::labels::Intents::check(&crate::labels::parse_list(&std::fs::read_to_string(list)?))?;
    if !notes.is_dir() {
        return Err(format!("no notes folder at {}", notes.display()).into());
    }
    let act = crate::act::Act::open(notes, memory)?;
    let path = socket_path(&|k| std::env::var_os(k));
    // sockaddr_un holds about 108 bytes; a deep XDG_RUNTIME_DIR would fail with a cryptic error.
    if path.as_os_str().len() >= 100 {
        return Err(format!(
            "the socket path {} is too long for a Unix socket ({} bytes, the limit is about 100): \
             set XDG_RUNTIME_DIR to a shorter directory",
            path.display(),
            path.as_os_str().len()
        )
        .into());
    }
    if UnixStream::connect(&path).is_ok() {
        return Err(format!("Kestrel is already running ({})", path.display()).into());
    }
    let _ = std::fs::remove_file(&path); // a socket left by a stopped service

    let t0 = Instant::now();
    let below4: Vec<u8> = cfg.tiers.iter().copied().filter(|&t| t != 4).collect();
    let models = crate::route_items::Models::load_for(config, &below4)?;
    let log = Log::default();
    let wrap = |t| Box::new(Recorder::new(t, log.clone())) as Box<dyn kestrel_core::TierModel + '_>;
    let mut tiers = models.tiers(&intents, wrap, &below4)?;
    let remote = Rc::new(RemoteTier::new(
        std::env::current_exe()?,
        vec!["tier4-worker".into(), config.display().to_string()],
        cfg.est_cost_ms[3],
        idle,
    ));
    if cfg.tiers.contains(&4) {
        tiers.push(wrap(Box::new(SharedRemote(remote.clone()))));
    }
    let cascade = kestrel_router::Cascade::new(tiers, router)?;
    let personal = crate::personal::Personal::open(Some(memory));
    let sink = crate::telemetry::Sink::open(
        std::env::var_os("KESTREL_TELEMETRY")
            .as_deref()
            .map(std::path::Path::new),
    );
    let mut assistant =
        crate::assistant::Assistant::new(cascade, log, personal, Some(act), sink, true);

    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    eprintln!(
        "Kestrel ready in {:.1} s on {} (notes {}, memory {}); the SLM loads when first needed",
        t0.elapsed().as_secs_f64(),
        path.display(),
        notes.display(),
        memory.display()
    );
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                let mut reader = io::BufReader::new(stream.try_clone()?);
                let mut writer = stream;
                // A client that goes away mid-answer only ends its own connection.
                let _ = handle_connection(&mut reader, &mut writer, &mut |text, confirm| {
                    assistant.handle(text, confirm)
                });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if remote.reap_if_idle(Instant::now()) {
                    eprintln!("the SLM was idle for {} min: unloaded", idle.as_secs() / 60);
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn the_socket_lives_in_the_users_runtime_directory() {
        let env = |k: &str| match k {
            "XDG_RUNTIME_DIR" => Some(OsString::from("/run/user/1000")),
            "USER" => Some(OsString::from("adi")),
            _ => None,
        };
        assert_eq!(
            socket_path(&env),
            PathBuf::from("/run/user/1000/kestrel.sock")
        );
        let no_runtime = |k: &str| (k == "USER").then(|| OsString::from("adi"));
        assert_eq!(
            socket_path(&no_runtime),
            PathBuf::from("/tmp/kestrel-adi.sock")
        );
        assert_eq!(
            socket_path(&|_| None),
            PathBuf::from("/tmp/kestrel-user.sock")
        );
    }

    #[test]
    fn the_server_relays_questions_and_ends_every_answer() {
        let mut input = Cursor::new(
            b"{\"say\": \"delete task 2\"}\n{\"answer\": true}\n{\"say\": \"hi\"}\n".to_vec(),
        );
        let mut output = Vec::new();
        let mut seen = Vec::new();
        handle_connection(&mut input, &mut output, &mut |text, confirm| {
            seen.push(text.to_string());
            if text.starts_with("delete") {
                let yes = confirm("delete task 2?");
                return vec![format!("answered {yes}")];
            }
            vec!["hello".into(), "second line".into()]
        })
        .unwrap();
        assert_eq!(seen, ["delete task 2", "hi"]);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "{\"ask\":\"delete task 2?\"}\n{\"line\":\"answered true\"}\n{\"done\":true}\n\
             {\"line\":\"hello\"}\n{\"line\":\"second line\"}\n{\"done\":true}\n"
        );
    }

    #[test]
    fn a_client_without_an_answer_says_no_and_garbage_is_skipped() {
        let mut input = Cursor::new(b"{\"say\": \"delete task 2\"}\nnot json\n".to_vec());
        let mut output = Vec::new();
        handle_connection(&mut input, &mut output, &mut |_, confirm| {
            vec![format!("answered {}", confirm("sure?"))]
        })
        .unwrap();
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("\"answered false\"")
        );
    }

    #[test]
    fn the_client_prints_lines_and_answers_questions() {
        let mut from_server = Cursor::new(
            b"{\"line\":\"a\"}\n{\"ask\":\"q?\"}\n{\"line\":\"b\"}\n{\"done\":true}\n".to_vec(),
        );
        let mut to_server = Vec::new();
        let (mut asked, mut printed) = (Vec::new(), Vec::new());
        client_exchange(
            &mut from_server,
            &mut to_server,
            "task 2 hata do",
            &mut |q| {
                asked.push(q.to_string());
                false
            },
            &mut |l| printed.push(l.to_string()),
        )
        .unwrap();
        assert_eq!(
            (asked, printed),
            (
                vec!["q?".to_string()],
                vec!["a".to_string(), "b".to_string()]
            )
        );
        assert_eq!(
            String::from_utf8(to_server).unwrap(),
            "{\"say\":\"task 2 hata do\"}\n{\"answer\":false}\n"
        );
        let mut cut = Cursor::new(b"{\"line\":\"a\"}\n".to_vec());
        assert!(
            client_exchange(&mut cut, &mut Vec::new(), "x", &mut |_| false, &mut |_| {}).is_err(),
            "server went away"
        );
    }
}
