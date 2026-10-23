//! Correction detection (Ch. 20, A.12, ADR-026): the deterministic baseline; no model weights
//! change. A correction is an opener ("no,", "actually", "nahi", "galat") followed by a
//! recognisable preference. "I prefer", "always", "hamesha", "aage se" make it durable: ask, then
//! store with provenance `correction`. "This time", "is baar", "abhi ke liye" make it one-off, and
//! so does anything unclear (the reversible default): apply to this turn, store nothing.
//!
//! Changes from A.12: words match whole tokens ("brief" no longer matches "briefing", nor
//! "short" "shortcut"); Hinglish cues (ADR-026); a negated style word counts for the other value
//! ("not so long", "itna lamba nahi" → short); the keys and values are the template planner's
//! (`answer_length` = short | detailed, `summary_style`, `language`), so a correction and an
//! `update_preference` write the same row; contradictory or multi-key statements are not taken as
//! corrections (nothing is applied rather than the wrong thing).

use kestrel_core::KestrelError;
use kestrel_core::features::tokens;
use kestrel_memory::{Draft, Kind, Memory, Source};

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub key: &'static str,
    pub value: &'static str,
    pub exception: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Durable-looking preference: ask the user to confirm, then store with provenance=correction.
    StoreAfterConfirm(Candidate),
    /// One-off instruction: apply to this turn only, store nothing.
    ApplyOnce(Candidate),
    NotACorrection,
}

// The word lists (ADR-026: reviewed by the human). Phrases are matched on whole tokens.

/// A correction starts with one of these token sequences.
const OPENERS: &[&[&str]] = &[
    &["no"],
    &["nope"],
    &["actually"],
    &["that's", "wrong"],
    &["that", "is", "wrong"],
    &["wrong"],
    &["nahi"],
    &["nahin"],
    &["galat"],
    &["aisa", "nahi"],
];
/// What follows the first of these is the stated exception, not part of the preference.
const EXCEPTION: &[&str] = &[" unless ", " except when ", " jab tak "];
/// One-off cues; checked first, so "I prefer ... for now" is one-off.
const ONE_OFF: &[&str] = &[
    "this time",
    "just now",
    "for this one",
    "for now",
    "right now",
    "just this once",
    "is baar",
    "abhi ke liye",
    "sirf abhi",
    "bas abhi",
];
const DURABLE: &[&str] = &[
    "i prefer",
    "always",
    "from now on",
    "in future",
    "going forward",
    "every time",
    "hamesha",
    "aage se",
    "ab se",
    "pasand",
    "har baar",
];
/// (key, value, words). Each key has exactly two values, so a negated word means the other one.
const STYLES: &[(&str, &str, &[&str])] = &[
    (
        "answer_length",
        "short",
        &[
            "short",
            "shorter",
            "brief",
            "briefer",
            "briefly",
            "concise",
            "chhota",
            "chhote",
            "chhoti",
            "sankshipt",
        ],
    ),
    (
        "answer_length",
        "detailed",
        &[
            "detailed", "long", "longer", "lengthy", "thorough", "lamba", "lambe", "lambi",
            "detail", "vistar", "vistaar",
        ],
    ),
    ("summary_style", "bullets", &["bullet", "bullets"]),
    (
        "summary_style",
        "paragraph",
        &["paragraph", "paragraphs", "prose"],
    ),
    ("language", "hinglish", &["hinglish", "hindi"]),
    ("language", "english", &["english", "angrezi"]),
];
/// Negation up to two tokens before a style word (English, "kam" = less) ...
const NEGATE_BEFORE: &[&str] = &[
    "not", "no", "don't", "dont", "never", "less", "without", "kam",
];
/// ... or up to two tokens after it (Hinglish: "itna lamba nahi").
const NEGATE_AFTER: &[&str] = &["nahi", "nahin", "mat", "na"];

/// Classifies normalized user text (kestrel_core::normalize).
pub fn detect(normalized: &str) -> Action {
    let (head, exception) = split_exception(normalized);
    let words = tokens(head);
    let Some(opener) = OPENERS.iter().find(|o| words.starts_with(o)) else {
        return Action::NotACorrection;
    };
    let Some((key, value)) = style(&words[opener.len()..]) else {
        return Action::NotACorrection;
    };
    let cand = Candidate {
        key,
        value,
        exception,
    };
    let text = format!(" {} ", tokens(normalized).join(" "));
    let says = |phrases: &[&str]| phrases.iter().any(|p| text.contains(&format!(" {p} ")));
    if says(ONE_OFF) {
        Action::ApplyOnce(cand)
    } else if says(DURABLE) {
        Action::StoreAfterConfirm(cand)
    } else {
        Action::ApplyOnce(cand) // unclear durability: the reversible, non-persistent action
    }
}

/// (text before the first exception marker, the exception after it).
fn split_exception(t: &str) -> (&str, Option<String>) {
    let first = EXCEPTION
        .iter()
        .filter_map(|m| t.find(m).map(|i| (i, m.len())))
        .min();
    match first {
        Some((i, n)) => {
            let e = t[i + n..].trim();
            (&t[..i], (!e.is_empty()).then(|| e.to_string()))
        }
        None => (t, None),
    }
}

/// The one (key, value) all style words agree on, a negated word voting for the key's other
/// value; None if there is none, or they disagree, or they name more than one key.
fn style(words: &[&str]) -> Option<(&'static str, &'static str)> {
    let mut votes = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let Some(&(key, value, _)) = STYLES.iter().find(|(_, _, ws)| ws.contains(w)) else {
            continue;
        };
        let before = &words[i.saturating_sub(2)..i];
        let after = &words[i + 1..(i + 3).min(words.len())];
        let negated = before.iter().any(|b| NEGATE_BEFORE.contains(b))
            || after.iter().any(|a| NEGATE_AFTER.contains(a));
        votes.push(if negated {
            (key, other(key, value))
        } else {
            (key, value)
        });
    }
    let first = *votes.first()?;
    votes.iter().all(|v| *v == first).then_some(first)
}

fn other(key: &str, value: &str) -> &'static str {
    STYLES
        .iter()
        .find(|(k, v, _)| *k == key && *v != value)
        .map(|(_, v, _)| *v)
        .expect("every style key has two values")
}

/// The confirmation question for a durable correction (Ch. 20 step 3).
pub fn question(c: &Candidate) -> String {
    match &c.exception {
        Some(e) => format!("Save: {} = {} (unless {e})?", c.key, c.value),
        None => format!("Save: {} = {}?", c.key, c.value),
    }
}

/// After the user said yes to `question`: a preference with provenance `correction` and its
/// exception, audited as a confirmation (Ch. 20 step 4).
pub fn save(memory: &mut Memory, c: &Candidate, now: i64) -> Result<i64, KestrelError> {
    let mut d = Draft::new(Kind::Preference, c.key, c.value).importance(4);
    if let Some(e) = &c.exception {
        d = d.exception(e);
    }
    memory.confirm(&d, Source::Correction, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::normalize::normalize;
    use kestrel_memory::{Filter, Source};

    fn act(text: &str) -> Action {
        detect(&normalize(text))
    }

    fn cand(key: &'static str, value: &'static str) -> Candidate {
        Candidate {
            key,
            value,
            exception: None,
        }
    }

    #[test]
    fn chapter_20_example() {
        assert_eq!(
            act("No, I prefer concise answers unless I ask for details."),
            Action::StoreAfterConfirm(Candidate {
                exception: Some("i ask for details".into()),
                ..cand("answer_length", "short")
            })
        );
    }

    #[test]
    fn one_off_is_not_persisted() {
        assert_eq!(
            act("No, just this time keep it short"),
            Action::ApplyOnce(cand("answer_length", "short"))
        );
        assert_eq!(
            act("nahi, is baar detail mein batao"),
            Action::ApplyOnce(cand("answer_length", "detailed"))
        );
        assert_eq!(
            act("No, I prefer bullet points for now"),
            Action::ApplyOnce(cand("summary_style", "bullets")),
            "one-off wins over a durable cue"
        );
    }

    #[test]
    fn ambiguous_defaults_to_non_persistent() {
        assert_eq!(
            act("Actually make it brief"),
            Action::ApplyOnce(cand("answer_length", "short"))
        );
        assert_eq!(
            act("nahi, chhote jawab do jab tak main detail na maangu"),
            Action::ApplyOnce(Candidate {
                exception: Some("main detail na maangu".into()),
                ..cand("answer_length", "short")
            })
        );
    }

    #[test]
    fn durable_cues_in_either_language() {
        for (text, key, value) in [
            (
                "Nahi, mujhe hamesha chhote jawab chahiye",
                "answer_length",
                "short",
            ),
            (
                "galat, aage se hinglish mein jawab do",
                "language",
                "hinglish",
            ),
            (
                "nahi yaar, mujhe lambe jawab pasand hain",
                "answer_length",
                "detailed",
            ),
            (
                "That's wrong, from now on use paragraphs",
                "summary_style",
                "paragraph",
            ),
            ("no, always answer in english", "language", "english"),
        ] {
            assert_eq!(
                act(text),
                Action::StoreAfterConfirm(cand(key, value)),
                "{text}"
            );
        }
    }

    #[test]
    fn a_negated_style_word_counts_for_the_other_value() {
        assert_eq!(
            act("no, not so long, keep it short"),
            Action::ApplyOnce(cand("answer_length", "short"))
        );
        assert_eq!(
            act("nahi, itna lamba nahi"),
            Action::ApplyOnce(cand("answer_length", "short"))
        );
        assert_eq!(
            act("No, I always want detailed answers, not short ones"),
            Action::StoreAfterConfirm(cand("answer_length", "detailed"))
        );
    }

    #[test]
    fn ordinary_text_is_not_a_correction() {
        for text in [
            "Summarize my notes in a concise way",
            "notes mein se short summary do",
            "no, add the briefing to my tasks",
            "actually the shortcut is ctrl s",
            "nahi pata",
            "no, short or long, whatever",
            "no, short answers in english",
            "nobody prefers long meetings",
        ] {
            assert_eq!(act(text), Action::NotACorrection, "{text}");
        }
    }

    #[test]
    fn the_question_names_the_value_and_the_exception() {
        let c = Candidate {
            exception: Some("i ask for details".into()),
            ..cand("answer_length", "short")
        };
        assert_eq!(
            question(&c),
            "Save: answer_length = short (unless i ask for details)?"
        );
        assert_eq!(
            question(&cand("language", "hinglish")),
            "Save: language = hinglish?"
        );
    }

    #[test]
    fn a_confirmed_correction_is_stored_with_provenance_and_a_confirm_event() {
        let mut m = Memory::in_memory().unwrap();
        let Action::StoreAfterConfirm(c) =
            act("No, I prefer concise answers unless I ask for details")
        else {
            panic!("durable")
        };
        let id = save(&mut m, &c, 10).unwrap();
        let items = m.items(Filter::Approved).unwrap();
        assert_eq!(items.len(), 1);
        let it = &items[0];
        assert_eq!(
            (
                it.id,
                it.key.as_str(),
                it.value.as_str(),
                it.source,
                it.exception.as_deref()
            ),
            (
                id,
                "answer_length",
                "short",
                Source::Correction,
                Some("i ask for details")
            )
        );
        let ops: Vec<String> = m.audit().unwrap().into_iter().map(|e| e.op).collect();
        assert_eq!(ops, ["confirm"]);
    }
}
