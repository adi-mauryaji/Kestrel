//! Is a model's summary grounded in the notes it summarizes? (ADR-034 feature 5; blueprint risk
//! table: "quote-grounding validator ... abstain when ungrounded".) A summary in the model's own
//! words may use words the notes do not, but never a fact they lack: every number and every name
//! (a capitalized word inside a sentence) must be in the notes, and at least `MIN_SUPPORT` of its
//! other content words. A word is in the notes when it is there itself or shares a long enough
//! start with a word that is ("discussed" / "discussion", "move" / "moved"). Deterministic, no
//! model; en + Romanized Hinglish stopwords are not counted.

/// The least share of a summary's content words (not numbers, not names) the notes must support.
pub const MIN_SUPPORT: f64 = 0.6;

/// Not counted as content: function words (en + Hinglish) and words about the request itself.
const STOP: &[&str] = &[
    "the", "and", "but", "for", "with", "from", "are", "was", "were", "been", "being", "has",
    "have", "had", "will", "would", "shall", "should", "can", "could", "may", "might", "must",
    "does", "did", "this", "that", "these", "those", "its", "they", "them", "their", "there",
    "then", "than", "also", "about", "into", "over", "after", "before", "all", "any", "some",
    "someone", "more", "most", "other", "such", "not", "you", "your", "our", "she", "his", "her",
    "him", "which", "who", "whom", "what", "when", "where", "how", "why", "while", "each", "both",
    "only", "just", "very", "out", "own", "same", "too", "here", "now", "note", "notes", "summary",
    "hai", "hain", "tha", "thi", "mein", "par", "aur", "bhi", "yeh", "woh", "kuch", "sab", "nahi",
    "nahin", "liye", "karna", "kiya", "kar", "raha", "rahi", "rahe", "hua", "hui", "hue", "gaya",
    "gayi", "gaye", "toh", "jab", "tab", "abhi",
];

/// Ok, or why not: the numbers and names the notes lack, or the share of content words they
/// support.
pub fn check(summary: &str, sources: &[&str]) -> Result<(), String> {
    let (missing, content, supported) = scan(summary, sources);
    facts_in_notes(missing)?;
    if content == 0 {
        return Err("no content words".into());
    }
    if (supported as f64) < MIN_SUPPORT * content as f64 {
        return Err(format!(
            "only {supported} of {content} content words are in the notes"
        ));
    }
    Ok(())
}

/// Ok, or the numbers and names `answer` has that the notes lack: the check for an answer from the
/// notes (src/notes_qa.rs), which may be in Hinglish over English notes and so use other words.
pub fn check_facts(answer: &str, sources: &[&str]) -> Result<(), String> {
    facts_in_notes(scan(answer, sources).0)
}

fn facts_in_notes(missing: Vec<String>) -> Result<(), String> {
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "names or numbers not in the notes: {}",
        missing.join(", ")
    ))
}

/// (the numbers and names of `text` the notes lack, its content words, how many the notes support)
fn scan(text: &str, sources: &[&str]) -> (Vec<String>, usize, usize) {
    let source: Vec<String> = sources
        .iter()
        .flat_map(|s| tokens(s))
        .map(|(w, _)| w.to_lowercase())
        .collect();
    let known = |w: &str| source.iter().any(|s| supports(s, w));
    let mut missing: Vec<String> = Vec::new();
    let (mut content, mut supported) = (0usize, 0usize);
    for (w, starts_sentence) in tokens(text) {
        let low = w.to_lowercase();
        let number = w.chars().all(|c| c.is_ascii_digit());
        let name = !starts_sentence && w != "I" && w.starts_with(char::is_uppercase);
        if number || name {
            let found = if number {
                source.contains(&low)
            } else {
                known(&low)
            };
            if !found && !missing.contains(&w) {
                missing.push(w);
            }
        } else if low.chars().count() >= 3 && !STOP.contains(&low.as_str()) {
            content += 1;
            supported += usize::from(known(&low));
        }
    }
    (missing, content, supported)
}

/// The words of `text` (letters and digits; split where they meet, so "3pm" is "3", "pm"), each
/// with whether it starts a sentence (after . ! ? : or a line break).
fn tokens(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut word = String::new();
    let (mut sentence_start, mut word_start) = (true, true);
    for c in text.chars().chain([' ']) {
        let same_kind = word
            .chars()
            .last()
            .is_none_or(|p| p.is_ascii_digit() == c.is_ascii_digit());
        if c.is_alphanumeric() && same_kind {
            if word.is_empty() {
                word_start = sentence_start;
            }
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            out.push((std::mem::take(&mut word), word_start));
            sentence_start = false;
        }
        if c.is_alphanumeric() {
            // A digit meets a letter: a new word in the same sentence.
            word_start = false;
            word.push(c);
        } else if matches!(c, '.' | '!' | '?' | ':' | '\n') {
            sentence_start = true;
        }
    }
    out
}

/// "moving" / "moved" / "move" -> "mov"; "hiring" / "hired" -> "hir".
fn stem(w: &str) -> &str {
    let long = |r: &str| r.chars().count() >= 3;
    let mut s = w;
    if let Some(r) = ["ing", "ed", "es", "s"]
        .iter()
        .find_map(|suf| s.strip_suffix(suf).filter(|r| long(r)))
    {
        s = r;
    }
    if let Some(r) = s.strip_suffix('e').filter(|r| long(r)) {
        s = r;
    }
    s
}

/// Whether a word of the notes stands for `w`: the same stem, or the same first five letters
/// ("discussed" / "discussion").
fn supports(source: &str, w: &str) -> bool {
    let shared = source
        .chars()
        .zip(w.chars())
        .take_while(|(a, b)| a == b)
        .count();
    stem(source) == stem(w) || shared >= 5
}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDUP: &str = "tags: meeting\nStandup with Asha and Ravi.\n- budget review moved to Friday\n- [ ] send the report by 3 pm\nDiscussion: hiring two interns.";

    #[test]
    fn a_faithful_summary_in_other_words_passes() {
        for s in [
            "At the standup, Asha and Ravi discussed moving the budget review to Friday; the report is due by 3 pm.",
            "The budget review moves to Friday. Someone must send the report, and two interns may be hired.",
            "Standup: budget review shifted to Friday, report to send.",
        ] {
            assert_eq!(check(s, &[STANDUP]), Ok(()), "{s}");
        }
    }

    #[test]
    fn a_number_or_name_the_notes_lack_fails() {
        let e = check(
            "Asha and Priya moved the budget review to Friday.",
            &[STANDUP],
        )
        .unwrap_err();
        assert!(e.contains("Priya"), "{e}");
        let e = check("The report is due by 5 pm on Friday.", &[STANDUP]).unwrap_err();
        assert!(e.contains('5'), "{e}");
        let e = check("Ravi will hire three interns by Monday.", &[STANDUP]).unwrap_err();
        assert!(e.contains("Monday"), "{e}");
    }

    #[test]
    fn mostly_new_words_fail_even_without_names() {
        let e = check(
            "The quarterly marketing strategy needs urgent approval from leadership.",
            &[STANDUP],
        )
        .unwrap_err();
        assert!(e.contains("content words"), "{e}");
        assert!(
            check("", &[STANDUP]).is_err(),
            "an empty summary is not a summary"
        );
    }

    /// Gemma-4-E2B's own summaries (live, 2026-10-25): faithful ones pass; its English translation
    /// of a Hinglish note cannot be checked word for word, so it does not.
    #[test]
    fn gemmas_real_summaries() {
        let ghar = "Ghar ka kaam\nKal plumber aayega subah 10 baje.\n- [ ] bijli ka bill bharna hai\nMaa ne kaha Diwali ki shopping Saturday ko karenge.\n";
        let standup = "tags: meeting\nStandup with Asha and Ravi.\n- budget review moved to Friday\n- [ ] Asha to send the vendor report by 3 pm\nRavi is hiring two interns for the data team.\n";
        assert_eq!(
            check(
                "Kal subah 10 baje plumber aayega. Bijli ka bill bharna hai aur Diwali ki shopping Saturday ko karne ka plan hai.",
                &[ghar]
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "A standup meeting was held with Asha and Ravi. The budget review has been moved to Friday, and Asha is responsible for sending the vendor report by 3 pm. Ravi is hiring two interns for the data team.",
                &[standup]
            ),
            Ok(())
        );
        assert!(
            check(
                "The plumber is scheduled to arrive in the morning at 10 o'clock. The mother mentioned that Diwali shopping will take place on Saturday.",
                &[ghar]
            )
            .is_err()
        );
    }

    #[test]
    fn hinglish_notes_and_several_sources() {
        let a = "kal ki meeting mein budget pe baat hui\n- [ ] Asha ko report bhejni hai";
        let b = "Ravi ne interns ke liye 2 naam diye";
        assert_eq!(
            check(
                "Budget pe baat hui, Asha ko report bhejni hai; Ravi ne 2 interns ke naam diye.",
                &[a, b]
            ),
            Ok(())
        );
        assert!(check("Budget meeting Sunil ke saath hui.", &[a, b]).is_err());
    }

    #[test]
    fn an_answer_in_hinglish_keeps_its_facts_to_the_notes() {
        // Notes Q&A (src/notes_qa.rs): the answer may be in Hinglish over English notes, so only its
        // numbers and names are held to the notes.
        let notes = ["Monthly budget\n\nRent 12000, groceries about 6000."];
        assert_eq!(check_facts("Mera rent 12000 hai.", &notes), Ok(()));
        assert!(
            check("Mera rent 12000 hai.", &notes).is_err(),
            "the summary rule would refuse it"
        );
        assert_eq!(
            check_facts("Mera rent 15000 hai.", &notes),
            Err("names or numbers not in the notes: 15000".into())
        );
        assert!(check_facts("Rent is paid to Sharma ji.", &notes).is_err());
    }
}
