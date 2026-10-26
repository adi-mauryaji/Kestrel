//! The browser page (ADR-034 (4)): `kestrel serve` also answers HTTP on 127.0.0.1, with the
//! standard library only, from the same `Assistant` as `kestrel chat`. `kestrel open` opens it.
//!
//! - `GET /` is the page (apps/cli/web/page.html, one file, nothing fetched from elsewhere: its
//!   Content-Security-Policy allows only this server).
//! - `GET /api/tasks`: the tasks, today's date, the minute now and whether v3's skills are on
//!   (the page then names them). `POST /api/say {"text"}`: the
//!   answer as JSON lines, `{"part"}` ... while a skill's text is written (v3), `{"line"}` ...,
//!   `{"ask"}` for a y/N question, `{"done": true}`; the page answers a question with
//!   `POST /api/answer {"answer": bool}`, served while it waits.
//! - Who may ask: every request must name this server in `Host` (a web page elsewhere cannot
//!   reach it by DNS rebinding); the API also needs `X-Kestrel-Token`, a secret kept in a 0600
//!   file that `kestrel open` passes in the URL fragment (never sent to a server). The page itself
//!   holds no secret.
//! - One request at a time, like the Unix socket; a read stalls at most `READ_TIMEOUT`.

use std::cell::RefCell;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;

use kestrel_memory::TaskRow;
use kestrel_tools::date::Date;
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
img-src data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const JSON: &str = "application/json";
/// A browser that stops sending holds the service at most this long.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
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
/// request without the token.
pub fn allowed(req: &HttpRequest, port: u16, token: &str) -> Result<(), u16> {
    let host = req.header("host").unwrap_or("");
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
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

/// The page's address; the token rides in the fragment, which the browser never sends.
pub fn url(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/#t={token}")
}

/// `kestrel open [--print]`: opens the running Kestrel's page in the browser (or prints its
/// address).
pub fn open(
    args: &[String],
    var: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    out: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = url_file(var);
    let address = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "Kestrel is not running ({} is missing): start it with scripts/install_service.sh",
            path.display()
        )
    })?;
    let address = address.trim();
    if args.iter().any(|a| a == "--print") {
        writeln!(out, "{address}")?;
        return Ok(());
    }
    let status = std::process::Command::new("xdg-open")
        .arg(address)
        .status()?;
    if !status.success() {
        return Err(format!("xdg-open failed ({status}); open this address: {address}").into());
    }
    writeln!(out, "opened Kestrel in your browser")?;
    Ok(())
}

/// Writes `text` to `path`, readable by this user only.
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

/// The page's secret: read from `path`, or made (32 random bytes as hex) and saved 0600.
pub fn token(path: &Path) -> io::Result<String> {
    if let Ok(t) = std::fs::read_to_string(path) {
        let t = t.trim();
        if t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(t.to_string());
        }
    }
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let t: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_private(path, &t)?;
    Ok(t)
}

/// What `GET /api/tasks` answers.
pub fn tasks_json(tasks: &[TaskRow], today: Date, minute: u32, skills: bool) -> Value {
    let rows: Vec<Value> = tasks
        .iter()
        .map(|t| json!({"id": t.id, "title": t.title, "due": t.due, "time": t.due_time}))
        .collect();
    json!({"today": today.to_string(), "minute": minute, "skills": skills, "tasks": rows})
}

/// The browser side of `kestrel serve`: a listener on 127.0.0.1 polled from the service loop.
pub struct Web {
    listener: std::net::TcpListener,
    port: u16,
    token: String,
    tasks: Box<dyn Fn() -> Value>,
}

impl Web {
    /// Listens on 127.0.0.1:`port` (0: any free port); `tasks` answers `GET /api/tasks`.
    pub fn bind(port: u16, token: String, tasks: Box<dyn Fn() -> Value>) -> io::Result<Self> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        Ok(Self {
            listener,
            port,
            token,
            tasks,
        })
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
        respond(&req, &mut out, &*self.tasks, say, &mut answer)
    }

    /// The request on `stream` if it is allowed (else it is refused here), and the stream to
    /// answer on.
    fn read(
        &self,
        stream: std::net::TcpStream,
    ) -> io::Result<Option<(HttpRequest, std::net::TcpStream)>> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        let mut out = stream.try_clone()?;
        let req = match read_request(&mut io::BufReader::new(stream)) {
            Ok(r) => r,
            Err(_) => {
                reply(&mut out, "400 Bad Request", TEXT, b"bad request")?;
                return Ok(None);
            }
        };
        match allowed(&req, self.port, &self.token) {
            Ok(()) => Ok(Some((req, out))),
            Err(403) => reply(&mut out, "403 Forbidden", TEXT, b"wrong host").map(|()| None),
            Err(_) => {
                let why = b"open the page with `kestrel open`";
                reply(&mut out, "401 Unauthorized", TEXT, why).map(|()| None)
            }
        }
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
            match (req.method.as_str(), req.path.as_str()) {
                ("POST", "/api/answer") => {
                    let a = serde_json::from_slice::<Value>(&req.body)
                        .ok()
                        .and_then(|v| v["answer"].as_bool());
                    let status = if a.is_some() {
                        "204 No Content"
                    } else {
                        "400 Bad Request"
                    };
                    let _ = reply(&mut out, status, TEXT, b"");
                    if a.is_some() {
                        return a;
                    }
                }
                ("POST", "/api/say") => {
                    let why = b"Kestrel is waiting for your answer to its question";
                    let _ = reply(&mut out, "409 Conflict", TEXT, why);
                }
                _ => {
                    let _ = respond(
                        &req,
                        &mut out,
                        &*self.tasks,
                        &mut |_, _, _| vec![],
                        &mut || None,
                    );
                }
            }
        }
        None
    }
}

/// Answers one allowed request. `tasks` reads the task list, `say` handles a request (its y/N
/// questions and written parts go to the page), `answer` waits for the page's answer to one.
pub fn respond(
    req: &HttpRequest,
    out: &mut dyn Write,
    tasks: &dyn Fn() -> Value,
    say: &mut Streamer<'_>,
    answer: &mut dyn FnMut() -> Option<bool>,
) -> io::Result<()> {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => reply(out, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/api/tasks") => reply(out, "200 OK", JSON, tasks().to_string().as_bytes()),
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
        (_, "/" | "/api/tasks" | "/api/say" | "/api/answer") => {
            reply(out, "405 Method Not Allowed", TEXT, b"method not allowed")
        }
        _ => reply(out, "404 Not Found", TEXT, b"not found"),
    }
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
            allowed(&with("127.0.0.1:7811", None, "/"), 7811, "s3cret"),
            Ok(())
        );
        assert_eq!(
            allowed(&with("localhost:7811", None, "/"), 7811, "s3cret"),
            Ok(())
        );
        // A name that resolves here but is not this server (DNS rebinding).
        assert_eq!(
            allowed(&with("evil.example:7811", None, "/"), 7811, "s3cret"),
            Err(403)
        );
        assert_eq!(
            allowed(&with("127.0.0.1:9999", None, "/"), 7811, "s3cret"),
            Err(403)
        );
        let api = "/api/tasks";
        assert_eq!(
            allowed(&with("127.0.0.1:7811", None, api), 7811, "s3cret"),
            Err(401)
        );
        assert_eq!(
            allowed(&with("127.0.0.1:7811", Some("guess"), api), 7811, "s3cret"),
            Err(401)
        );
        assert_eq!(
            allowed(&with("127.0.0.1:7811", Some("s3cret"), api), 7811, "s3cret"),
            Ok(())
        );
    }

    #[test]
    fn the_token_is_made_once_and_kept_private() {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("kestrel_web_token_{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let t = token(&p).unwrap();
        assert_eq!(t.len(), 64);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
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
        );
        assert_eq!(
            v,
            json!({
                "today": "2026-10-25",
                "minute": 1180,
                "skills": false,
                "tasks": [
                    {"id": 6, "title": "bill bharna", "due": "2026-10-25", "time": "18:00"},
                    {"id": 1, "title": "fix the flaky build", "due": null, "time": null}
                ]
            })
        );
        // With v3's skills on, the page says so (and offers one to try).
        assert_eq!(tasks_json(&[], today, 0, true)["skills"], true);
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
        respond(r, &mut out, &|| json!({"tasks": []}), &mut say, &mut || {
            page_says
        })
        .unwrap();
        (String::from_utf8(out).unwrap(), heard)
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
        assert!(out.contains("<title>Kestrel</title>"));
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

        let mut web = Web::bind(0, "s3cret".into(), Box::new(|| json!({"tasks": []}))).unwrap();
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
    fn open_finds_the_address_the_service_left() {
        let dir = std::env::temp_dir().join(format!("kestrel_web_open_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let var = |k: &str| (k == "XDG_RUNTIME_DIR").then(|| dir.clone().into_os_string());
        assert_eq!(url_file(&var), dir.join("kestrel.url"));
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
