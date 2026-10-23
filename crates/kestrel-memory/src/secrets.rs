//! Detect-and-refuse for obvious secrets (Ch. 17.1 [D]): memory never stores passwords, PINs,
//! OTPs, API keys or long digit runs (card, account or Aadhaar numbers). The list is small and
//! errs on the side of refusing: a refused harmless memory costs a retry, a stored credential
//! sits in a file until deleted.

use kestrel_core::features::tokens;

/// Words that name a credential, as whole words of the key or the value.
const WORDS: &[&str] = &[
    "password", "passwd", "passcode", "pin", "otp", "cvv", "apikey",
];
/// Two-word names of a credential.
const PAIRS: &[[&str; 2]] = &[
    ["api", "key"],
    ["secret", "key"],
    ["access", "key"],
    ["private", "key"],
    ["access", "token"],
    ["auth", "token"],
];
/// Prefixes of well-known API keys (OpenAI, GitHub, AWS, Slack).
const KEY_PREFIXES: &[&str] = &["sk-", "ghp_", "github_pat_", "akia", "xoxb-", "xoxp-"];
/// Digits in one number (spaces or hyphens between groups) from which it is a card, account or
/// Aadhaar number. A number written after `+` is a phone number and passes.
const LONG_NUMBER: usize = 12;
/// A word this long with letters and digits and no other punctuation than `-_+/=.` is a token.
const LONG_TOKEN: usize = 24;

/// Why `key: value` looks like a secret, or None.
pub fn refusal(key: &str, value: &str) -> Option<&'static str> {
    let text = format!("{key} {value}").to_lowercase();
    let words = tokens(&text);
    if words.iter().any(|w| WORDS.contains(w))
        || words.windows(2).any(|p| PAIRS.iter().any(|q| p == q))
    {
        return Some("it names a password, PIN, OTP or key");
    }
    for w in text.split_whitespace() {
        if KEY_PREFIXES.iter().any(|p| w.starts_with(p)) && w.len() >= 16 {
            return Some("it looks like an API key");
        }
        let token_chars = w
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_+/=.".contains(c));
        if w.len() >= LONG_TOKEN
            && token_chars
            && w.chars().any(|c| c.is_ascii_alphabetic())
            && w.chars().any(|c| c.is_ascii_digit())
        {
            return Some("it looks like a key or token");
        }
    }
    if longest_number(&text) >= LONG_NUMBER {
        return Some("it contains a long number (card, account or ID)");
    }
    None
}

/// Digits in the longest number of `text`, where one space or hyphen may join digit groups
/// ("4111 1111 1111 1111" = 16). Numbers right after `+` (phone numbers) count 0.
fn longest_number(text: &str) -> usize {
    let (mut best, mut run, mut phone, mut gap, mut prev) = (0, 0, false, false, ' ');
    for c in text.chars() {
        if c.is_ascii_digit() {
            if run == 0 {
                phone = prev == '+';
            }
            run += 1;
            gap = false;
            if !phone {
                best = best.max(run);
            }
        } else if (c == ' ' || c == '-') && run > 0 && !gap {
            gap = true;
        } else {
            run = 0;
            gap = false;
        }
        prev = c;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_keys_and_long_numbers_are_refused() {
        for (key, value) in [
            ("wifi_password", "hunter2"),
            ("bank", "my pin is 4821"),
            ("bank_otp", "482193"),
            ("note", "the api key is abc"),
            ("github", "ghp_abcdefghijklmnopqrstuvwxyz0123"),
            ("openai", "sk-proj-abcdefghijklmnop"),
            ("server", "Zx9q2LmP0aB7cD3eF5gH1iJ4"),
            ("card", "4111 1111 1111 1111"),
            ("aadhaar", "1234 5678 9012"),
            ("account", "acct 123456789012"),
        ] {
            assert!(refusal(key, value).is_some(), "{key}: {value}");
        }
    }

    #[test]
    fn ordinary_memories_pass() {
        for (key, value) in [
            ("answer_length", "concise"),
            ("advisor", "Dr. Rao, office hours Tuesday 3 pm"),
            ("asha_phone", "+91 98765 43210"),
            ("asha_phone_local", "98765 43210"),
            ("deadline", "Kestrel draft due 2026-12-15"),
            ("shopping", "spinach and paneer"),
            ("project", "kestrel-memory crate in Rust"),
            ("asha_birthday", "asha ka birthday 14 march ko hai"),
            (
                "docs",
                "https://example.org/kestrel/notes/2026-10-23-standup",
            ),
        ] {
            assert_eq!(refusal(key, value), None, "{key}: {value}");
        }
    }
}
