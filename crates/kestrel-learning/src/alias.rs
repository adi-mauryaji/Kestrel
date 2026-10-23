//! Taught aliases (ADR-028): "when I say 'standup doc' I mean team sync", "by the gang I mean Riya
//! and Kabir", "jab main standup doc bolun to mera matlab team sync hai". A taught alias is a fact
//! the user confirms (key `alias_<words>`, value = what it means); before a request is planned, its
//! alias phrases are replaced by what they mean, so the deterministic planner sees the user's
//! canonical request. Phrases match whole words, longest first, in one pass: a replacement is never
//! replaced again.
//!
//! Limits: a fixed set of teaching phrasings (below; written by the dataset's author, ADR-028); a
//! phrase is kept as its words, so punctuation inside it ("q4-budget") does not survive.

use kestrel_core::KestrelError;
use kestrel_memory::{Memory, Source};

#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    /// What the user says (normalized words, single spaces).
    pub phrase: String,
    /// What they mean by it.
    pub target: String,
    /// It came as a correction ("no, by X I mean Y"), not as plain teaching.
    pub corrected: bool,
}

/// A correction opener before the teaching ("no, by X I mean Y").
const OPENERS: &[&str] = &["no", "nope", "actually", "nahi", "nahin", "galat"];
/// Hinglish "I say" forms after "jab main X ...".
const SAY: &[&str] = &["bolun", "bolu", "boloon", "kahun", "kahu", "kahoon"];
const SAY_HABIT: &[&str] = &["bolta", "bolti", "kehta", "kehti", "kahta", "kahti"];
const MAX_PHRASE_WORDS: usize = 6;
const MAX_TARGET: usize = 100;
const KEY_PREFIX: &str = "alias_";

/// The alias a normalized utterance teaches, if it is one of the teaching phrasings:
/// "when i say X, i mean Y", "by X i mean Y", "jab main X bolun/kahun (to) (mera) matlab Y (hai)",
/// "X se mera matlab Y (hai)"; optionally after a correction opener.
pub fn detect(normalized: &str) -> Option<Alias> {
    let (rest, corrected) = strip_opener(normalized);
    let (phrase, target) = english(rest).or_else(|| hinglish(rest))?;
    let phrase = clean(&phrase)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let target = clean(&target).to_string();
    let words = phrase.split(' ').count();
    let ok = !phrase.is_empty()
        && words <= MAX_PHRASE_WORDS
        && !target.is_empty()
        && target.chars().count() <= MAX_TARGET
        && !target.starts_with("to "); // "by friday i mean to finish": intent, not an alias
    ok.then_some(Alias {
        phrase,
        target,
        corrected,
    })
}

fn strip_opener(t: &str) -> (&str, bool) {
    for op in OPENERS {
        if let Some(rest) = t.strip_prefix(op)
            && rest.starts_with([',', ' '])
        {
            return (rest.trim_start_matches([',', ' ']), true);
        }
    }
    (t, false)
}

fn english(t: &str) -> Option<(String, String)> {
    let rest = t
        .strip_prefix("when i say ")
        .or_else(|| t.strip_prefix("by "))?;
    let (phrase, target) = rest.split_once(" i mean ")?;
    Some((phrase.into(), target.into()))
}

fn hinglish(t: &str) -> Option<(String, String)> {
    if let Some((phrase, target)) = t.split_once(" se mera matlab ") {
        return Some((phrase.into(), strip_hai(target).into()));
    }
    let words: Vec<&str> = t.split(' ').collect();
    if words.first() != Some(&"jab")
        || !matches!(words.get(1), Some(&("main" | "mai" | "mein" | "me")))
    {
        return None;
    }
    // The first "I say" form after the phrase: "bolun", or a habitual "bolta hoon".
    let (v, after) = (3..words.len()).find_map(|i| {
        let w = words[i].trim_end_matches(',');
        let next = words.get(i + 1).map(|w| w.trim_end_matches(','));
        if SAY.contains(&w) {
            Some((i, i + 1))
        } else if SAY_HABIT.contains(&w) && next == Some("hoon") {
            Some((i, i + 2))
        } else {
            None
        }
    })?;
    let mut rest = &words[after..];
    if matches!(rest.first(), Some(&("to" | "toh"))) {
        rest = &rest[1..];
    }
    let target = match rest {
        ["mera", "matlab", tail @ ..] | ["matlab", tail @ ..] => tail.join(" "),
        _ => return None,
    };
    Some((words[2..v].join(" "), strip_hai(&target).into()))
}

fn strip_hai(t: &str) -> &str {
    ["hota hai", "se hai", "hai"]
        .iter()
        .find_map(|s| t.strip_suffix(s))
        .map_or(t, str::trim_end)
}

fn clean(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ",'\"‘’“”".contains(c))
}

/// A phrase's words as compared: lowercase, apostrophes dropped, other characters split words.
fn slugs(text: &str) -> Vec<String> {
    word_spans(text).into_iter().map(|(_, _, w)| w).collect()
}

/// (start, end, slug) of every word: runs of alphanumerics and apostrophes.
fn word_spans(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        let word = c.is_alphanumeric() || c == '\'';
        match (start, word) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                let slug: String = text[s..i]
                    .to_lowercase()
                    .chars()
                    .filter(|c| *c != '\'')
                    .collect();
                if !slug.is_empty() {
                    out.push((s, i, slug));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The memory key of a phrase: `alias_` + its words joined by `_` (a-z, 0-9 only), at most 64.
pub fn key(phrase: &str) -> Option<String> {
    let words: Vec<String> = slugs(phrase)
        .into_iter()
        .map(|w| {
            w.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect();
    let key = format!("{KEY_PREFIX}{}", words.join("_"));
    (!words.is_empty() && key.len() <= kestrel_memory::store::MAX_KEY).then_some(key)
}

/// The confirmation question (a taught alias is stored only after a yes).
pub fn question(a: &Alias) -> String {
    format!(
        "Save: when you say \"{}\" you mean \"{}\"?",
        a.phrase, a.target
    )
}

/// After the user's yes: a fact `alias_<words>` = target, audited as a confirmation; provenance
/// `correction` if it came as one, else `user_explicit`.
pub fn save(memory: &mut Memory, a: &Alias, now: i64) -> Result<i64, KestrelError> {
    let key = key(&a.phrase)
        .ok_or_else(|| KestrelError::Invalid(format!("alias phrase {:?}", a.phrase)))?;
    let d = kestrel_memory::Draft::new(kestrel_memory::Kind::Fact, &key, &a.target).importance(4);
    let source = if a.corrected {
        Source::Correction
    } else {
        Source::UserExplicit
    };
    memory.confirm(&d, source, now)
}

/// The approved, unexpired aliases as (phrase, target), most words first (ties by phrase).
pub fn aliases(memory: &Memory, now: i64) -> Result<Vec<(String, String)>, KestrelError> {
    let mut out: Vec<(String, String)> = memory
        .items(kestrel_memory::Filter::Approved)?
        .into_iter()
        .filter(|i| i.kind == kestrel_memory::Kind::Fact && i.expires_at.is_none_or(|at| at > now))
        .filter_map(|i| {
            let phrase = i.key.strip_prefix(KEY_PREFIX)?.replace('_', " ");
            Some((phrase, i.value))
        })
        .collect();
    out.sort_by(|a, b| {
        b.0.split(' ')
            .count()
            .cmp(&a.0.split(' ').count())
            .then_with(|| a.0.cmp(&b.0))
    });
    Ok(out)
}

/// `text` with every alias phrase replaced by its target: whole words, the most words first at
/// each position, one left-to-right pass (targets are never scanned again). Everything outside a
/// match is kept as it was.
pub fn resolve(text: &str, aliases: &[(String, String)]) -> String {
    let mut phrases: Vec<(Vec<String>, &str)> = aliases
        .iter()
        .map(|(p, t)| (slugs(p), t.as_str()))
        .filter(|(p, _)| !p.is_empty())
        .collect();
    phrases.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    let spans = word_spans(text);
    let (mut out, mut last, mut i) = (String::new(), 0, 0);
    while i < spans.len() {
        let hit = phrases.iter().find(|(p, _)| {
            i + p.len() <= spans.len() && p.iter().zip(&spans[i..]).all(|(w, s)| *w == s.2)
        });
        match hit {
            Some((p, target)) => {
                out.push_str(&text[last..spans[i].0]);
                out.push_str(target);
                last = spans[i + p.len() - 1].1;
                i += p.len();
            }
            None => i += 1,
        }
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::normalize::normalize;
    use kestrel_memory::{Draft, Filter, Kind};

    fn alias(phrase: &str, target: &str, corrected: bool) -> Option<Alias> {
        Some(Alias {
            phrase: phrase.into(),
            target: target.into(),
            corrected,
        })
    }

    fn taught(text: &str) -> Option<Alias> {
        detect(&normalize(text))
    }

    #[test]
    fn english_teaching_phrasings() {
        assert_eq!(
            taught("When I say standup doc, I mean team sync."),
            alias("standup doc", "team sync", false)
        );
        assert_eq!(
            taught("No, by 'the gang' I mean Riya, Kabir and Asha"),
            alias("the gang", "riya, kabir and asha", true)
        );
        assert_eq!(
            taught("actually when i say latest notes i mean last 3 notes"),
            alias("latest notes", "last 3 notes", true)
        );
    }

    #[test]
    fn hinglish_teaching_phrasings() {
        assert_eq!(
            taught("jab main standup doc bolun to mera matlab team sync hai"),
            alias("standup doc", "team sync", false)
        );
        assert_eq!(
            taught("Nahi, gang se mera matlab riya aur kabir hai"),
            alias("gang", "riya aur kabir", true)
        );
        assert_eq!(
            taught("jab main \"ghar wale\" kahun toh matlab mummy aur papa"),
            alias("ghar wale", "mummy aur papa", false)
        );
    }

    #[test]
    fn ordinary_requests_are_not_teaching() {
        for text in [
            "summarize my last three meeting notes",
            "what does entropy mean",
            "i mean it, call asha",
            "when i say so, remind me",
            "No, I prefer short answers",
            "by tomorrow I mean to finish the report",
            "when I say",
        ] {
            assert_eq!(taught(text), None, "{text}");
        }
    }

    #[test]
    fn keys_are_the_phrase_words() {
        assert_eq!(key("standup doc").as_deref(), Some("alias_standup_doc"));
        assert_eq!(key("mom's place").as_deref(), Some("alias_moms_place"));
        assert_eq!(key("q4-budget").as_deref(), Some("alias_q4_budget"));
        assert_eq!(key("!!"), None);
        assert_eq!(key(&"word ".repeat(20)), None, "longer than a memory key");
    }

    #[test]
    fn the_question_shows_both_sides() {
        let a = alias("the gang", "riya and kabir", true).unwrap();
        assert_eq!(
            question(&a),
            "Save: when you say \"the gang\" you mean \"riya and kabir\"?"
        );
    }

    #[test]
    fn saved_aliases_come_back_longest_first_and_only_approved_and_live() {
        let mut m = Memory::in_memory().unwrap();
        save(&mut m, &alias("doc", "project plan", false).unwrap(), 0).unwrap();
        save(&mut m, &alias("standup doc", "team sync", true).unwrap(), 0).unwrap();
        let guess = Draft::new(Kind::Fact, "alias_boss", "dr rao");
        m.propose(&guess, Source::ModelInferred, 0).unwrap(); // pending: not an alias yet
        let old = Draft::new(Kind::Fact, "alias_old_flat", "kothrud flat").expires_at(5);
        m.propose(&old, Source::UserExplicit, 0).unwrap(); // expired at 10
        m.propose(
            &Draft::new(Kind::Fact, "advisor", "dr rao"),
            Source::UserExplicit,
            0,
        )
        .unwrap(); // a fact, not an alias
        assert_eq!(
            aliases(&m, 10).unwrap(),
            [
                ("standup doc".to_string(), "team sync".to_string()),
                ("doc".to_string(), "project plan".to_string())
            ]
        );
        let sources: Vec<Source> = m
            .items(Filter::Approved)
            .unwrap()
            .into_iter()
            .filter(|i| i.key.starts_with("alias_s") || i.key == "alias_doc")
            .map(|i| i.source)
            .collect();
        assert_eq!(sources, [Source::UserExplicit, Source::Correction]);
        let ops: Vec<String> = m.audit().unwrap().into_iter().map(|e| e.op).collect();
        assert_eq!(
            &ops[..2],
            ["confirm", "confirm"],
            "a taught alias is a confirmed write"
        );
    }

    #[test]
    fn resolving_replaces_whole_phrases_once_longest_first() {
        let al = vec![
            ("standup doc".to_string(), "team sync".to_string()),
            ("doc".to_string(), "project plan".to_string()),
            ("the gang".to_string(), "riya and kabir".to_string()),
            ("gang".to_string(), "kabir".to_string()),
            ("team".to_string(), "never used".to_string()),
        ];
        assert_eq!(resolve("open my standup doc", &al), "open my team sync");
        assert_eq!(resolve("open the doc", &al), "open the project plan");
        assert_eq!(
            resolve("message the gang about the standup doc", &al),
            "message riya and kabir about the team sync"
        );
        assert_eq!(
            resolve("trip to gangtok", &al),
            "trip to gangtok",
            "whole words only"
        );
        assert_eq!(resolve("summarize my notes", &al), "summarize my notes");
        assert_eq!(resolve("gang, call", &al), "kabir, call");
    }
}
