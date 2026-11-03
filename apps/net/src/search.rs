//! Web search through the search provider slot (blueprint §4.8): a SearXNG instance the person chose, on
//! the public internet over https, or on this device. Results are untrusted text: plain, capped, and
//! only web addresses.

use serde_json::{Value, json};

use crate::plain::{plain, web_address};

/// The longest search asked, in letters.
const LONGEST_QUERY: usize = 300;
/// The most results kept.
const MOST: usize = 8;
const LONGEST_TITLE: usize = 200;
const LONGEST_SNIPPET: usize = 500;

/// The host of an address's authority; None when it hides another host behind a user part.
fn host_of(rest: &str) -> Option<&str> {
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    authority.split(':').next()
}

/// May this provider address be asked? Why not, if not.
pub fn provider_refusal(base: &str) -> Option<String> {
    const NOT_ONE: &str =
        "a search provider is an https address, or an http address on this device";
    if let Some(rest) = base.strip_prefix("http://") {
        let here = matches!(host_of(rest), Some("127.0.0.1" | "localhost"));
        return (!here).then(|| NOT_ONE.to_string());
    }
    let Some(host) = base.strip_prefix("https://").and_then(host_of) else {
        return Some(NOT_ONE.to_string());
    };
    crate::fetch::refused_name(host)
}

/// SearXNG's JSON answer as plain results.
pub fn parse_results(body: &Value) -> Value {
    let found = body["results"].as_array().map_or(&[][..], Vec::as_slice);
    let results: Vec<Value> = found
        .iter()
        .filter_map(|r| {
            let url = web_address(r["url"].as_str()?)?;
            let text = |key: &str, longest| plain(r[key].as_str().unwrap_or(""), longest);
            let title = Some(text("title", LONGEST_TITLE))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| site_of(&url));
            Some(json!({"title": title, "url": url, "snippet": text("content", LONGEST_SNIPPET)}))
        })
        .take(MOST)
        .collect();
    json!({"results": results})
}

/// The site an address is on.
fn site_of(url: &str) -> String {
    let rest = url.split("//").nth(1).unwrap_or(url);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_string()
}

pub fn search(provider: &str, query: &str) -> Result<Value, String> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > LONGEST_QUERY {
        return Err(format!("a search of 1 to {LONGEST_QUERY} letters"));
    }
    if let Some(why) = provider_refusal(provider) {
        return Err(why);
    }
    let base = provider.trim_end_matches('/');
    // A name is checked again after it is looked up, like every public address.
    if provider.starts_with("https://") {
        crate::fetch::check(base)?;
    }
    let (status, body) = crate::fetch::get_json(
        &format!("{base}/search"),
        &[("q", query), ("format", "json")],
    )?;
    if status != 200 {
        return Err(format!(
            "the search provider said {status}: is JSON format turned on there?"
        ));
    }
    if !body["results"].is_array() {
        return Err(
            "the search provider did not answer in JSON: is JSON format turned on there?".into(),
        );
    }
    Ok(parse_results(&body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    #[test]
    fn a_provider_is_a_public_https_address_or_on_this_device() {
        for good in [
            "https://search.example.org",
            "https://search.example.org/searx/",
            "http://127.0.0.1:8888",
            "http://localhost:8888/",
        ] {
            assert_eq!(provider_refusal(good), None, "{good}");
        }
        for bad in [
            "http://search.example.org",
            "ftp://x.example",
            "https://192.168.1.1",
            "https://10.0.0.5:8888",
            "https://localhost:8888",
            "http://192.168.1.5:8888",
            "http://127.0.0.1.evil.example",
            "http://127.0.0.1:80@evil.example",
            "https://search.example.org@192.168.1.1",
            "http://localhost.evil.example:80",
            "not an address",
            "",
        ] {
            assert!(provider_refusal(bad).is_some(), "{bad}");
        }
    }

    #[test]
    fn results_are_plain_capped_and_only_web_addresses() {
        let long = "y".repeat(900);
        let body = json!({"results": [
            {"title": "First <b>one</b>\n", "url": "https://a.example/1", "content": long, "engine": "x"},
            {"title": "Script", "url": "javascript:alert(1)", "content": "bad"},
            {"title": "No address", "content": "bad"},
            {"title": "", "url": "https://a.example/2", "content": "no title"},
            {"url": "https://a.example/3", "content": "text"},
        ]});
        let v = parse_results(&body);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 3, "two have no web address: {v}");
        assert_eq!(results[0]["title"], "First one");
        assert_eq!(results[0]["url"], "https://a.example/1");
        assert_eq!(
            results[0]["snippet"].as_str().unwrap().chars().count(),
            LONGEST_SNIPPET
        );
        assert_eq!(results[1]["url"], "https://a.example/2");
        assert_eq!(
            results[1]["title"], "a.example",
            "a page with no title is called by its site"
        );
        assert_eq!(parse_results(&json!({}))["results"], json!([]));
        let many: Vec<Value> = (0..30)
            .map(|i| json!({"title": "t", "url": format!("https://a.example/{i}")}))
            .collect();
        assert_eq!(
            parse_results(&json!({"results": many}))["results"]
                .as_array()
                .unwrap()
                .len(),
            MOST
        );
    }

    /// One answer on a real socket on this machine; what was asked comes back through the join.
    fn server(
        status: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            first
        });
        (address, handle)
    }

    #[test]
    fn a_search_asks_for_json_and_says_the_words_once_encoded() {
        let body =
            r#"{"results": [{"title": "A", "url": "https://a.example/", "content": "text"}]}"#;
        let (address, asked) = server("200 OK", body);
        let v = search(&address, "mausam & rain? 100%").unwrap();
        assert_eq!(v["results"][0]["title"], "A");
        let asked = asked.join().unwrap();
        assert!(asked.starts_with("GET /search?"), "{asked}");
        assert!(asked.contains("format=json"), "{asked}");
        assert!(
            asked.contains("q=mausam") && asked.contains("%26") && asked.contains("%25"),
            "{asked}"
        );
        assert!(!asked.contains("rain? "), "{asked}");
    }

    #[test]
    fn a_provider_that_refuses_says_so_with_the_likely_reason() {
        let (address, _asked) = server("403 Forbidden", "{}");
        let e = search(&address, "x").unwrap_err();
        assert!(e.contains("403") && e.contains("JSON"), "{e}");
    }

    #[test]
    fn an_empty_search_or_a_bad_provider_asks_nothing() {
        assert!(search("http://127.0.0.1:9", "  ").is_err());
        let long = "x".repeat(301);
        assert!(
            search("http://127.0.0.1:9", &long)
                .unwrap_err()
                .contains("300")
        );
        assert!(
            search("https://192.168.1.1", "x")
                .unwrap_err()
                .contains("not on the public internet")
        );
    }
}
