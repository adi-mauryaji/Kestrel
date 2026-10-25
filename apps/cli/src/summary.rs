//! Summaries in the model's own words (ADR-034 feature 5). In the service, Gemma (the Tier-4
//! worker, src/remote.rs) writes the summary; `ModelText` keeps it only if it is grounded in the
//! notes (src/grounding.rs), and otherwise falls back to the deterministic first lines
//! (`MarkedItems`, still the research and test baseline) and says why. Action items stay
//! deterministic. The prompt is built here; the worker only adds the chat format.

use kestrel_core::KestrelError;
use kestrel_tools::plan::Data;
use kestrel_tools::runner::{MarkedItems, TextOps};

/// The instructions for a summary.
pub const SYSTEM: &str = "You summarize the user's notes in 1 to 3 short sentences, in your own \
words. Write in the language and script of the notes: English notes get an English summary; \
Hinglish notes (Hindi written in Latin letters, like \"kal plumber aayega\") get a Hinglish summary \
in Latin letters, not English and not Devanagari. Use only facts that are in the notes: no names, \
numbers, dates or places that the notes do not contain. Answer with the summary only.";
/// The notes sent are cut to this many characters (Gemma's context is 2048 tokens).
pub const MAX_CHARS: usize = 4000;
/// The longest summary, in tokens.
pub const MAX_TOKENS: usize = 160;

/// Writes a summary of note texts.
pub trait Summarizer {
    fn summarize(&self, texts: &[&str]) -> Result<String, KestrelError>;
}

/// The user's turn: the notes, separated, cut to `MAX_CHARS` in all.
pub fn user_message(texts: &[&str]) -> String {
    let mut m = String::from("Notes:\n\n");
    for (i, t) in texts.iter().enumerate() {
        let sep = if i == 0 { "" } else { "\n\n---\n\n" };
        let room = MAX_CHARS.saturating_sub(m.chars().count() + sep.len());
        m += sep;
        if t.chars().count() > room {
            m.extend(t.chars().take(room));
            m += "[…]";
            break;
        }
        m += t;
    }
    m
}

/// The texts of notes and text inputs, in input order.
fn texts<'d>(inputs: &[&'d Data]) -> Vec<&'d str> {
    inputs
        .iter()
        .flat_map(|d| match d {
            Data::Notes(notes) => notes.iter().map(|n| n.text.as_str()).collect(),
            Data::Text(t) => vec![t.as_str()],
            _ => vec![],
        })
        .collect()
}

/// Summaries from the model when there is one and its summary is grounded; else the first lines.
pub struct ModelText<'s> {
    model: Option<&'s dyn Summarizer>,
    /// Why the model's summary was not used (None when it was, or there is no model).
    pub fell_back: Option<String>,
}

impl<'s> ModelText<'s> {
    pub fn new(model: Option<&'s dyn Summarizer>) -> Self {
        Self {
            model,
            fell_back: None,
        }
    }
}

impl TextOps for ModelText<'_> {
    fn summarize(&mut self, inputs: &[&Data]) -> Result<String, KestrelError> {
        if let Some(model) = self.model {
            let texts = texts(inputs);
            let checked = model
                .summarize(&texts)
                .map(|s| s.trim().to_string())
                .map_err(|e| e.to_string())
                .and_then(|s| crate::grounding::check(&s, &texts).map(|()| s));
            match checked {
                Ok(s) => return Ok(s),
                Err(why) => self.fell_back = Some(why),
            }
        }
        MarkedItems.summarize(inputs)
    }

    fn extract_action_items(&mut self, inputs: &[&Data]) -> Result<Vec<String>, KestrelError> {
        MarkedItems.extract_action_items(inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_tools::tools::Note;

    struct Says(Result<&'static str, ()>);

    impl Summarizer for Says {
        fn summarize(&self, _: &[&str]) -> Result<String, KestrelError> {
            self.0
                .map(String::from)
                .map_err(|()| KestrelError::ModelUnavailable(kestrel_core::Tier::Generative))
        }
    }

    fn notes() -> Data {
        Data::Notes(vec![
            Note {
                name: "a.md".into(),
                text: "Standup.\n- budget review moved to Friday".into(),
            },
            Note {
                name: "b.md".into(),
                text: "Planning with Asha.\n- [ ] send the report".into(),
            },
        ])
    }

    #[test]
    fn a_grounded_summary_is_used() {
        let model = Says(Ok(
            "The budget review moved to Friday; Asha will send the report.",
        ));
        let mut t = ModelText::new(Some(&model));
        assert_eq!(
            t.summarize(&[&notes()]).unwrap(),
            "The budget review moved to Friday; Asha will send the report."
        );
        assert_eq!(t.fell_back, None);
    }

    #[test]
    fn otherwise_the_first_lines_and_why() {
        let model = Says(Ok("On Monday, Ravi moved the budget review."));
        let mut t = ModelText::new(Some(&model));
        assert_eq!(
            t.summarize(&[&notes()]).unwrap(),
            "Standup. Planning with Asha."
        );
        assert!(
            t.fell_back.as_deref().unwrap().contains("Ravi"),
            "{:?}",
            t.fell_back
        );

        let down = Says(Err(()));
        let mut t = ModelText::new(Some(&down));
        assert_eq!(
            t.summarize(&[&notes()]).unwrap(),
            "Standup. Planning with Asha."
        );
        assert!(t.fell_back.is_some());

        let mut t = ModelText::new(None);
        assert_eq!(
            t.summarize(&[&notes()]).unwrap(),
            "Standup. Planning with Asha."
        );
        assert_eq!(t.fell_back, None, "no model is not a fallback");
    }

    #[test]
    fn the_notes_are_separated_and_cut_to_fit() {
        assert_eq!(user_message(&["one", "two"]), "Notes:\n\none\n\n---\n\ntwo");
        let long = "x".repeat(MAX_CHARS * 2);
        let m = user_message(&[&long, "never reached"]);
        assert!(m.chars().count() <= MAX_CHARS + 20, "{}", m.len());
        assert!(m.ends_with("[…]"), "the cut is marked");
        assert!(!m.contains("never reached"));
    }
}
