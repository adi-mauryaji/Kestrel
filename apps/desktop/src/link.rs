//! The shell's link to the running Kestrel service, std only and loopback only: the page's address,
//! the app's events (`GET /api/events`, apps/cli/src/events.rs) and where the window may go.

use serde_json::Value;

/// The running service's page: its port on 127.0.0.1 and the token its API asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub port: u16,
    pub token: String,
}

impl Page {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/#t={}", self.port, self.token)
    }
}

/// The page's address as `kestrel open --print` prints it ("http://127.0.0.1:7811/#t=<token>");
/// None for any other host.
pub fn page(address: &str) -> Option<Page> {
    let rest = address.trim().strip_prefix("http://127.0.0.1:")?;
    let (port, token) = rest.split_once("/#t=")?;
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(Page {
        port: port.parse().ok()?,
        token: token.into(),
    })
}

/// The request for the events numbered above `after`.
pub fn events_request(page: &Page, after: u64) -> String {
    format!(
        "GET /api/events?after={after} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\
         X-Kestrel-Token: {}\r\nConnection: close\r\n\r\n",
        page.port, page.token
    )
}

/// The body of a whole 200 response; None for any other status.
pub fn body(response: &str) -> Option<&str> {
    let (head, body) = response.split_once("\r\n\r\n")?;
    (head.split_whitespace().nth(1) == Some("200")).then_some(body)
}

/// From an events answer: the notifications to show (title, body) and the number to ask after
/// next. None when it is not an events answer. When the service shows them on the desktop itself,
/// nothing to show.
pub fn notifications(answer: &str) -> Option<(Vec<(String, String)>, u64)> {
    let v: Value = serde_json::from_str(answer).ok()?;
    let (events, last) = (v["events"].as_array()?, v["last"].as_u64()?);
    if v["desktop"].as_bool() == Some(true) {
        return Some((vec![], last));
    }
    let shown = events
        .iter()
        .filter_map(|e| Some((e["title"].as_str()?.into(), e["body"].as_str()?.into())))
        .collect();
    Some((shown, last))
}

/// The window may show the service's page and the shell's own page, nothing else.
pub fn may_open(url: &str, port: u16) -> bool {
    url.starts_with(&format!("http://127.0.0.1:{port}/"))
        || url.starts_with("tauri://localhost/")
        || url.starts_with("http://tauri.localhost/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn here() -> Page {
        Page {
            port: 7811,
            token: TOKEN.into(),
        }
    }

    #[test]
    fn the_page_is_read_from_kestrel_open_print_and_only_on_this_machine() {
        assert_eq!(
            page(&format!("http://127.0.0.1:7811/#t={TOKEN}\n")),
            Some(here())
        );
        assert_eq!(here().url(), format!("http://127.0.0.1:7811/#t={TOKEN}"));
        // The wifi address (`kestrel open --lan`), a missing or odd token: not for the shell.
        assert_eq!(page(&format!("http://192.168.1.20:7811/#t={TOKEN}")), None);
        assert_eq!(page("http://127.0.0.1:7811/"), None);
        assert_eq!(page("http://127.0.0.1:7811/#t=abc"), None);
        assert_eq!(page(&format!("http://127.0.0.1:x/#t={TOKEN}")), None);
    }

    #[test]
    fn the_events_are_asked_for_with_the_token_and_this_host() {
        assert_eq!(
            events_request(&here(), 4),
            format!(
                "GET /api/events?after=4 HTTP/1.1\r\nHost: 127.0.0.1:7811\r\n\
                 X-Kestrel-Token: {TOKEN}\r\nConnection: close\r\n\r\n"
            )
        );
    }

    #[test]
    fn only_a_200_has_a_body() {
        let ok = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        assert_eq!(body(ok), Some("{\"a\":1}"));
        assert_eq!(body("HTTP/1.1 401 Unauthorized\r\n\r\nopen the page"), None);
        assert_eq!(body("HTTP/1.1 200 OK\r\nno end of headers"), None);
    }

    #[test]
    fn each_event_becomes_a_notification_unless_the_desktop_shows_it() {
        let answer = r#"{"events": [
            {"seq": 3, "kind": "reminder", "title": "Kestrel reminder", "body": "09:00 Call mom", "at": 1},
            {"seq": 4, "kind": "reminder", "title": "Kestrel: 2 reminders", "body": "a\nb", "at": 2}],
            "last": 4, "desktop": false}"#;
        assert_eq!(
            notifications(answer),
            Some((
                vec![
                    ("Kestrel reminder".into(), "09:00 Call mom".into()),
                    ("Kestrel: 2 reminders".into(), "a\nb".into())
                ],
                4
            ))
        );
        let shown =
            r#"{"events": [{"seq": 1, "title": "t", "body": "b"}], "last": 1, "desktop": true}"#;
        assert_eq!(notifications(shown), Some((vec![], 1)));
        // An older service has no events (404 never reaches here); anything else is no answer.
        assert_eq!(notifications("null"), None);
        assert_eq!(notifications("not json"), None);
    }

    #[test]
    fn the_window_stays_on_this_machine() {
        assert!(may_open("http://127.0.0.1:7811/", 7811));
        assert!(may_open("http://127.0.0.1:7811/#t=x", 7811));
        assert!(may_open("tauri://localhost/index.html", 7811));
        assert!(may_open("http://tauri.localhost/index.html", 7811));
        assert!(!may_open("http://127.0.0.1:7812/", 7811));
        assert!(!may_open("http://127.0.0.1:7811@evil.example/", 7811));
        assert!(!may_open("https://example.com/", 7811));
    }
}
