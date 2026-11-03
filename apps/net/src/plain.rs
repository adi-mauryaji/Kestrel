//! Text from the internet as plain text: no markup, no control characters, capped. It reaches the page
//! and a model's prompt as data, so nothing here keeps a tag or a line break that could pass for one.

/// `text` without entities, tags and control characters, cut at `longest` characters.
pub fn plain(text: &str, longest: usize) -> String {
    let unescaped = unescape(text);
    let mut kept = String::with_capacity(unescaped.len());
    let mut rest = unescaped.as_str();
    while let Some(at) = rest.find('<') {
        kept.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let is_tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match after.find('>').filter(|_| is_tag) {
            Some(end) => rest = &after[end + 1..],
            None => {
                kept.push('<');
                rest = after;
            }
        }
    }
    kept.push_str(rest);
    let spaced: String = kept
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let words: Vec<&str> = spaced.split_whitespace().collect();
    words.join(" ").chars().take(longest).collect()
}

/// The entities a headline uses; an unknown one stays as it is.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let found = after.find(';').filter(|end| *end <= 8).and_then(|end| {
            let name = &after[..end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => name.strip_prefix('#').and_then(|n| {
                    let code = match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    };
                    code.and_then(char::from_u32)
                }),
            };
            c.map(|c| (c, end))
        });
        match found {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A web address, if `text` is one.
pub fn web_address(text: &str) -> Option<String> {
    let text = text.trim();
    let web = text.starts_with("https://") || text.starts_with("http://");
    (web && !text.contains(|c: char| c.is_whitespace() || c.is_control())).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_and_entities_go_and_ordinary_signs_stay() {
        for (text, want) in [
            ("A &lt;i&gt;new&lt;/i&gt; bridge", "A new bridge"),
            ("Tom &amp; Jerry &#8211; &#x2014;", "Tom & Jerry – —"),
            ("3 < 5 and 7 > 2", "3 < 5 and 7 > 2"),
            ("AT&T & co; &unknown;", "AT&T & co; &unknown;"),
            ("two\n\n lines\tand\u{7}bell", "two lines and bell"),
            ("<script>alert(1)</script>safe", "alert(1)safe"),
            ("<<<>>>", "<<<>>>"),
            ("&#xFFFFFFFF; &#55555555;", "&#xFFFFFFFF; &#55555555;"),
        ] {
            assert_eq!(plain(text, 100), want, "{text}");
        }
        assert_eq!(plain("नमस्ते दुनिया", 6), "नमस्ते");
    }

    #[test]
    fn only_a_web_address_is_one() {
        assert_eq!(
            web_address(" https://a.example/x ").as_deref(),
            Some("https://a.example/x")
        );
        for bad in [
            "javascript:alert(1)",
            "ftp://a",
            "https://a b",
            "file:///etc/passwd",
            "",
        ] {
            assert_eq!(web_address(bad), None, "{bad}");
        }
    }
}
