//! v3: a question answered from the user's notes (ADR-041). Gemma gets the closest notes (src/meaning.rs)
//! and the question, answers only from them in the question's language, or says exactly NOT IN NOTES.
//! The worker runs the grounding check (src/grounding.rs `check_facts`: the answer's numbers and
//! names must be in the notes; its other words may be Hinglish over English notes) and says why an answer is not grounded; the caller shows only a grounded answer, so the
//! answer is not streamed. Measured on Kestrel-NotesQA-v0 (experiments/v3/notes_qa.py).

/// Gemma's instructions (the worker adds the chat format).
pub const SYSTEM: &str = "You answer the user's question using only their notes, given below the word \
Notes. Answer in one or two short sentences, in the language and script of the question: English for \
English, Hinglish (Hindi in Latin letters) for Hinglish. Use the notes' own numbers, names and dates, \
and add nothing they do not say. If the notes do not state the answer itself, reply exactly: NOT IN \
NOTES, even when they mention the topic.";

/// What Gemma writes when the notes do not answer the question.
pub const NOT_IN_NOTES: &str = "NOT IN NOTES";

/// The longest answer, in tokens: one or two sentences.
pub const MAX_TOKENS: usize = 96;

/// The notes (name and text), then the question.
pub fn user_message(question: &str, notes: &[(&str, &str)]) -> String {
    let body: Vec<String> = (notes.iter())
        .map(|(name, text)| format!("[{name}]\n{}", text.trim_end()))
        .collect();
    format!("Notes:\n\n{}\n\nQuestion: {question}", body.join("\n\n"))
}

/// Did Gemma say the notes do not answer the question?
pub fn not_in_notes(answer: &str) -> bool {
    let a = answer.trim().to_lowercase();
    (a.trim_end_matches(['.', '!'])).starts_with(&NOT_IN_NOTES.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notes_come_named_before_the_question() {
        let m = user_message(
            "mera rent kitna hai?",
            &[(
                "2026-09-02-budget-plan.md",
                "Monthly budget\n\nRent 12000.\n",
            )],
        );
        assert_eq!(
            m,
            "Notes:\n\n[2026-09-02-budget-plan.md]\nMonthly budget\n\nRent 12000.\n\nQuestion: mera rent kitna hai?"
        );
    }

    #[test]
    fn the_instructions_ask_for_the_notes_only_and_the_exact_marker() {
        assert!(SYSTEM.contains(NOT_IN_NOTES) && SYSTEM.contains("only"));
    }

    #[test]
    fn not_in_notes_is_told_however_it_is_written() {
        assert!(not_in_notes("NOT IN NOTES"));
        assert!(not_in_notes("Not in notes."));
        assert!(not_in_notes("  not in notes"));
        assert!(!not_in_notes("Your rent is 12000."));
        assert!(!not_in_notes(
            "The notes say the rent is 12000, not in cash."
        ));
    }
}
