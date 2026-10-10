//! Text normalizer: the first step every request passes through (blueprint Ch. 9, A.1).
//! Deterministic, so Tier 1 rules and the experiments see the same text for the same input.

/// Lowercase, collapse runs of whitespace to one space,
/// strip trailing sentence punctuation (. ! ?).
pub fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let collapsed = lowered.split_whitespace().collect::<Vec<_>>().join(" ");
    // Strip punctuation and whitespace together: "call asha !" must not keep the space
    // before the "!" (the blueprint's A.1 version leaves "call asha ").
    collapsed
        .trim_end_matches(|c: char| matches!(c, '.' | '!' | '?') || c.is_whitespace())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_and_collapses_whitespace() {
        assert_eq!(
            normalize("  Remind   me TO call Asha "),
            "remind me to call asha"
        );
    }

    #[test]
    fn strips_trailing_sentence_punctuation() {
        assert_eq!(normalize("call asha!!"), "call asha");
        assert_eq!(normalize("what's next?"), "what's next");
    }

    #[test]
    fn keeps_inner_punctuation() {
        assert_eq!(normalize("meet at 8.30 tomorrow."), "meet at 8.30 tomorrow");
    }

    #[test]
    fn no_trailing_space_left_after_stripping() {
        assert_eq!(normalize("call asha !"), "call asha");
    }

    #[test]
    fn strips_mixed_trailing_punctuation_and_spaces() {
        assert_eq!(normalize("call asha ! ?"), "call asha");
    }

    #[test]
    fn hinglish_passes_through_unchanged_except_case() {
        assert_eq!(
            normalize("Kal 8 baje Python class yaad dilana"),
            "kal 8 baje python class yaad dilana"
        );
    }

    #[test]
    fn whitespace_only_becomes_empty() {
        assert_eq!(normalize("   "), "");
    }
}
