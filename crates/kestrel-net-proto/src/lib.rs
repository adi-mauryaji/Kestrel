//! What Kestrel's core says to `kestrel-net` (v4 C, blueprint §4.8), one JSON object per line, and the
//! approval token. No network crate here: the core links this one.
//!
//! A mail leaves only with a token made after the y/N gate: the SHA-256 of a key shared at the
//! process's start, a nonce used once, and the exact message. So a message changed after the yes, a
//! second send of the same yes, and a send that never passed the gate are all refused by the process
//! that holds the network, even if the core has a bug.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One mail, exactly as previewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mail {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// The operations there are; anything else is not parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    /// A web page as text.
    Fetch { url: String },
    /// A mail through Gmail, with the password from the OS keyring.
    SmtpSend {
        mail: Mail,
        nonce: String,
        approval: String,
    },
    /// Keeps the mail password in the OS keyring. It goes no further.
    SecretSet { value: String },
    /// Is the mail password in the keyring?
    SecretHas,
    /// Removes it.
    SecretDelete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub op: Op,
}

/// The token for sending `mail` once, under the process's `key`.
pub fn approval(key: &str, nonce: &str, mail: &Mail) -> String {
    let mut hash = Sha256::new();
    // Each part with its length, so text cannot move from one part to the next.
    for part in [key, nonce, &mail.from, &mail.to, &mail.subject, &mail.body] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The host of an https address, lowercased, for the log and the checks; None for anything else.
pub fn https_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = match host_port.strip_prefix('[') {
        Some(v6) => v6.split(']').next()?,
        None => host_port.split(':').next()?,
    };
    (!host.is_empty()).then(|| host.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail() -> Mail {
        Mail {
            from: "me@example.com".into(),
            to: "ravi@example.com".into(),
            subject: "Leave".into(),
            body: "I will be away on Friday.".into(),
        }
    }

    #[test]
    fn the_token_is_bound_to_the_key_the_nonce_and_every_part_of_the_mail() {
        let t = approval("key", "n1", &mail());
        assert_eq!(t.len(), 64);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(t, approval("key", "n1", &mail()));
        assert_ne!(t, approval("other", "n1", &mail()));
        assert_ne!(t, approval("key", "n2", &mail()));
        for change in [
            |m: &mut Mail| m.from.push('x'),
            |m: &mut Mail| m.to.push('x'),
            |m: &mut Mail| m.subject.push('x'),
            |m: &mut Mail| m.body.push('x'),
        ] {
            let mut m = mail();
            change(&mut m);
            assert_ne!(t, approval("key", "n1", &m), "{m:?}");
        }
    }

    #[test]
    fn moving_text_between_the_parts_changes_the_token() {
        let mut a = mail();
        a.subject = "ab".into();
        a.body = "c".into();
        let mut b = mail();
        b.subject = "a".into();
        b.body = "bc".into();
        assert_ne!(approval("k", "n", &a), approval("k", "n", &b));
    }

    #[test]
    fn an_operation_goes_as_one_json_object_and_an_unknown_one_is_refused() {
        let r = Request {
            id: 7,
            op: Op::Fetch {
                url: "https://example.com/".into(),
            },
        };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(
            line,
            r#"{"id":7,"op":"fetch","url":"https://example.com/"}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), r);
        assert!(serde_json::from_str::<Request>(r#"{"id":1,"op":"shell","cmd":"ls"}"#).is_err());
    }

    #[test]
    fn only_https_addresses_have_a_host() {
        assert_eq!(
            https_host("https://Example.com/a?b=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            https_host("https://example.com:8443/").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            https_host("https://user:pw@example.com/").as_deref(),
            Some("example.com")
        );
        for bad in [
            "http://example.com/",
            "ftp://x/",
            "example.com",
            "https://",
            "https:///a",
            "",
        ] {
            assert_eq!(https_host(bad), None, "{bad}");
        }
    }
}
