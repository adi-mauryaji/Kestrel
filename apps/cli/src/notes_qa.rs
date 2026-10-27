//! v3: a question answered from the user's notes (ADR-041). Gemma gets the closest notes (src/meaning.rs)
//! and the question, answers only from them in the question's language, or says exactly NOT IN NOTES.
//! The worker runs the grounding check (src/grounding.rs `check_facts`: the answer's numbers and
//! names must be in the notes; its other words may be Hinglish over English notes) and says why an answer is not grounded; the caller shows only a grounded answer, so the
//! answer is not streamed. Measured on Kestrel-NotesQA-v0 (experiments/v3/notes_qa.py): the `N`
//! closest notes by meaning, none when even the closest is below θ (src/meaning.rs `closest`).
//! The user sees the answer and the note it came from, the human's wording when the notes do not
//! say (ADR-042), or the closest note when the answer could not be checked.

use kestrel_tools::tools::Note;

use crate::kinds::Writer;

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

/// After a Hinglish question: Gemma answered half of them in English when only the system prompt
/// said so (Kestrel-NotesQA-v0 test, 177122b); the writer follows such a line better (src/kinds.rs).
/// Its second sentence came after dev: with the first alone, Gemma put NOT IN NOTES into Hinglish.
pub const HINGLISH_NOTE: &str = "The question is Hinglish: answer in Hinglish, in Latin letters. \
If the notes do not answer it, still reply exactly: NOT IN NOTES.";

/// The notes (name and text), then the question, and for a Hinglish question its language.
pub fn user_message(question: &str, notes: &[(&str, &str)]) -> String {
    let body: Vec<String> = (notes.iter())
        .map(|(name, text)| format!("[{name}]\n{}", text.trim_end()))
        .collect();
    let mut m = format!("Notes:\n\n{}\n\nQuestion: {question}", body.join("\n\n"));
    if crate::kinds::is_hinglish(question) {
        m.push_str("\n\n");
        m.push_str(HINGLISH_NOTE);
    }
    m
}

/// Did Gemma say the notes do not answer the question?
pub fn not_in_notes(answer: &str) -> bool {
    let a = answer.trim().to_lowercase();
    (a.trim_end_matches(['.', '!'])).starts_with(&NOT_IN_NOTES.to_lowercase())
}

/// The notes given to Gemma: the closest by meaning (fixed before the dev run).
pub const N: usize = 2;

/// What the user sees when the notes do not answer (the human's wording, ADR-042).
pub const NOT_IN_YOUR_NOTES: &str =
    "The notes do not say this: so the reply is not available for this information";

/// The worker's answer and, when it is not grounded, why.
pub struct NotesAnswer {
    pub text: String,
    pub ungrounded: Option<String>,
}

/// The lines for `question` answered from `notes` (the closest first) by `writer`.
pub fn answer(question: &str, notes: &[Note], writer: &dyn Writer) -> Vec<String> {
    let Some(closest) = notes.first() else {
        return vec![NOT_IN_YOUR_NOTES.into()];
    };
    let pairs: Vec<(&str, &str)> = (notes.iter())
        .map(|n| (n.name.as_str(), n.text.as_str()))
        .collect();
    match writer.answer(question, &pairs) {
        Err(e) => vec![format!("I couldn't answer from your notes: {e}")],
        Ok(a) if not_in_notes(&a.text) => vec![NOT_IN_YOUR_NOTES.into()],
        // The reason names the unchecked fact, so it is not shown.
        Ok(a) if a.ungrounded.is_some() => vec![format!(
            "I couldn't check an answer against your notes; the closest note is {}.",
            closest.name
        )],
        Ok(a) => vec![
            a.text.trim().to_string(),
            format!("(from {})", sources(&a.text, notes)),
        ],
    }
}

/// The note the answer's numbers and names all come from; the closest when both hold them, and
/// every note given when it took more than one.
fn sources(answer: &str, notes: &[Note]) -> String {
    let alone: Vec<&Note> = (notes.iter())
        .filter(|n| crate::grounding::check_facts(answer, &[n.text.as_str()]).is_ok())
        .collect();
    let named: Vec<&str> = match alone.as_slice() {
        [one] => vec![one.name.as_str()],
        [] => notes.iter().map(|n| n.name.as_str()).collect(),
        _ => vec![notes[0].name.as_str()],
    };
    named.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::KestrelError;

    #[test]
    fn the_notes_come_named_before_the_question() {
        let m = user_message(
            "how much is my rent?",
            &[(
                "2026-09-02-budget-plan.md",
                "Monthly budget\n\nRent 12000.\n",
            )],
        );
        assert_eq!(
            m,
            "Notes:\n\n[2026-09-02-budget-plan.md]\nMonthly budget\n\nRent 12000.\n\nQuestion: how much is my rent?"
        );
    }

    #[test]
    fn a_hinglish_question_is_followed_by_its_language_once_more() {
        let notes = [("budget.md", "Rent 12000.")];
        let m = user_message("mera rent kitna hai?", &notes);
        assert!(
            m.ends_with(&format!(
                "Question: mera rent kitna hai?\n\n{HINGLISH_NOTE}"
            )),
            "{m}"
        );
        assert!(!user_message("how much is my rent?", &notes).contains(HINGLISH_NOTE));
        assert!(HINGLISH_NOTE.ends_with(&format!("{NOT_IN_NOTES}.")));
    }

    #[test]
    fn the_instructions_ask_for_the_notes_only_and_the_exact_marker() {
        assert!(SYSTEM.contains(NOT_IN_NOTES) && SYSTEM.contains("only"));
    }

    fn note(name: &str, text: &str) -> Note {
        Note {
            name: name.into(),
            text: text.into(),
        }
    }

    /// A writer whose notes answer is `reply` (text, ungrounded), or fails.
    struct Says(Result<(&'static str, Option<&'static str>), ()>);

    impl Writer for Says {
        fn write(
            &self,
            _: crate::kinds::Kind,
            _: &str,
            _: &mut dyn FnMut(&str),
        ) -> Result<String, KestrelError> {
            unreachable!("a notes question is answered, not written")
        }
        fn answer(&self, _: &str, _: &[(&str, &str)]) -> Result<NotesAnswer, KestrelError> {
            (self.0)
                .map(|(text, why)| NotesAnswer {
                    text: text.into(),
                    ungrounded: why.map(String::from),
                })
                .map_err(|()| KestrelError::Invalid("worker gone".into()))
        }
    }

    fn two() -> Vec<Note> {
        vec![
            note(
                "budget.md",
                "Monthly budget\n\nRent 12000. Recurring deposit 5000.",
            ),
            note(
                "landlord.md",
                "Landlord\n\nMr. Sharma sent Raju the plumber.",
            ),
        ]
    }

    #[test]
    fn a_grounded_answer_is_shown_with_the_note_it_came_from() {
        let lines = answer(
            "mera rent kitna hai?",
            &two(),
            &Says(Ok(("Rent 12000 hai.", None))),
        );
        assert_eq!(lines, ["Rent 12000 hai.", "(from budget.md)"]);
        // A name is a capital inside a sentence (src/grounding.rs).
        let raju = Says(Ok(("The plumber was Raju.", None)));
        let lines = answer("who fixed the tap?", &two(), &raju);
        assert_eq!(lines, ["The plumber was Raju.", "(from landlord.md)"]);
        // Facts from both notes name both; facts in either name the closest.
        let both = Says(Ok(("Rent 12000, and Raju fixed it.", None)));
        assert_eq!(
            answer("q", &two(), &both)[1],
            "(from budget.md, landlord.md)"
        );
        let none = Says(Ok(("Yes, it is fixed.", None)));
        assert_eq!(answer("q", &two(), &none)[1], "(from budget.md)");
    }

    #[test]
    fn no_note_or_not_in_notes_gets_the_humans_wording() {
        let never = Says(Err(()));
        assert_eq!(answer("passport?", &[], &never), [NOT_IN_YOUR_NOTES]);
        let no = Says(Ok(("NOT IN NOTES", None)));
        assert_eq!(answer("passport?", &two(), &no), [NOT_IN_YOUR_NOTES]);
    }

    #[test]
    fn an_unchecked_answer_is_withheld_and_a_failure_said() {
        let wrong = Says(Ok((
            "Rent 15000.",
            Some("names or numbers not in the notes: 15000"),
        )));
        let lines = answer("rent?", &two(), &wrong);
        assert_eq!(
            lines,
            ["I couldn't check an answer against your notes; the closest note is budget.md."]
        );
        assert!(!lines[0].contains("15000"));
        assert_eq!(
            answer("rent?", &two(), &Says(Err(()))),
            ["I couldn't answer from your notes: invalid input: worker gone"]
        );
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
