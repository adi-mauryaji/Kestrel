//! Request features read by more than one component (blueprint Ch. 13.1): Tier-1 rules match on
//! `tokens`, and the router reads `has_conjunction` as its multi-intent signal (ADR-017). Kept
//! here so both see the same tokenization.

/// Words that join two requests ("summarize my notes and create a task"), English and Hinglish.
pub const CONJUNCTIONS: &[&str] = &["and", "then", "aur", "phir", "fir"];

/// Split normalized text into word tokens; apostrophes stay inside words ("don't").
pub fn tokens(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|t| !t.is_empty())
        .collect()
}

/// Whether normalized text contains a conjunction token: the router's multi-intent signal.
pub fn has_conjunction(text: &str) -> bool {
    tokens(text).iter().any(|t| CONJUNCTIONS.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::normalize;

    #[test]
    fn tokens_split_on_punctuation_and_keep_apostrophes() {
        assert_eq!(
            tokens("notes, please: don't open todo.md"),
            vec!["notes", "please", "don't", "open", "todo", "md"]
        );
        assert!(tokens("  ,. ").is_empty());
    }

    #[test]
    fn conjunction_flag_is_on_whole_tokens_in_either_language() {
        for text in [
            "summarize my notes and create a task",
            "ideas.md padho phir task add karo",
            "notes dhundo aur summary do",
            "open todo.md, then read it",
        ] {
            assert!(has_conjunction(&normalize(text)), "{text:?}");
        }
        for text in ["find notes about android", "standup notes kholo", "thanks"] {
            assert!(!has_conjunction(&normalize(text)), "{text:?}");
        }
    }
}
