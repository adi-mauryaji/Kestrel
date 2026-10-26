//! The always-on assistant (ADR-034). `kestrel serve --notes DIR CONFIG` keeps rules + encoder
//! loaded and answers on a Unix socket only this user can open; Tier 4 (Gemma) runs in a
//! `kestrel tier4-worker` process started on demand and stopped after `--idle-minutes` (default 10)
//! without a request (src/remote.rs). `kestrel chat` and `kestrel say TEXT...` are its clients:
//! they answer instantly because nothing loads. Every answer comes from the same `Assistant` as
//! `kestrel ask`. While it runs, tasks with a time of day ring at that time (src/remind.rs), and
//! the browser page answers on 127.0.0.1 (src/web.rs, `kestrel open`).
//!
//! Protocol, one JSON object per line. Client: `{"say": "..."}`, and `{"answer": true|false}`
//! when asked. Server: `{"part": "..."}` per piece of a skill's text while Gemma writes it (v3;
//! the client shows it at once and ends its line before what follows), `{"line": "..."}` per line
//! of the answer, `{"ask": "..."}` for a y/N question, `{"done": true}` at the end of an answer.

use std::cell::RefCell;
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

/// Where the Gemma worker keeps its primed prompt prefix (ADR-036): $XDG_CACHE_HOME/kestrel, else
/// ~/.cache/kestrel; `None` without either (the prefix is then computed at every start).
pub fn cache_dir(var: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |k: &str| var(k).filter(|v| !v.is_empty());
    let cache = var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join("kestrel"))
}

/// What answers a request: (text, a y/N question asker, a sink for text while it is written)
/// -> the answer's lines (what follows the written text).
pub type Streamer<'h> =
    dyn FnMut(&str, &mut dyn FnMut(&str) -> bool, &mut dyn FnMut(&str)) -> Vec<String> + 'h;

/// Serves one client: each `{"say"}` goes to `handle`, whose y/N questions and written parts
/// travel to the client.
pub fn handle_connection(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    handle: &mut Streamer<'_>,
) -> io::Result<()> {
    let writer = RefCell::new(writer);
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
                let mut w = writer.borrow_mut();
                if writeln!(w, "{}", json!({"ask": q}))
                    .and_then(|()| w.flush())
                    .is_err()
                {
                    return false;
                }
                drop(w);
                let mut a = String::new();
                match reader.read_line(&mut a) {
                    Ok(n) if n > 0 => serde_json::from_str::<Value>(&a)
                        .ok()
                        .and_then(|v| v["answer"].as_bool())
                        .unwrap_or(false),
                    _ => false,
                }
            };
            // A part is sent at once; a client gone mid-answer only ends its own connection.
            let mut part = |p: &str| {
                let mut w = writer.borrow_mut();
                let _ = writeln!(w, "{}", json!({"part": p})).and_then(|()| w.flush());
            };
            handle(&text, &mut confirm, &mut part)
        };
        let mut w = writer.borrow_mut();
        for l in lines {
            writeln!(w, "{}", json!({"line": l}))?;
        }
        writeln!(w, "{}", json!({"done": true}))?;
        w.flush()?;
    }
}

/// One request from the client's side: send it, print its lines, answer its questions, and show
/// written text as it comes (`part`), ended by a "\n" part before anything that follows it.
pub fn client_exchange(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    text: &str,
    answer: &mut dyn FnMut(&str) -> bool,
    print: &mut dyn FnMut(&str),
    part: &mut dyn FnMut(&str),
) -> io::Result<()> {
    writeln!(writer, "{}", json!({"say": text}))?;
    writer.flush()?;
    let mut line = String::new();
    let mut open = false; // parts shown since the last end of line
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
        if let Some(p) = v["part"].as_str() {
            part(p);
            open = true;
            continue;
        }
        if std::mem::take(&mut open) {
            part("\n");
        }
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
    let mut part = |p: &str| {
        print!("{p}");
        let _ = io::stdout().flush();
    };
    if let Some(text) = once {
        client_exchange(
            &mut reader,
            &mut writer,
            text,
            &mut answer,
            &mut print,
            &mut part,
        )?;
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
            &mut part,
        )?;
        println!();
    }
}

/// `kestrel tier4-worker CONFIG`: Gemma as the config's Tier 4, one request per line (src/remote.rs),
/// and as the writer of summaries (feature 5) in a context of its own, made at the first one.
#[cfg(feature = "llama")]
pub fn worker(config: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use kestrel_core::Request;
    use kestrel_inference::prompt::{ChatFormat, ChatPrompt};

    let cache = cache_dir(&|k| std::env::var_os(k));
    let models = crate::route_items::Models::load_for_service(config, cache.as_deref())?;
    let tiers = models.tiers(&crate::labels::Intents, |t| t, &[4])?;
    let tier = tiers.first().ok_or("the config has no Tier 4")?;
    let slm = models.slm().ok_or("the config has no Tier-4 model")?;
    let arch = slm.architecture()?;
    let format =
        ChatFormat::for_architecture(&arch).ok_or(format!("no chat format for {arch:?}"))?;
    let prompt = ChatPrompt::new(format, crate::summary::SYSTEM);
    let mut writer = None;
    // v3's kind fallback, made at its first request (its prefix is primed once, then saved).
    let mut kinds = None;
    let mut out = io::stdout().lock();
    writeln!(out, "{}", json!({"ready": true}))?;
    out.flush()?;
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line?).unwrap_or(Value::Null);
        let reply = if let Some(texts) = v["summarize"].as_array() {
            let texts: Vec<&str> = texts.iter().filter_map(Value::as_str).collect();
            match write_summary(slm, &mut writer, &prompt, &texts) {
                Ok(s) => json!({"summary": s}),
                Err(e) => json!({"error": e.to_string()}),
            }
        } else if v["write"].is_object() {
            // v3 (ADR-037): a skill request, written with its kind's instructions.
            let kind = v["write"]["kind"]
                .as_str()
                .and_then(crate::kinds::Kind::from_label);
            let text = v["write"]["text"].as_str().unwrap_or_default();
            // Each piece goes out as written (src/remote.rs); the whole text follows.
            let mut part = |p: &str| {
                let _ = writeln!(out, "{}", json!({"part": p})).and_then(|()| out.flush());
            };
            match kind.map(|k| write_skill(slm, &mut writer, format, k, text, &mut part)) {
                Some(Ok(t)) => json!({"text": t}),
                Some(Err(e)) => json!({"error": e.to_string()}),
                None => json!({"error": "unknown kind"}),
            }
        } else if let Some(text) = v["kind"].as_str() {
            let tier =
                kinds.get_or_insert_with(|| models.kinds_fallback().map_err(|e| e.to_string()));
            let req = Request::new(v["id"].as_u64().unwrap_or(0), text);
            match tier
                .as_ref()
                .map(|t| kestrel_core::TierModel::classify(t, &req))
            {
                Ok(Ok(probs)) => {
                    json!({"probs": probs.iter().map(|(k, p)| json!([k.label(), p])).collect::<Vec<_>>()})
                }
                Ok(Err(e)) => json!({"error": e.to_string()}),
                Err(e) => json!({"error": format!("no kind fallback: {e}")}),
            }
        } else {
            let req = Request::new(
                v["id"].as_u64().unwrap_or(0),
                v["text"].as_str().unwrap_or(""),
            );
            match kestrel_core::TierModel::classify(tier.as_ref(), &req) {
                Ok(probs) => {
                    json!({"probs": probs.iter().map(|(i, p)| json!([i.label(), p])).collect::<Vec<_>>()})
                }
                Err(e) => json!({"error": e.to_string()}),
            }
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}

/// Gemma's summary of `texts` (greedy, at most `summary::MAX_TOKENS`), in `writer`'s context.
#[cfg(feature = "llama")]
fn write_summary<'m>(
    slm: &'m kestrel_inference::llama::LlamaSlm,
    writer: &mut Option<kestrel_inference::llama::LlamaSession<'m>>,
    prompt: &kestrel_inference::prompt::ChatPrompt,
    texts: &[&str],
) -> Result<String, kestrel_core::KestrelError> {
    let user = crate::summary::user_message(texts);
    // Note text must not open or close chat turns (as for requests, tier.rs).
    if slm.has_special_tokens(&user)? {
        return Err(kestrel_core::KestrelError::Invalid(
            "the notes contain control-token text".into(),
        ));
    }
    answer(
        slm,
        writer,
        prompt,
        &user,
        crate::summary::MAX_TOKENS,
        &mut |_| {},
    )
}

/// A v3 skill's text (src/kinds.rs) for the request as typed, in `writer`'s context.
#[cfg(feature = "llama")]
fn write_skill<'m>(
    slm: &'m kestrel_inference::llama::LlamaSlm,
    writer: &mut Option<kestrel_inference::llama::LlamaSession<'m>>,
    format: kestrel_inference::prompt::ChatFormat,
    kind: crate::kinds::Kind,
    request: &str,
    on_part: &mut dyn FnMut(&str),
) -> Result<String, kestrel_core::KestrelError> {
    // The request must not open or close chat turns (as for Tier 4, tier.rs).
    if slm.has_special_tokens(request)? {
        return Err(kestrel_core::KestrelError::Invalid(
            "the request contains control-token text".into(),
        ));
    }
    let prompt = kestrel_inference::prompt::ChatPrompt::new(format, kind.system());
    answer(slm, writer, &prompt, request, kind.max_tokens(), on_part)
}

/// Gemma's greedy answer (at most `max_tokens`) to `user` under `prompt`, in `writer`'s context
/// (made at the first use), trimmed of space and the stop string; its pieces go to `on_part` as
/// they are written (src/stream.rs), and they join to the answer.
#[cfg(feature = "llama")]
fn answer<'m>(
    slm: &'m kestrel_inference::llama::LlamaSlm,
    writer: &mut Option<kestrel_inference::llama::LlamaSession<'m>>,
    prompt: &kestrel_inference::prompt::ChatPrompt,
    user: &str,
    max_tokens: usize,
    on_part: &mut dyn FnMut(&str),
) -> Result<String, kestrel_core::KestrelError> {
    use kestrel_core::resources::{CancelToken, Deadline};
    use kestrel_inference::generate::{GenParams, Generator};

    let session = match writer {
        Some(s) => s,
        None => writer.insert(slm.session()?),
    };
    let stop = prompt.end_of_answer();
    let p = GenParams {
        stop: vec![stop.into()],
        ..GenParams::greedy(max_tokens)
    };
    let (mut trimmed, mut out) = (crate::stream::Trimmed::new(stop), String::new());
    let mut show = |s: String| {
        if !s.is_empty() {
            on_part(&s);
            out.push_str(&s);
        }
    };
    session.generate(
        &(prompt.prefix() + &prompt.suffix(user)),
        &p,
        &CancelToken::new(),
        &Deadline::after(std::time::Duration::from_secs(120)),
        &mut |t| show(trimmed.push(t)),
    )?;
    show(trimmed.finish());
    Ok(out)
}

/// The page on 127.0.0.1:`port`: its token beside the memory file (kept across restarts, so a
/// bookmark keeps working), its address with the token for `kestrel open`, and its own connection
/// to the memory file for the task list.
#[cfg(feature = "llama")]
fn open_web(
    port: u16,
    memory: &std::path::Path,
) -> Result<crate::web::Web, Box<dyn std::error::Error>> {
    let dir = memory.parent().unwrap_or(std::path::Path::new("."));
    let token = crate::web::token(&dir.join("web-token"))?;
    let tasks_db = crate::memory::open(memory)?;
    let offset = crate::clock::utc_offset();
    let tasks = move || {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        crate::web::tasks_json(
            &tasks_db.tasks().unwrap_or_default(),
            crate::clock::local_date(now, offset),
            crate::clock::local_minute(now, offset),
        )
    };
    let web = crate::web::Web::bind(port, token.clone(), Box::new(tasks))?;
    let file = crate::web::url_file(&|k| std::env::var_os(k));
    crate::web::write_private(&file, &crate::web::url(web.port(), &token))?;
    Ok(web)
}

/// `kestrel serve --notes DIR [--idle-minutes N] [--web-port N | --no-web] CONFIG`.
#[cfg(feature = "llama")]
pub fn serve(
    config: &std::path::Path,
    notes: &std::path::Path,
    memory: &std::path::Path,
    idle: std::time::Duration,
    web_port: Option<u16>,
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
    let act = act.with_summarizer(Box::new(SharedRemote(remote.clone())));
    let mut assistant =
        crate::assistant::Assistant::new(cascade, log, personal, Some(act), sink, true);
    // v3 (ADR-037): the kind head, once experiments/tier3/train.py has made it.
    match models.kinds_tier() {
        Ok(head) => {
            let writer = Box::new(SharedRemote(remote.clone()));
            let mut kinds = crate::kinds::Kinds::new(Box::new(head), writer, crate::kinds::TAU);
            // Gemma decides when the head is unsure; without its config the head alone asks.
            match crate::kinds::FallbackConfig::load(std::path::Path::new(
                crate::kinds::FALLBACK_CONFIG,
            )) {
                Ok(fb) => {
                    let fallback = Box::new(crate::remote::KindFallback(remote.clone()));
                    kinds = kinds.with_fallback(fallback, fb.tau_g);
                }
                Err(e) => eprintln!("the kind fallback is off (unsure requests are asked): {e}"),
            }
            assistant = assistant.with_kinds(kinds);
        }
        Err(e) => eprintln!("v3 skills (chat, email, rewrite, translate) are off: {e}"),
    }

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
    // Tasks with a time ring from their own thread, so a long `kestrel chat` does not hold them up.
    let ring_db = memory.to_path_buf();
    std::thread::spawn(move || crate::remind::ring_forever(&ring_db));
    // The browser page; without it the service still answers on the socket.
    let mut web = web_port.and_then(|port| match open_web(port, memory) {
        Ok(w) => {
            eprintln!(
                "the page is at http://127.0.0.1:{}/ (kestrel open)",
                w.port()
            );
            Some(w)
        }
        Err(e) => {
            eprintln!("the browser page is off: {e}");
            None
        }
    });
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                let mut reader = io::BufReader::new(stream.try_clone()?);
                let mut writer = stream;
                // A client that goes away mid-answer only ends its own connection.
                let _ = handle_connection(&mut reader, &mut writer, &mut |text, confirm, part| {
                    assistant.handle_streaming(text, confirm, part)
                });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Some(w) = web.as_mut()
                    && let Err(e) = w.poll(&mut |text, confirm, part| {
                        assistant.handle_streaming(text, confirm, part)
                    })
                {
                    eprintln!("the browser page stopped: {e}");
                    web = None;
                }
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
    fn the_prefix_cache_lives_in_the_users_cache_directory() {
        let env = |k: &str| match k {
            "XDG_CACHE_HOME" => Some(OsString::from("/c")),
            "HOME" => Some(OsString::from("/home/adi")),
            _ => None,
        };
        assert_eq!(cache_dir(&env), Some(PathBuf::from("/c/kestrel")));
        let home = |k: &str| match k {
            "XDG_CACHE_HOME" => Some(OsString::new()),
            "HOME" => Some(OsString::from("/home/adi")),
            _ => None,
        };
        assert_eq!(
            cache_dir(&home),
            Some(PathBuf::from("/home/adi/.cache/kestrel"))
        );
        assert_eq!(cache_dir(&|_| None), None);
    }

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
        handle_connection(&mut input, &mut output, &mut |text, confirm, _| {
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
        handle_connection(&mut input, &mut output, &mut |_, confirm, _| {
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
            &mut |_| panic!("no parts"),
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
            client_exchange(
                &mut cut,
                &mut Vec::new(),
                "x",
                &mut |_| false,
                &mut |_| {},
                &mut |_| {}
            )
            .is_err(),
            "server went away"
        );
    }

    #[test]
    fn the_server_relays_parts_as_they_come() {
        let mut input = Cursor::new(b"{\"say\": \"explain recursion\"}\n".to_vec());
        let mut output = Vec::new();
        handle_connection(&mut input, &mut output, &mut |_, _, part| {
            part("Recursion is");
            part(" a function calling itself.");
            vec!["(note)".into()]
        })
        .unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "{\"part\":\"Recursion is\"}\n{\"part\":\" a function calling itself.\"}\n\
             {\"line\":\"(note)\"}\n{\"done\":true}\n"
        );
    }

    #[test]
    fn the_client_shows_parts_at_once_and_ends_their_line_before_what_follows() {
        let mut from_server = Cursor::new(
            b"{\"part\":\"Dear\"}\n{\"part\":\" Asha\"}\n{\"line\":\"(note)\"}\n{\"done\":true}\n\
              {\"part\":\"Hi\"}\n{\"done\":true}\n"
                .to_vec(),
        );
        for (parts_seen, lines_seen) in [
            (vec!["Dear", " Asha", "\n"], vec!["(note)"]),
            (vec!["Hi", "\n"], vec![]),
        ] {
            let (mut parts, mut printed) = (Vec::new(), Vec::new());
            client_exchange(
                &mut from_server,
                &mut Vec::new(),
                "x",
                &mut |_| false,
                &mut |l| printed.push(l.to_string()),
                &mut |p| parts.push(p.to_string()),
            )
            .unwrap();
            assert_eq!(
                (parts, printed),
                (
                    parts_seen.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    lines_seen
                        .iter()
                        .map(|s: &&str| s.to_string())
                        .collect::<Vec<_>>()
                )
            );
        }
    }
}
