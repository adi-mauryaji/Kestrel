//! The browser page (ADR-034 (4)): `kestrel serve` also answers HTTP on 127.0.0.1, with the
//! standard library only, from the same `Assistant` as `kestrel chat`. `kestrel open` opens it.
//!
//! - `GET /` is the page (apps/cli/web/page.html, one file, nothing fetched from elsewhere: its
//!   Content-Security-Policy allows only this server), built to the human's design (2026-10-31);
//!   `GET /fonts/*.woff2` its three fonts (apps/cli/web/fonts, OFL), without the key.
//! - `GET /api/capabilities` (v4 A2): what Kestrel can do on this device, card by card
//!   (src/capabilities.rs).
//! - `GET /api/events?after=N` (v4 A3): reminders that rang, for the Kestrel app, answered at once
//!   (src/events.rs).
//! - `GET /api/tasks`: the tasks, today's date, the minute now and whether v3's skills are on
//!   (the page then names them). `POST /api/say {"text"}`: the
//!   answer as JSON lines, `{"part"}` ... while a skill's text is written (v3), `{"line"}` ...,
//!   `{"ask"}` for a y/N question, `{"done": true}`; the page answers a question with
//!   `POST /api/answer {"answer": bool}`, served while it waits.
//! - Who may ask: every request must name this server in `Host` (a web page elsewhere cannot
//!   reach it by DNS rebinding); the API also needs `X-Kestrel-Token`, a secret kept in a 0600
//!   file that `kestrel open` passes in the URL fragment (never sent to a server). The page itself
//!   holds no secret.
//! - One request at a time, like the Unix socket; a read stalls at most `READ_TIMEOUT`. Each
//!   connection ends gently (`finish`), so the browser never loses the end of an answer.

use std::cell::RefCell;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;

use kestrel_memory::TaskRow;
use kestrel_tools::date::Date;
use kestrel_tools::tools::Note;
use serde_json::{Value, json};

use crate::service::Streamer;

pub const DEFAULT_PORT: u16 = 7811;
pub const PAGE: &str = include_str!("../web/page.html");
/// The largest request body read (a typed request is far smaller).
pub const MAX_BODY: usize = 64 * 1024;
/// The longest request or header line read.
const MAX_LINE: u64 = 8 * 1024;
const MAX_HEADERS: usize = 100;
/// The page may run its own inline script and style and talk to this server; nothing else.
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
img-src data:; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; \
frame-ancestors 'none'";
/// The page's fonts (OFL, apps/cli/web/fonts with their licences): headings, the day's line, text.
const FONTS: [(&str, &[u8]); 3] = [
    (
        "/fonts/source-serif-4.woff2",
        include_bytes!("../web/fonts/source-serif-4.woff2"),
    ),
    (
        "/fonts/caveat.woff2",
        include_bytes!("../web/fonts/caveat.woff2"),
    ),
    (
        "/fonts/inter.woff2",
        include_bytes!("../web/fonts/inter.woff2"),
    ),
];
const JSON: &str = "application/json";
/// A browser that stops sending holds the service at most this long.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// How long an answered connection waits for the browser to close before it is closed anyway.
const LINGER: std::time::Duration = std::time::Duration::from_millis(100);
/// How long a y/N question waits for the page's answer.
const ANSWER_WAIT: std::time::Duration = std::time::Duration::from_secs(120);
const TEXT: &str = "text/plain; charset=utf-8";

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    /// Names lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One request: request line, headers, and a body of `Content-Length` bytes (at most `MAX_BODY`).
pub fn read_request(r: &mut dyn BufRead) -> io::Result<HttpRequest> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
    let mut line = String::new();
    let mut next = |line: &mut String| -> io::Result<bool> {
        line.clear();
        Ok((&mut *r).take(MAX_LINE).read_line(line)? > 0)
    };
    if !next(&mut line)? {
        return Err(bad("no request"));
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    let [method, path, version] = parts[..] else {
        return Err(bad("request line"));
    };
    if !version.starts_with("HTTP/") {
        return Err(bad("request line"));
    }
    let (method, path) = (method.to_string(), path.to_string());
    let mut headers = Vec::new();
    loop {
        if !next(&mut line)? {
            return Err(bad("the headers end early"));
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            break;
        }
        if headers.len() == MAX_HEADERS {
            return Err(bad("too many headers"));
        }
        let (k, v) = l.split_once(':').ok_or_else(|| bad("header"))?;
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    let mut req = HttpRequest {
        method,
        path,
        headers,
        body: Vec::new(),
    };
    let len: usize = match req.header("content-length") {
        Some(v) => v.parse().map_err(|_| bad("content-length"))?,
        None => 0,
    };
    if len > MAX_BODY {
        return Err(bad("the body is too large"));
    }
    req.body = vec![0; len];
    r.read_exact(&mut req.body)?;
    Ok(req)
}

/// Ok, or the status to refuse with: 403 for a `Host` that is not this server, 401 for an API
/// request without the token. Shared on the wifi (`lan`), that address is this server too.
pub fn allowed(
    req: &HttpRequest,
    port: u16,
    token: &str,
    lan: Option<std::net::Ipv4Addr>,
) -> Result<(), u16> {
    let host = req.header("host").unwrap_or("");
    let here = host == format!("127.0.0.1:{port}")
        || host == format!("localhost:{port}")
        || lan.is_some_and(|ip| host == format!("{ip}:{port}"));
    if !here {
        return Err(403);
    }
    let has_token = req
        .header("x-kestrel-token")
        .is_some_and(|t| same(t.as_bytes(), token.as_bytes()));
    if req.path.starts_with("/api/") && !has_token {
        return Err(401);
    }
    Ok(())
}

/// Equal, in a time that does not depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Where `kestrel serve` leaves the page's address with its token, for `kestrel open`: beside the
/// socket ($XDG_RUNTIME_DIR/kestrel.url).
pub fn url_file(var: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> std::path::PathBuf {
    crate::service::socket_path(var).with_extension("url")
}

/// The port in a page address: "http://127.0.0.1:7811/#t=..." -> 7811.
pub fn port_of(address: &str) -> Option<u16> {
    let host = address.trim().strip_prefix("http://")?.split('/').next()?;
    host.rsplit_once(':')?.1.parse().ok()
}

/// The running service's page answers on this machine: how Windows, without a socket, tells
/// whether Kestrel runs.
#[cfg_attr(unix, allow(dead_code))]
pub fn page_answers(var: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> bool {
    std::fs::read_to_string(url_file(var))
        .ok()
        .and_then(|a| port_of(&a))
        .is_some_and(|port| {
            let at = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
            std::net::TcpStream::connect_timeout(&at, std::time::Duration::from_millis(500)).is_ok()
        })
}

/// The page's address; the token rides in the fragment, which the browser never sends.
pub fn url(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/#t={token}")
}

/// Where the service leaves the page's wifi address with its token, when it is shared there.
pub fn lan_url_file(var: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> std::path::PathBuf {
    crate::service::socket_path(var).with_extension("lan.url")
}

/// The page's address on the wifi (plain HTTP: anyone on that network can read what it carries).
pub fn lan_url(ip: std::net::Ipv4Addr, port: u16, token: &str) -> String {
    format!("http://{ip}:{port}/#t={token}")
}

/// `kestrel open [--print] [--lan]`: opens the running Kestrel's page in the browser (or prints its
/// address); `--lan`, its address on the wifi when the service shares it there (`--web-lan`).
pub fn open(
    args: &[String],
    var: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    out: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.iter().any(|a| a == "--lan") {
        let address = std::fs::read_to_string(lan_url_file(var)).map_err(|_| {
            "the page is not shared on the wifi: KESTREL_WEB_LAN=<this laptop's IP> \
             scripts/install_service.sh ~/notes"
        })?;
        writeln!(out, "{}", address.trim())?;
        return Ok(());
    }
    let path = url_file(var);
    let address = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "Kestrel is not running ({} is missing): {}",
            path.display(),
            crate::service::start_hint()
        )
    })?;
    let address = address.trim();
    if args.iter().any(|a| a == "--print") {
        writeln!(out, "{address}")?;
        return Ok(());
    }
    let mut browser = kestrel_platform::open_url(kestrel_platform::Os::current(), address);
    let status = browser.status()?;
    if !status.success() {
        return Err(format!(
            "{} failed ({status}); open this address: {address}",
            browser.get_program().to_string_lossy()
        )
        .into());
    }
    writeln!(out, "opened Kestrel in your browser")?;
    Ok(())
}

/// Writes `text` to `path`, readable by this user only. Windows: the file lives in the user's own
/// %LOCALAPPDATA% (kestrel-platform), which the profile's permissions keep private.
#[cfg(not(unix))]
pub fn write_private(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)
}

/// Writes `text` to `path`, readable by this user only.
#[cfg(unix)]
pub fn write_private(path: &Path, text: &str) -> io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(text.as_bytes())?;
    // An existing file keeps its mode through open(): make it private either way.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// The page's secret: read from `path`, or made (32 secure random bytes as hex, kestrel-platform)
/// and saved private.
pub fn token(path: &Path) -> io::Result<String> {
    if let Ok(t) = std::fs::read_to_string(path) {
        let t = t.trim();
        if t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(t.to_string());
        }
    }
    let bytes = kestrel_platform::random_bytes(kestrel_platform::Os::current(), 32)?;
    let t: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_private(path, &t)?;
    Ok(t)
}

/// What `GET /api/tasks` answers.
pub fn tasks_json(
    tasks: &[TaskRow],
    today: Date,
    minute: u32,
    skills: bool,
    name: Option<&str>,
) -> Value {
    let rows: Vec<Value> = tasks
        .iter()
        .map(|t| json!({"id": t.id, "title": t.title, "due": t.due, "time": t.due_time}))
        .collect();
    json!({"today": today.to_string(), "minute": minute, "skills": skills, "name": name, "tasks": rows})
}

/// The browser side of `kestrel serve`: a listener on 127.0.0.1 polled from the service loop; shared
/// on the wifi (`lan`), on every address, answering to that one too.
pub struct Web {
    listener: std::net::TcpListener,
    port: u16,
    token: String,
    tasks: Box<dyn Fn() -> Value>,
    capabilities: Box<dyn Fn() -> Value>,
    events: Box<dyn Fn(u64) -> Value>,
    settings: Box<Settings>,
    notes: Box<NotesReader>,
    lan: Option<std::net::Ipv4Addr>,
}

/// Reads the settings (None) or changes them (Some(change)); the settings after, or why not.
pub type Settings = dyn Fn(Option<&Value>) -> Result<Value, String>;
/// Lists the notes (None) or reads one by its listed name; or why not (none such: 404).
pub type NotesReader = dyn Fn(Option<&str>) -> Result<Value, String>;

/// What the API reads and changes beside the requests themselves.
pub struct Api<'a> {
    pub tasks: &'a dyn Fn() -> Value,
    pub capabilities: &'a dyn Fn() -> Value,
    pub events: &'a dyn Fn(u64) -> Value,
    pub settings: &'a Settings,
    pub notes: &'a NotesReader,
}

#[cfg(test)]
impl Api<'static> {
    /// Answers null for each read and refuses each change (a page without a service behind it).
    pub fn none() -> Self {
        Api {
            tasks: &|| Value::Null,
            capabilities: &|| Value::Null,
            events: &|_| Value::Null,
            settings: &|_| Err("no settings here".into()),
            notes: &|_| Err("no notes here".into()),
        }
    }
}

impl Web {
    /// Listens on 127.0.0.1:`port` (0: any free port), or on every address when shared on the wifi
    /// at `lan`; `tasks` answers `GET /api/tasks`.
    pub fn bind(
        port: u16,
        token: String,
        tasks: Box<dyn Fn() -> Value>,
        lan: Option<std::net::Ipv4Addr>,
    ) -> io::Result<Self> {
        let host = if lan.is_some() {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        };
        let listener = std::net::TcpListener::bind((host, port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        Ok(Self {
            listener,
            port,
            token,
            tasks,
            capabilities: Box::new(|| Value::Null),
            events: Box::new(|_| Value::Null),
            settings: Box::new(|_| Err("no settings here".into())),
            notes: Box::new(|_| Err("no notes here".into())),
            lan,
        })
    }

    /// `settings` answers `GET` / `POST /api/settings` (v4).
    pub fn with_settings(mut self, settings: Box<Settings>) -> Self {
        self.settings = settings;
        self
    }

    /// `notes` answers `GET /api/notes` and `POST /api/notes/read` (v4).
    pub fn with_notes(mut self, notes: Box<NotesReader>) -> Self {
        self.notes = notes;
        self
    }

    fn api(&self) -> Api<'_> {
        Api {
            tasks: &*self.tasks,
            capabilities: &*self.capabilities,
            events: &*self.events,
            settings: &*self.settings,
            notes: &*self.notes,
        }
    }

    /// `capabilities` answers `GET /api/capabilities` (v4 A2); without it the answer is null.
    pub fn with_capabilities(mut self, capabilities: Box<dyn Fn() -> Value>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// `events` answers `GET /api/events?after=N` (v4 A3); without it the answer is null.
    pub fn with_events(mut self, events: Box<dyn Fn(u64) -> Value>) -> Self {
        self.events = events;
        self
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Answers every browser request waiting now, through `say`; returns when none is left.
    pub fn poll(&mut self, say: &mut Streamer<'_>) -> io::Result<()> {
        loop {
            match self.listener.accept() {
                // One browser that goes away only ends its own request.
                Ok((stream, _)) => {
                    let _ = self.serve(stream, say);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    fn serve(&self, stream: std::net::TcpStream, say: &mut Streamer<'_>) -> io::Result<()> {
        let Some((req, mut out)) = self.read(stream)? else {
            return Ok(());
        };
        let mut answer = || self.wait_for_answer();
        let done = respond(&req, &mut out, &self.api(), say, &mut answer);
        finish(out);
        done
    }

    /// The request on `stream` if it is allowed (else it is refused here), and the stream to
    /// answer on.
    fn read(
        &self,
        mut stream: std::net::TcpStream,
    ) -> io::Result<Option<(HttpRequest, std::net::TcpStream)>> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        // Read through a borrow: one socket handle, closed once (no clone closed early).
        let req = read_request(&mut io::BufReader::new(&stream));
        let refused = match req {
            Err(_) => reply(&mut stream, "400 Bad Request", TEXT, b"bad request"),
            Ok(req) => match allowed(&req, self.port, &self.token, self.lan) {
                Ok(()) => return Ok(Some((req, stream))),
                Err(403) => reply(&mut stream, "403 Forbidden", TEXT, b"wrong host"),
                Err(_) => {
                    let why = b"open the page with `kestrel open`";
                    reply(&mut stream, "401 Unauthorized", TEXT, why)
                }
            },
        };
        finish(stream);
        refused.map(|()| None)
    }

    /// While a question waits: its answer (Some), the page and the task list as usual, a new
    /// request turned away. None when no answer came within `ANSWER_WAIT`.
    fn wait_for_answer(&self) -> Option<bool> {
        let until = std::time::Instant::now() + ANSWER_WAIT;
        while std::time::Instant::now() < until {
            let stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
                Err(_) => return None,
            };
            let Ok(Some((req, mut out))) = self.read(stream) else {
                continue;
            };
            let mut answered = None;
            match (req.method.as_str(), req.path.as_str()) {
                ("POST", "/api/answer") => {
                    answered = serde_json::from_slice::<Value>(&req.body)
                        .ok()
                        .and_then(|v| v["answer"].as_bool());
                    let status = if answered.is_some() {
                        "204 No Content"
                    } else {
                        "400 Bad Request"
                    };
                    let _ = reply(&mut out, status, TEXT, b"");
                }
                ("POST", "/api/say") => {
                    let why = b"Kestrel is waiting for your answer to its question";
                    let _ = reply(&mut out, "409 Conflict", TEXT, why);
                }
                _ => {
                    let _ = respond(
                        &req,
                        &mut out,
                        &self.api(),
                        &mut |_, _, _| vec![],
                        &mut || None,
                    );
                }
            }
            finish(out);
            if answered.is_some() {
                return answered;
            }
        }
        None
    }
}

/// Answers one allowed request. `api` reads the task list, what Kestrel can do on this device, the
/// app's events, the settings and the notes; `say` handles a request (its y/N questions and
/// written parts go to the page), `answer` waits for the page's answer to one.
pub fn respond(
    req: &HttpRequest,
    out: &mut dyn Write,
    api: &Api,
    say: &mut Streamer<'_>,
    answer: &mut dyn FnMut() -> Option<bool>,
) -> io::Result<()> {
    let (path, query) = req.path.split_once('?').unwrap_or((&req.path, ""));
    match (req.method.as_str(), path) {
        ("GET", "/") => reply(out, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/api/tasks") => reply(out, "200 OK", JSON, (api.tasks)().to_string().as_bytes()),
        ("GET", font) if font.starts_with("/fonts/") => {
            match FONTS.iter().find(|(p, _)| *p == font) {
                Some((_, bytes)) => reply(out, "200 OK", "font/woff2", bytes),
                None => reply(out, "404 Not Found", TEXT, b"not found"),
            }
        }
        ("GET", "/api/capabilities") => reply(
            out,
            "200 OK",
            JSON,
            (api.capabilities)().to_string().as_bytes(),
        ),
        ("GET", "/api/events") => match crate::events::after_in(query) {
            Some(after) => reply(
                out,
                "200 OK",
                JSON,
                (api.events)(after).to_string().as_bytes(),
            ),
            None => reply(out, "400 Bad Request", TEXT, b"after=N, N a number"),
        },
        ("GET", "/api/settings") => {
            answered(out, (api.settings)(None), "500 Internal Server Error")
        }
        ("POST", "/api/settings") => match serde_json::from_slice::<Value>(&req.body) {
            Ok(change) => answered(out, (api.settings)(Some(&change)), "400 Bad Request"),
            Err(_) => reply(out, "400 Bad Request", TEXT, b"send {\"name\": \"...\"}"),
        },
        ("GET", "/api/notes") => answered(out, (api.notes)(None), "500 Internal Server Error"),
        ("POST", "/api/notes/read") => {
            let body = serde_json::from_slice::<Value>(&req.body).ok();
            match body.as_ref().and_then(|v| v["name"].as_str()) {
                Some(name) => answered(out, (api.notes)(Some(name)), "404 Not Found"),
                None => reply(out, "400 Bad Request", TEXT, b"send {\"name\": \"...\"}"),
            }
        }
        ("POST", "/api/say") => {
            let text = serde_json::from_slice::<Value>(&req.body)
                .ok()
                .and_then(|v| v["text"].as_str().map(str::trim).map(String::from))
                .filter(|t| !t.is_empty());
            let Some(text) = text else {
                return reply(out, "400 Bad Request", TEXT, b"send {\"text\": \"...\"}");
            };
            head(out, "200 OK", "application/x-ndjson", None)?;
            let out = RefCell::new(out);
            let lines = {
                // A question goes to the page; no answer in time counts as no.
                let mut confirm = |q: &str| -> bool {
                    let mut o = out.borrow_mut();
                    let sent = writeln!(o, "{}", json!({"ask": q})).and_then(|()| o.flush());
                    drop(o);
                    sent.is_ok() && answer().unwrap_or(false)
                };
                // Written text is sent as it comes; a page gone mid-answer only loses its answer.
                let mut part = |p: &str| {
                    let mut o = out.borrow_mut();
                    let _ = writeln!(o, "{}", json!({"part": p})).and_then(|()| o.flush());
                };
                say(&text, &mut confirm, &mut part)
            };
            let out = out.into_inner();
            for l in lines {
                writeln!(out, "{}", json!({"line": l}))?;
            }
            writeln!(out, "{}", json!({"done": true}))?;
            out.flush()
        }
        ("POST", "/api/answer") => reply(out, "409 Conflict", TEXT, b"no question is waiting"),
        (
            _,
            "/" | "/api/tasks" | "/api/capabilities" | "/api/events" | "/api/settings"
            | "/api/notes" | "/api/notes/read" | "/api/say" | "/api/answer",
        ) => reply(out, "405 Method Not Allowed", TEXT, b"method not allowed"),
        _ => reply(out, "404 Not Found", TEXT, b"not found"),
    }
}

/// A JSON answer, or the reason it could not be given with `refused` as the status.
fn answered(out: &mut dyn Write, got: Result<Value, String>, refused: &str) -> io::Result<()> {
    match got {
        Ok(v) => reply(out, "200 OK", JSON, v.to_string().as_bytes()),
        Err(why) => reply(out, refused, TEXT, why.as_bytes()),
    }
}

/// What `GET /api/notes` answers: each note's name in the folder, a title (a first `# heading`, else
/// the file name without its date and extension), its date (from a `YYYY-MM-DD-` name) and the
/// start of its text.
pub fn notes_json(folder: &Path, notes: &[Note]) -> Value {
    let rows: Vec<Value> = notes
        .iter()
        .map(|n| {
            let (title, rest) = title_of(n);
            let mut preview = rest.split_whitespace().collect::<Vec<_>>().join(" ");
            if preview.chars().count() > PREVIEW {
                preview = preview.chars().take(PREVIEW).collect::<String>() + "…";
            }
            json!({"name": n.name, "title": title, "date": date_of(&n.name), "preview": preview})
        })
        .collect();
    json!({"folder": folder.display().to_string(), "notes": rows})
}

/// What `POST /api/notes/read` answers: the note's name, title, date and whole text.
pub fn note_json(n: &Note) -> Value {
    let (title, _) = title_of(n);
    json!({"name": n.name, "title": title, "date": date_of(&n.name), "text": n.text})
}

/// The longest preview of a note in the list, in characters.
const PREVIEW: usize = 180;

/// (title, the text after it).
fn title_of(n: &Note) -> (String, &str) {
    let text = n.text.trim_start();
    if let Some(h) = text.strip_prefix('#') {
        let (line, rest) = h.split_once('\n').unwrap_or((h, ""));
        let line = line.trim_start_matches('#').trim();
        if !line.is_empty() {
            return (line.to_string(), rest);
        }
    }
    let file = n.name.rsplit('/').next().unwrap_or(&n.name);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let stem = match date_of(&n.name) {
        Some(_) => stem.get(11..).unwrap_or(stem),
        None => stem,
    };
    (stem.replace(['-', '_'], " ").trim().to_string(), text)
}

/// The date in a `YYYY-MM-DD-...` file name.
fn date_of(name: &str) -> Option<String> {
    let file = name.rsplit('/').next().unwrap_or(name);
    let d = file.get(..10)?;
    (Date::parse(d).is_some() && file.get(10..11) == Some("-")).then(|| d.to_string())
}

/// The status line and headers; with `len`, a Content-Length (else the body ends at close).
fn head(out: &mut dyn Write, status: &str, kind: &str, len: Option<usize>) -> io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
         Content-Security-Policy: {CSP}\r\nConnection: close\r\n"
    )?;
    if let Some(n) = len {
        write!(out, "Content-Length: {n}\r\n")?;
    }
    write!(out, "\r\n")
}

/// Ends an answered connection gently: the end of the answer is sent first, then what the browser
/// still sends is read away until it closes or `LINGER` passes. A socket closed with bytes unread
/// is reset instead, and the browser can lose the end of the answer (seen on Windows CI).
fn finish(mut stream: std::net::TcpStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let until = std::time::Instant::now() + LINGER;
    let mut sink = [0u8; 4096];
    loop {
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return;
        }
        if matches!(stream.read(&mut sink), Ok(0) | Err(_)) {
            return;
        }
    }
}

/// A whole response.
pub fn reply(out: &mut dyn Write, status: &str, kind: &str, body: &[u8]) -> io::Result<()> {
    head(out, status, kind, Some(body.len()))?;
    out.write_all(body)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(raw: &str) -> HttpRequest {
        read_request(&mut io::BufReader::new(raw.as_bytes())).unwrap()
    }

    fn post(path: &str, body: &str) -> HttpRequest {
        req(&format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:7811\r\nX-Kestrel-Token: s3cret\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ))
    }

    fn get(path: &str) -> HttpRequest {
        req(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:7811\r\nX-Kestrel-Token: s3cret\r\n\r\n"
        ))
    }

    #[test]
    fn a_request_is_read_with_its_headers_and_body() {
        let r = post("/api/say", r#"{"text":"mere tasks btao"}"#);
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/api/say"));
        assert_eq!(
            r.header("x-kestrel-token"),
            Some("s3cret"),
            "names are lowercased"
        );
        assert_eq!(r.body, br#"{"text":"mere tasks btao"}"#);
        for bad in [
            "",
            "garbage\r\n\r\n",
            &format!(
                "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                MAX_BODY + 1
            ),
        ] {
            assert!(
                read_request(&mut io::BufReader::new(bad.as_bytes())).is_err(),
                "{bad:.40}"
            );
        }
    }

    #[test]
    fn only_this_server_by_name_and_the_api_only_with_the_token() {
        let with = |host: &str, token: Option<&str>, path: &str| {
            let t = token.map_or(String::new(), |t| format!("X-Kestrel-Token: {t}\r\n"));
            req(&format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{t}\r\n"))
        };
        assert_eq!(
            allowed(&with("127.0.0.1:7811", None, "/"), 7811, "s3cret", None),
            Ok(())
        );
        assert_eq!(
            allowed(&with("localhost:7811", None, "/"), 7811, "s3cret", None),
            Ok(())
        );
        // A name that resolves here but is not this server (DNS rebinding).
        assert_eq!(
            allowed(&with("evil.example:7811", None, "/"), 7811, "s3cret", None),
            Err(403)
        );
        assert_eq!(
            allowed(&with("127.0.0.1:9999", None, "/"), 7811, "s3cret", None),
            Err(403)
        );
        let api = "/api/tasks";
        assert_eq!(
            allowed(&with("127.0.0.1:7811", None, api), 7811, "s3cret", None),
            Err(401)
        );
        assert_eq!(
            allowed(
                &with("127.0.0.1:7811", None, "/api/capabilities"),
                7811,
                "s3cret",
                None
            ),
            Err(401),
            "the device's details need the key too"
        );
        assert_eq!(
            allowed(
                &with("127.0.0.1:7811", Some("guess"), api),
                7811,
                "s3cret",
                None
            ),
            Err(401)
        );
        assert_eq!(
            allowed(
                &with("127.0.0.1:7811", Some("s3cret"), api),
                7811,
                "s3cret",
                None
            ),
            Ok(())
        );
    }

    #[test]
    fn the_wifi_address_counts_only_when_the_page_is_shared_there() {
        let with = |host: &str, token: Option<&str>, path: &str| {
            let t = token.map_or(String::new(), |t| format!("X-Kestrel-Token: {t}\r\n"));
            req(&format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{t}\r\n"))
        };
        let lan = Some(std::net::Ipv4Addr::new(192, 168, 251, 140));
        let page = with("192.168.251.140:7811", None, "/");
        assert_eq!(allowed(&page, 7811, "s3cret", None), Err(403));
        assert_eq!(allowed(&page, 7811, "s3cret", lan), Ok(()));
        assert_eq!(
            allowed(&with("127.0.0.1:7811", None, "/"), 7811, "s3cret", lan),
            Ok(())
        );
        assert_eq!(
            allowed(&with("192.168.251.9:7811", None, "/"), 7811, "s3cret", lan),
            Err(403)
        );
        let api = |token| with("192.168.251.140:7811", token, "/api/tasks");
        assert_eq!(
            allowed(&api(None), 7811, "s3cret", lan),
            Err(401),
            "the key, still"
        );
        assert_eq!(allowed(&api(Some("s3cret")), 7811, "s3cret", lan), Ok(()));
    }

    #[test]
    fn shared_on_the_wifi_the_page_listens_everywhere_and_open_gives_that_link() {
        let lan = std::net::Ipv4Addr::new(192, 168, 251, 140);
        let w = Web::bind(0, "t".into(), Box::new(|| Value::Null), Some(lan)).unwrap();
        assert!(w.listener.local_addr().unwrap().ip().is_unspecified());
        let w = Web::bind(0, "t".into(), Box::new(|| Value::Null), None).unwrap();
        assert!(w.listener.local_addr().unwrap().ip().is_loopback());
        assert_eq!(
            lan_url(lan, 7811, "ab12"),
            "http://192.168.251.140:7811/#t=ab12"
        );
        let dir = std::env::temp_dir().join(format!("kestrel_web_lan_{}", std::process::id()));
        let var = runtime_in(&dir);
        assert_eq!(
            lan_url_file(&var),
            url_file(&var).with_extension("lan.url"),
            "next to the page's address"
        );
        std::fs::write(url_file(&var), url(7811, "ab12")).unwrap();
        let mut out = Vec::new();
        let e = open(&["--print".into(), "--lan".into()], &var, &mut out).unwrap_err();
        assert!(e.to_string().contains("not shared"), "{e}");
        std::fs::write(lan_url_file(&var), lan_url(lan, 7811, "ab12")).unwrap();
        open(&["--print".into(), "--lan".into()], &var, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "http://192.168.251.140:7811/#t=ab12\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_token_is_made_once_and_kept_private() {
        let p = std::env::temp_dir().join(format!("kestrel_web_token_{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let t = token(&p).unwrap();
        assert_eq!(t.len(), 64);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(token(&p).unwrap(), t, "the same token after a restart");
        let _ = std::fs::remove_file(&p);
    }

    fn row(id: i64, title: &str, due: Option<&str>, time: Option<&str>) -> TaskRow {
        TaskRow {
            id,
            title: title.into(),
            due: due.map(Into::into),
            created_at: 0,
            due_time: time.map(Into::into),
            notified_at: None,
        }
    }

    #[test]
    fn the_tasks_come_with_today_and_the_minute_now() {
        let today = Date::parse("2026-10-25").unwrap();
        let v = tasks_json(
            &[
                row(6, "bill bharna", Some("2026-10-25"), Some("18:00")),
                row(1, "fix the flaky build", None, None),
            ],
            today,
            1180,
            false,
            Some("Aditya"),
        );
        assert_eq!(
            v,
            json!({
                "today": "2026-10-25",
                "minute": 1180,
                "skills": false,
                "name": "Aditya",
                "tasks": [
                    {"id": 6, "title": "bill bharna", "due": "2026-10-25", "time": "18:00"},
                    {"id": 1, "title": "fix the flaky build", "due": null, "time": null}
                ]
            })
        );
        // With v3's skills on, the page says so (and offers one to try).
        assert_eq!(tasks_json(&[], today, 0, true, None)["skills"], true);
        // No name saved (`kestrel settings name NAME`): the page greets without one.
        assert_eq!(tasks_json(&[], today, 0, true, None)["name"], Value::Null);
    }

    /// Runs `respond` with a stand-in assistant; the response as text.
    fn answer_to(r: &HttpRequest, page_says: Option<bool>) -> (String, Vec<String>) {
        let mut heard = Vec::new();
        let mut out = Vec::new();
        let mut say = |text: &str,
                       confirm: &mut dyn FnMut(&str) -> bool,
                       part: &mut dyn FnMut(&str)|
         -> Vec<String> {
            heard.push(text.to_string());
            if text == "explain recursion" {
                part("Recursion is");
                part(" a function calling itself.");
                return vec!["(note)".into()];
            }
            if text == "delete task 2" {
                let yes = confirm("delete task 2?");
                return vec![
                    if yes {
                        "deleted task #2"
                    } else {
                        "kept task #2 (you said no)"
                    }
                    .into(),
                ];
            }
            vec!["no tasks".into()]
        };
        let settings = |change: Option<&Value>| match change {
            None => Ok(json!({"name": "Aditya"})),
            Some(c) if c["name"] == "" => Err("a name of 1 to 40 letters".to_string()),
            Some(c) => Ok(json!({"name": c["name"]})),
        };
        let notes = |name: Option<&str>| match name {
            None => Ok(json!({"notes": []})),
            Some("a.md") => Ok(json!({"name": "a.md"})),
            Some(n) => Err(format!("no note named {n}")),
        };
        let api = Api {
            tasks: &|| json!({"tasks": []}),
            capabilities: &|| json!({"cards": ["x"]}),
            events: &|after| json!({"after": after}),
            settings: &settings,
            notes: &notes,
        };
        respond(r, &mut out, &api, &mut say, &mut || page_says).unwrap();
        (String::from_utf8(out).unwrap(), heard)
    }

    #[test]
    fn notes_are_listed_with_a_title_a_date_and_a_preview() {
        let notes = [
            Note {
                name: "2026-10-05-budget-plan.md".into(),
                text: "# Budget plan\n\nMeeting on   Friday.\nRavi shares the plan.".into(),
            },
            Note {
                name: "ideas/desk.txt".into(),
                text: "buy a desk".into(),
            },
        ];
        assert_eq!(
            notes_json(std::path::Path::new("/home/me/notes"), &notes),
            json!({"folder": "/home/me/notes", "notes": [
                {"name": "2026-10-05-budget-plan.md", "title": "Budget plan", "date": "2026-10-05",
                 "preview": "Meeting on Friday. Ravi shares the plan."},
                {"name": "ideas/desk.txt", "title": "desk", "date": null, "preview": "buy a desk"}
            ]})
        );
        let one = note_json(&notes[0]);
        assert_eq!(one["title"], "Budget plan");
        assert_eq!(one["text"], notes[0].text);
    }

    #[test]
    fn settings_and_notes_have_their_routes() {
        let (out, _) = answer_to(&get("/api/settings"), None);
        assert!(out.ends_with("{\"name\":\"Aditya\"}"), "{out}");
        let (out, _) = answer_to(&post("/api/settings", r#"{"name":"Asha"}"#), None);
        assert!(out.starts_with("HTTP/1.1 200 OK"), "{out:.80}");
        assert!(out.ends_with("{\"name\":\"Asha\"}"), "{out}");
        let (out, _) = answer_to(&post("/api/settings", r#"{"name":""}"#), None);
        assert!(out.starts_with("HTTP/1.1 400"), "{out:.80}");
        let (out, _) = answer_to(&get("/api/notes"), None);
        assert!(out.ends_with("{\"notes\":[]}"), "{out}");
        let (out, _) = answer_to(&post("/api/notes/read", r#"{"name":"a.md"}"#), None);
        assert!(out.ends_with("{\"name\":\"a.md\"}"), "{out}");
        let (out, _) = answer_to(&post("/api/notes/read", r#"{"name":"nope.md"}"#), None);
        assert!(out.starts_with("HTTP/1.1 404"), "{out:.80}");
        let (out, _) = answer_to(&post("/api/notes", "{}"), None);
        assert!(out.starts_with("HTTP/1.1 405"), "{out:.80}");
    }

    #[test]
    fn the_events_come_after_the_number_asked_and_only_by_get() {
        let (out, _) = answer_to(&get("/api/events?after=4"), None);
        assert!(out.starts_with("HTTP/1.1 200 OK\r\n"), "{out:.80}");
        assert!(out.ends_with("{\"after\":4}"), "{out}");
        let (out, _) = answer_to(&get("/api/events"), None);
        assert!(out.ends_with("{\"after\":0}"), "{out}");
        let (out, _) = answer_to(&get("/api/events?after=soon"), None);
        assert!(out.starts_with("HTTP/1.1 400"), "{out:.80}");
        let (out, _) = answer_to(&post("/api/events", "{}"), None);
        assert!(out.starts_with("HTTP/1.1 405"), "{out:.80}");
    }

    #[test]
    fn the_capabilities_come_from_their_reader_and_only_by_get() {
        let (out, _) = answer_to(&get("/api/capabilities"), None);
        assert!(out.starts_with("HTTP/1.1 200 OK\r\n"), "{out:.80}");
        assert!(out.ends_with("{\"cards\":[\"x\"]}"), "{out}");
        let (out, _) = answer_to(&post("/api/capabilities", "{}"), None);
        assert!(out.starts_with("HTTP/1.1 405"), "{out:.80}");
    }

    #[test]
    fn the_page_is_served_and_may_load_nothing_from_elsewhere() {
        let (out, _) = answer_to(&get("/"), None);
        assert!(out.starts_with("HTTP/1.1 200 OK\r\n"), "{out:.80}");
        assert!(out.contains("Content-Type: text/html; charset=utf-8"));
        let csp = out
            .lines()
            .find(|l| l.starts_with("Content-Security-Policy: "))
            .expect("a CSP header");
        assert!(
            csp.contains("default-src 'none'") && csp.contains("connect-src 'self'"),
            "{csp}"
        );
        // Its fonts come from this server only (apps/cli/web/fonts, nothing fetched elsewhere).
        assert!(csp.contains("font-src 'self'"), "{csp}");
        assert!(out.contains("<title>Kestrel</title>"));
    }

    #[test]
    fn the_pages_fonts_are_served_without_the_key_and_only_these() {
        let r = read_request(&mut io::BufReader::new(
            "GET /fonts/inter.woff2 HTTP/1.1\r\nHost: 127.0.0.1:7811\r\n\r\n".as_bytes(),
        ))
        .unwrap();
        assert_eq!(allowed(&r, 7811, "k", None), Ok(()));
        for f in ["source-serif-4", "inter", "caveat"] {
            let mut out = Vec::new();
            let req = get(&format!("/fonts/{f}.woff2"));
            respond(
                &req,
                &mut out,
                &Api::none(),
                &mut |_, _, _| vec![],
                &mut || None,
            )
            .unwrap();
            let head = String::from_utf8_lossy(&out[..out.len().min(400)]).into_owned();
            assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{f}: {head:.80}");
            assert!(head.contains("Content-Type: font/woff2"), "{f}");
            let body = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            assert_eq!(&out[body..body + 4], b"wOF2", "{f}");
        }
        let (out, _) = answer_to(&get("/fonts/other.woff2"), None);
        assert!(out.starts_with("HTTP/1.1 404"), "{out:.80}");
    }

    #[test]
    fn a_request_is_answered_line_by_line() {
        let (out, heard) = answer_to(&post("/api/say", r#"{"text":"mere tasks btao"}"#), None);
        assert_eq!(heard, ["mere tasks btao"]);
        let body = out.split("\r\n\r\n").nth(1).unwrap();
        assert_eq!(body, "{\"line\":\"no tasks\"}\n{\"done\":true}\n");
        assert!(out.contains("Content-Type: application/x-ndjson"));
    }

    #[test]
    fn a_written_answer_goes_to_the_page_in_parts() {
        let (out, _) = answer_to(&post("/api/say", r#"{"text":"explain recursion"}"#), None);
        let body = out.split("\r\n\r\n").nth(1).unwrap();
        assert_eq!(
            body,
            "{\"part\":\"Recursion is\"}\n{\"part\":\" a function calling itself.\"}\n\
             {\"line\":\"(note)\"}\n{\"done\":true}\n"
        );
    }

    #[test]
    fn a_question_goes_to_the_page_and_its_answer_comes_back() {
        let (out, _) = answer_to(&post("/api/say", r#"{"text":"delete task 2"}"#), Some(true));
        let body = out.split("\r\n\r\n").nth(1).unwrap();
        assert_eq!(
            body,
            "{\"ask\":\"delete task 2?\"}\n{\"line\":\"deleted task #2\"}\n{\"done\":true}\n"
        );
        // No answer in time counts as no.
        let (out, _) = answer_to(&post("/api/say", r#"{"text":"delete task 2"}"#), None);
        assert!(out.contains("kept task #2 (you said no)"), "{out}");
    }

    #[test]
    fn tasks_unknown_paths_and_bad_bodies() {
        let (out, _) = answer_to(&get("/api/tasks"), None);
        assert!(
            out.starts_with("HTTP/1.1 200 OK") && out.ends_with("{\"tasks\":[]}"),
            "{out}"
        );
        let (out, _) = answer_to(&get("/nope"), None);
        assert!(out.starts_with("HTTP/1.1 404"), "{out:.40}");
        let (out, heard) = answer_to(&post("/api/say", "not json"), None);
        assert!(
            out.starts_with("HTTP/1.1 400") && heard.is_empty(),
            "{out:.40}"
        );
        let (out, _) = answer_to(&post("/api/answer", r#"{"answer":true}"#), None);
        assert!(
            out.starts_with("HTTP/1.1 409"),
            "no question is waiting: {out:.40}"
        );
    }

    #[test]
    fn the_page_answers_a_question_in_a_second_request() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpStream;

        let mut web =
            Web::bind(0, "s3cret".into(), Box::new(|| json!({"tasks": []})), None).unwrap();
        let port = web.port();
        let send = move |path: &str, body: &str| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                s,
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Kestrel-Token: s3cret\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            s
        };
        let browser = std::thread::spawn(move || {
            let mut said = BufReader::new(send("/api/say", r#"{"text":"delete task 2"}"#));
            let mut line = String::new();
            while !line.starts_with("{\"ask\"") {
                line.clear();
                said.read_line(&mut line).unwrap();
            }
            let asked = line.clone();
            // While the question waits, the task list still loads; a new request is turned away.
            let mut busy = String::new();
            send("/api/say", r#"{"text":"show my tasks"}"#)
                .read_to_string(&mut busy)
                .unwrap();
            let mut answered = String::new();
            send("/api/answer", r#"{"answer":true}"#)
                .read_to_string(&mut answered)
                .unwrap();
            let mut rest = String::new();
            said.read_to_string(&mut rest).unwrap();
            (asked, busy, answered, rest)
        });
        let mut say = |text: &str,
                       confirm: &mut dyn FnMut(&str) -> bool,
                       _: &mut dyn FnMut(&str)|
         -> Vec<String> {
            assert_eq!(text, "delete task 2");
            vec![
                if confirm("delete task 2?") {
                    "deleted task #2"
                } else {
                    "kept"
                }
                .into(),
            ]
        };
        while !browser.is_finished() {
            web.poll(&mut say).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (asked, busy, answered, rest) = browser.join().unwrap();
        assert_eq!(asked, "{\"ask\":\"delete task 2?\"}\n");
        assert!(busy.starts_with("HTTP/1.1 409"), "{busy:.60}");
        assert!(answered.starts_with("HTTP/1.1 204"), "{answered:.60}");
        assert_eq!(rest, "{\"line\":\"deleted task #2\"}\n{\"done\":true}\n");
    }

    #[test]
    fn the_whole_answer_arrives_when_the_browser_sent_more_than_its_request() {
        // A connection closed with bytes unread is reset (Linux, Windows), and the browser can lose
        // the end of the answer. More than the request's read buffer, so some stay in the socket.
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let mut web =
            Web::bind(0, "s3cret".into(), Box::new(|| json!({"tasks": []})), None).unwrap();
        let port = web.port();
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            s,
            "GET /api/tasks HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
        )
        .unwrap();
        s.write_all(&[b'x'; 64 * 1024]).unwrap();
        // The service answers (401: no token) and is done with the connection before the read.
        std::thread::sleep(std::time::Duration::from_millis(100));
        web.poll(&mut |_, _, _| vec![]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut answer = String::new();
        s.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 401"), "{answer:.60}");
        assert!(
            answer.ends_with("open the page with `kestrel open`"),
            "{answer}"
        );
    }

    /// An environment whose runtime folder (on any of the three systems) is `dir`, created.
    fn runtime_in(dir: &Path) -> impl Fn(&str) -> Option<std::ffi::OsString> + use<> {
        let dir = dir.to_path_buf();
        let var = move |k: &str| {
            matches!(k, "XDG_RUNTIME_DIR" | "TMPDIR" | "LOCALAPPDATA")
                .then(|| dir.clone().into_os_string())
        };
        std::fs::create_dir_all(url_file(&var).parent().unwrap()).unwrap();
        var
    }

    #[test]
    fn a_page_address_names_its_port() {
        assert_eq!(port_of("http://127.0.0.1:7811/#t=ab12"), Some(7811));
        assert_eq!(port_of("http://192.168.251.140:7811/#t=ab12\n"), Some(7811));
        assert_eq!(port_of("http://127.0.0.1/"), None);
        assert_eq!(port_of("https://127.0.0.1:7811/"), None);
        assert_eq!(port_of(""), None);
    }

    #[test]
    fn a_page_that_answers_is_found_and_a_stale_address_is_not() {
        let dir = std::env::temp_dir().join(format!("kestrel_web_alive_{}", std::process::id()));
        let var = runtime_in(&dir);
        let _ = std::fs::remove_file(url_file(&var));
        assert!(!page_answers(&var), "no address at all");
        let live = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = live.local_addr().unwrap().port();
        std::fs::write(url_file(&var), url(port, "ab12")).unwrap();
        assert!(page_answers(&var));
        drop(live);
        assert!(
            !page_answers(&var),
            "the service stopped and left its address"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_finds_the_address_the_service_left() {
        let dir = std::env::temp_dir().join(format!("kestrel_web_open_{}", std::process::id()));
        let var = runtime_in(&dir);
        assert!(
            url_file(&var).starts_with(&dir),
            "{}",
            url_file(&var).display()
        );
        if kestrel_platform::Os::current() == kestrel_platform::Os::Linux {
            assert_eq!(url_file(&var), dir.join("kestrel.url"));
        }
        assert_eq!(url(7811, "ab12"), "http://127.0.0.1:7811/#t=ab12");
        let _ = std::fs::remove_file(url_file(&var));
        let mut out = Vec::new();
        let e = open(&["--print".into()], &var, &mut out).unwrap_err();
        assert!(e.to_string().contains("not running"), "{e}");
        std::fs::write(url_file(&var), url(7811, "ab12")).unwrap();
        open(&["--print".into()], &var, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "http://127.0.0.1:7811/#t=ab12\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
