//! A written answer shown while Gemma writes it (v3, ADR-036 (4)). The generator hands over pieces
//! of text; the user should see exactly the answer the whole text gives: no leading or trailing
//! space and no stop string (llama.rs passes the piece that completes a stop on). `Trimmed` holds
//! back what may still turn out to be one of those, so the pieces it lets out, joined, are the
//! trimmed answer.

pub struct Trimmed {
    stop: String,
    held: String,
    started: bool,
}

impl Trimmed {
    pub fn new(stop: &str) -> Self {
        Self {
            stop: stop.into(),
            held: String::new(),
            started: false,
        }
    }

    /// The text that can be shown now that `piece` came.
    pub fn push(&mut self, piece: &str) -> String {
        self.held.push_str(piece);
        if !self.started {
            let text = self.held.trim_start();
            if text.is_empty() {
                self.held.clear();
                return String::new();
            }
            self.held = text.to_string();
            self.started = true;
        }
        // Held: a stop and all after it, the start of a stop at the end, and the space before either.
        let stop_at = (!self.stop.is_empty())
            .then(|| self.held.find(&self.stop))
            .flatten();
        let cut = stop_at.unwrap_or(self.held.len() - self.stop_begun());
        let cut = self.held[..cut].trim_end().len();
        self.held.drain(..cut).collect()
    }

    /// The rest of the answer once the generator has stopped.
    pub fn finish(self) -> String {
        let stop = self.stop.as_str();
        self.held
            .trim_end()
            .trim_end_matches(stop)
            .trim_end()
            .to_string()
    }

    /// The length of the longest end of `held` that a stop could still grow from.
    fn stop_begun(&self) -> usize {
        let n = self.held.len();
        (1..self.stop.len().min(n + 1))
            .rev()
            .find(|&k| {
                self.held.is_char_boundary(n - k) && self.stop.starts_with(&self.held[n - k..])
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answer as the worker trimmed it before streaming (service.rs `answer`).
    fn whole(text: &str, stop: &str) -> String {
        text.trim_end().trim_end_matches(stop).trim().to_string()
    }

    fn streamed(pieces: &[&str], stop: &str) -> (Vec<String>, String) {
        let mut t = Trimmed::new(stop);
        let shown: Vec<String> = pieces.iter().map(|p| t.push(p)).collect();
        (shown, t.finish())
    }

    #[test]
    fn the_pieces_join_to_the_trimmed_answer_however_the_text_is_cut() {
        let stop = "<end_of_turn>";
        let texts = [
            "Hello there.",
            "  \nSubject: Leave\n\nDear Asha,\n\nI am unwell.  \n",
            "Done.<end_of_turn>",
            "Done. \n<end_of_turn>\n",
            "a <end_of_turn><end_of_turn>",
            "x <end_of",
            "a < b, and <end_or not>",
            "मुझे कल देर हो जाएगी\n",
            "   ",
            "",
        ];
        for text in texts {
            let want = whole(text, stop);
            let chars: Vec<(usize, char)> = text.char_indices().collect();
            // Every cut into two pieces, and the text one character at a time.
            for &(i, _) in &chars {
                let (shown, rest) = streamed(&[&text[..i], &text[i..]], stop);
                assert_eq!(shown.concat() + &rest, want, "{text:?} cut at {i}");
            }
            let singles: Vec<String> = text.chars().map(String::from).collect();
            let singles: Vec<&str> = singles.iter().map(String::as_str).collect();
            let (shown, rest) = streamed(&singles, stop);
            assert_eq!(shown.concat() + &rest, want, "{text:?} by characters");
        }
    }

    #[test]
    fn text_is_shown_as_soon_as_it_cannot_be_trimmed_away() {
        let mut t = Trimmed::new("<end_of_turn>");
        assert_eq!(t.push("\n"), "");
        assert_eq!(t.push("Dear Asha,"), "Dear Asha,");
        assert_eq!(t.push("\n\n"), "", "space waits for what follows it");
        assert_eq!(t.push("I am"), "\n\nI am");
        assert_eq!(t.push(" <end_"), "", "may be the stop");
        assert_eq!(t.push("of_turn>"), "");
        assert_eq!(t.finish(), "");
    }
}
