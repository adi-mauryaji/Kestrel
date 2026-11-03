//! News feeds (RSS 2.0 and Atom) as a short list of plain headlines. A feed is untrusted text: this reads
//! only the few tags it needs, keeps no markup, and caps what it keeps.

use serde_json::{Value, json};

use crate::plain::{plain, web_address};

/// The most headlines kept from one feed.
const MOST: usize = 20;
/// The longest headline kept, in characters.
const LONGEST: usize = 200;

/// The inner text of each `<tag ...>...</tag>` in `text`; one never closed is not returned.
fn blocks<'a>(text: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        // `<item` must end there: `<items>` is another tag.
        if !after.starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace()) {
            rest = after;
            continue;
        }
        let Some(end) = after.find('>') else { break };
        let (attributes, inner) = (&after[..end], &after[end + 1..]);
        if attributes.ends_with('/') {
            found.push((attributes, ""));
            rest = inner;
            continue;
        }
        let Some(stop) = inner.find(&close) else {
            break;
        };
        found.push((attributes, &inner[..stop]));
        rest = &inner[stop + close.len()..];
    }
    found
}

/// A tag's text, without a CDATA wrapper.
fn unwrapped(inner: &str) -> &str {
    let inner = inner.trim();
    inner
        .strip_prefix("<![CDATA[")
        .and_then(|i| i.strip_suffix("]]>"))
        .unwrap_or(inner)
}

fn first_text(block: &str, tag: &str) -> Option<String> {
    let (_, inner) = *blocks(block, tag).first()?;
    Some(plain(unwrapped(inner), LONGEST)).filter(|t| !t.is_empty())
}

/// An attribute's value in a tag's attributes.
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let at = attributes.find(&format!("{name}=\""))?;
    let value = &attributes[at + name.len() + 2..];
    value.split('"').next()
}

/// The page an item points to: RSS's text, or Atom's `href` that is not the feed's own.
fn link_of(block: &str) -> Option<String> {
    blocks(block, "link")
        .into_iter()
        .find_map(|(attributes, inner)| {
            let rel = attribute(attributes, "rel").unwrap_or("alternate");
            let text = match attribute(attributes, "href") {
                Some(href) if rel == "alternate" => href,
                Some(_) => return None,
                None => unwrapped(inner),
            };
            web_address(text)
        })
}

/// The feed's name and its items, newest as the feed orders them.
pub fn parse_feed(text: &str) -> Value {
    let entry = if text.contains("<entry") {
        "entry"
    } else {
        "item"
    };
    let head = text.split(&format!("<{entry}")).next().unwrap_or("");
    let items: Vec<Value> = blocks(text, entry)
        .into_iter()
        .filter_map(|(_, block)| {
            let title = first_text(block, "title")?;
            let date = ["pubDate", "published", "updated"]
                .iter()
                .find_map(|tag| first_text(block, tag));
            Some(json!({"title": title, "link": link_of(block), "date": date}))
        })
        .take(MOST)
        .collect();
    json!({"title": first_text(head, "title"), "items": items})
}

pub fn rss(url: &str) -> Result<Value, String> {
    let page = crate::fetch::fetch(url)?;
    if page["status"] != 200 {
        return Err(format!("the feed answered {}", page["status"]));
    }
    Ok(parse_feed(page["text"].as_str().unwrap_or("")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>The Daily &amp; More</title><link>https://news.example</link>
<item><title>Rain in Delhi &#8211; roads shut</title><link>https://news.example/1</link><pubDate>Tue, 03 Nov 2026 08:00:00 GMT</pubDate><description>Long text</description></item>
<item><title><![CDATA[Budget <b>passed</b> & signed]]></title><link><![CDATA[https://news.example/2]]></link></item>
<item><description>No title here</description></item>
</channel></rss>"#;

    const ATOM: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title type="text">Atom News</title>
<entry><title type="html">A &lt;i&gt;new&lt;/i&gt; bridge</title><link rel="alternate" href="https://atom.example/a"/><updated>2026-11-03T08:00:00Z</updated></entry>
<entry><link rel="self" href="https://atom.example/self"/><link href="https://atom.example/b"></link><title>Second</title><published>2026-11-02</published></entry>
</feed>"#;

    #[test]
    fn rss_items_come_as_plain_headlines() {
        let v = parse_feed(RSS);
        assert_eq!(v["title"], "The Daily & More");
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "an item with no title is left out: {v}");
        assert_eq!(items[0]["title"], "Rain in Delhi – roads shut");
        assert_eq!(items[0]["link"], "https://news.example/1");
        assert_eq!(items[0]["date"], "Tue, 03 Nov 2026 08:00:00 GMT");
        assert_eq!(items[1]["title"], "Budget passed & signed");
        assert_eq!(items[1]["link"], "https://news.example/2");
    }

    #[test]
    fn atom_entries_too_with_the_page_link_not_the_feed_s_own() {
        let v = parse_feed(ATOM);
        assert_eq!(v["title"], "Atom News");
        assert_eq!(v["items"][0]["title"], "A new bridge");
        assert_eq!(v["items"][0]["link"], "https://atom.example/a");
        assert_eq!(v["items"][0]["date"], "2026-11-03T08:00:00Z");
        assert_eq!(v["items"][1]["link"], "https://atom.example/b");
        assert_eq!(v["items"][1]["date"], "2026-11-02");
    }

    #[test]
    fn a_link_that_is_not_a_web_address_is_dropped() {
        let v = parse_feed(
            "<rss><channel><item><title>x</title><link>javascript:alert(1)</link></item></channel></rss>",
        );
        assert_eq!(v["items"][0]["title"], "x");
        assert_eq!(v["items"][0]["link"], Value::Null);
    }

    #[test]
    fn what_is_kept_is_capped_and_plain() {
        let long = "x".repeat(500);
        let many = format!(
            "<rss><channel>{}</channel></rss>",
            format!("<item><title>{long}\u{7}\nmore</title></item>").repeat(50)
        );
        let v = parse_feed(&many);
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), MOST);
        let t = items[0]["title"].as_str().unwrap();
        assert_eq!(t.chars().count(), LONGEST);
        assert!(!t.chars().any(char::is_control), "{t:?}");
    }

    #[test]
    fn something_that_is_no_feed_has_no_items() {
        for text in [
            "",
            "<html><body>not a feed</body></html>",
            "<rss><item><title>never closed",
            "<<<>>>",
        ] {
            assert_eq!(parse_feed(text)["items"], json!([]), "{text}");
        }
    }
}
