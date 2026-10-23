//! The deterministic retrieval baseline (Ch. 18.1):
//!
//! ```text
//! score = w_rel * matched / terms + w_rec * 2^(-age_days / half_life) + w_imp * importance / 5
//! ```
//!
//! over approved, unexpired items (the store filters) with at least one matched term, so
//! importance and recency alone never bring in an unrelated memory. Ties break by id.
//!
//! Changes from A.11: terms match whole words of the key and value, not substrings ("short" no
//! longer matches "shortcut", nor "kal" "kalpana"); a possessive matches its name ("asha's" →
//! "asha"); repeated query terms count once; weights are checked, so the sort cannot meet a NaN
//! (A.11's `partial_cmp().unwrap()` panicked on one).

use std::collections::HashSet;

use kestrel_core::KestrelError;
use kestrel_core::features::tokens;

use crate::store::Item;

pub const DAY: i64 = 86_400;

/// Hyperparameters, tuned on validation data (Ch. 18.1 defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub relevance: f64,
    pub recency: f64,
    pub importance: f64,
    pub half_life_days: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            relevance: 0.6,
            recency: 0.25,
            importance: 0.15,
            half_life_days: 30.0,
        }
    }
}

impl Weights {
    /// Finite, non-negative weights and a positive half-life.
    pub fn check(&self) -> Result<(), KestrelError> {
        let weights = [self.relevance, self.recency, self.importance];
        let ok = weights.iter().all(|w| w.is_finite() && *w >= 0.0)
            && self.half_life_days.is_finite()
            && self.half_life_days > 0.0;
        if ok {
            Ok(())
        } else {
            Err(KestrelError::Invalid(format!("retrieval weights {self:?}")))
        }
    }
}

/// An item with its score and how many query terms it matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    pub item: Item,
    pub score: f64,
    pub matched: usize,
}

/// Words that carry no topic, English and Hinglish; dropped from queries.
const STOP: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "am", "i", "me", "my", "mine",
    "you", "your", "we", "our", "he", "she", "they", "them", "his", "her", "their", "it", "its",
    "this", "that", "these", "those", "of", "to", "in", "on", "at", "for", "from", "with", "by",
    "about", "and", "or", "but", "do", "does", "did", "what", "when", "where", "who", "whom",
    "which", "how", "why", "can", "could", "would", "should", "will", "please", "tell", "show",
    "give", "remember", "recall", "know", "any", "some", "there", "here", "have", "has", "had",
    "kya", "kab", "kaun", "kahan", "kaise", "kyun", "hai", "hain", "tha", "thi", "ho", "ka", "ki",
    "ke", "ko", "se", "mein", "ne", "par", "pe", "aur", "ya", "bhi", "toh", "yeh", "ye", "woh",
    "wo", "mera", "meri", "mere", "mujhe", "hum", "tum", "aap", "apna", "apni", "apne", "batao",
    "bata", "yaad", "karo", "kar",
];

/// Lowercased words of `text`; a possessive is its name ("asha's" → "asha").
fn words(text: &str) -> Vec<String> {
    tokens(&text.to_lowercase())
        .into_iter()
        .map(|t| t.strip_suffix("'s").unwrap_or(t).to_string())
        .collect()
}

/// Query terms: the words of `text` minus stopwords, each once, in order.
pub fn terms(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    words(text)
        .into_iter()
        .filter(|w| !STOP.contains(&w.as_str()) && seen.insert(w.clone()))
        .collect()
}

/// The item's score for `terms` at `now` and the number of terms it matched; None when it
/// matches none (or there are no terms).
pub fn score(item: &Item, terms: &[String], now: i64, w: &Weights) -> Option<(f64, usize)> {
    let have: HashSet<String> = words(&format!("{} {}", item.key, item.value))
        .into_iter()
        .collect();
    let matched = terms.iter().filter(|t| have.contains(*t)).count();
    if matched == 0 {
        return None;
    }
    let relevance = matched as f64 / terms.len() as f64;
    let age_days = (now - item.updated_at).max(0) as f64 / DAY as f64;
    let recency = 0.5f64.powf(age_days / w.half_life_days);
    let importance = f64::from(item.importance) / 5.0;
    let s = w.relevance * relevance + w.recency * recency + w.importance * importance;
    Some((s, matched))
}

/// The `limit` best items by score, ties by id; items matching no term are left out.
pub fn rank(
    items: Vec<Item>,
    terms: &[String],
    now: i64,
    limit: usize,
    w: &Weights,
) -> Vec<Scored> {
    let mut scored: Vec<Scored> = items
        .into_iter()
        .filter_map(|item| {
            let (score, matched) = score(&item, terms, now, w)?;
            Some(Scored {
                item,
                score,
                matched,
            })
        })
        .collect();
    scored.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.item.id.cmp(&b.item.id)));
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Kind, Source};

    fn item(id: i64, key: &str, value: &str, importance: u8, updated_at: i64) -> Item {
        Item {
            id,
            kind: Kind::Fact,
            key: key.into(),
            value: value.into(),
            exception: None,
            source: Source::UserExplicit,
            importance,
            approved: true,
            created_at: 0,
            updated_at,
            expires_at: None,
        }
    }

    #[test]
    fn terms_drop_stopwords_and_repeats_in_either_language() {
        assert_eq!(
            terms("What do you remember about Asha's birthday?"),
            ["asha", "birthday"]
        );
        assert_eq!(
            terms("asha ka birthday kab hai, asha ka?"),
            ["asha", "birthday"]
        );
        assert!(terms("what is the").is_empty());
    }

    #[test]
    fn terms_match_whole_words_not_substrings() {
        let w = Weights::default();
        let shortcut = item(1, "editor", "the save shortcut is ctrl s", 3, 0);
        assert_eq!(score(&shortcut, &terms("short"), 0, &w), None);
        let kalpana = item(2, "colleague", "kalpana leads the design team", 3, 0);
        assert_eq!(score(&kalpana, &terms("kal"), 0, &w), None);
        assert!(score(&kalpana, &terms("kalpana"), 0, &w).is_some());
    }

    #[test]
    fn key_words_and_possessives_match() {
        let w = Weights::default();
        let pref = item(1, "answer_length", "concise", 3, 0);
        assert_eq!(
            score(&pref, &terms("answer length"), 0, &w).map(|s| s.1),
            Some(2)
        );
        let asha = item(2, "birthday", "Asha's birthday is 14 March", 3, 0);
        assert_eq!(score(&asha, &terms("asha"), 0, &w).map(|s| s.1), Some(1));
    }

    #[test]
    fn score_follows_the_formula() {
        // Half the terms matched, 30 days old (recency 0.5), importance 5:
        // 0.6 * 0.5 + 0.25 * 0.5 + 0.15 * 1.0 = 0.575
        let it = item(1, "deadline", "kestrel draft due in week 16", 5, 0);
        let (s, matched) =
            score(&it, &terms("kestrel budget"), 30 * DAY, &Weights::default()).unwrap();
        assert_eq!(matched, 1);
        assert!((s - 0.575).abs() < 1e-12, "{s}");
    }

    #[test]
    fn ranking_is_by_score_then_id_and_needs_a_match() {
        let items = vec![
            item(3, "a", "budget review", 3, 0),
            item(1, "b", "budget review", 3, 0),
            item(2, "c", "budget review friday", 5, 0),
            item(4, "d", "groceries", 5, 0),
        ];
        let ids: Vec<i64> = rank(items, &terms("budget"), 0, 10, &Weights::default())
            .iter()
            .map(|s| s.item.id)
            .collect();
        assert_eq!(
            ids,
            [2, 1, 3],
            "importance first, then the tie by id; no match, no entry"
        );
        let one = rank(
            vec![item(1, "a", "budget", 3, 0)],
            &terms("budget"),
            0,
            0,
            &Weights::default(),
        );
        assert!(one.is_empty(), "limit 0 returns nothing");
    }

    #[test]
    fn bad_weights_are_rejected() {
        let d = Weights::default();
        for w in [
            Weights {
                half_life_days: 0.0,
                ..d
            },
            Weights {
                relevance: f64::NAN,
                ..d
            },
            Weights { recency: -1.0, ..d },
            Weights {
                importance: f64::INFINITY,
                ..d
            },
        ] {
            assert!(w.check().is_err(), "{w:?}");
        }
        assert!(d.check().is_ok());
    }
}
