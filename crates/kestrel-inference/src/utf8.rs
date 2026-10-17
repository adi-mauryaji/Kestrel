//! Incremental UTF-8 decoding of token pieces. A tokenizer may split one character across two
//! tokens (byte-level BPE does this for emoji and Devanagari), so a piece is not always valid
//! UTF-8 on its own. `Utf8Stream` holds back an incomplete tail until the next piece completes
//! it, and replaces bytes that can never be valid with U+FFFD instead of stalling.

#[derive(Debug, Default)]
pub struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `bytes` and returns all text that is now complete.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    out.push_str(text);
                    self.pending.clear();
                    return out;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    // The prefix is valid UTF-8, so the lossy conversion copies it unchanged.
                    out.push_str(&String::from_utf8_lossy(&self.pending[..valid]));
                    match e.error_len() {
                        // Incomplete sequence at the end: keep it for the next piece.
                        None => {
                            self.pending.drain(..valid);
                            return out;
                        }
                        // Bytes that no continuation can fix: replace them and go on.
                        Some(bad) => {
                            out.push('\u{FFFD}');
                            self.pending.drain(..valid + bad);
                        }
                    }
                }
            }
        }
    }

    /// Text left at the end of the stream; an incomplete tail becomes U+FFFD.
    pub fn finish(self) -> String {
        String::from_utf8_lossy(&self.pending).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passes_through() {
        let mut s = Utf8Stream::new();
        assert_eq!(s.push(b"hello "), "hello ");
        assert_eq!(s.push(b"world"), "world");
        assert_eq!(s.finish(), "");
    }

    #[test]
    fn a_character_split_across_pieces_waits_for_its_last_byte() {
        let emoji = "🙂".as_bytes(); // 4 bytes
        let mut s = Utf8Stream::new();
        assert_eq!(s.push(&emoji[..1]), "");
        assert_eq!(s.push(&emoji[1..3]), "");
        assert_eq!(s.push(&emoji[3..]), "🙂");
    }

    #[test]
    fn devanagari_split_mid_character() {
        let text = "नमस्ते".as_bytes();
        let mut s = Utf8Stream::new();
        let mut out = s.push(&text[..4]); // "न" + first byte of "म"
        assert_eq!(out, "न");
        out += &s.push(&text[4..]);
        assert_eq!(out, "नमस्ते");
    }

    #[test]
    fn invalid_bytes_become_replacement_characters_without_stalling() {
        let mut s = Utf8Stream::new();
        assert_eq!(s.push(b"a\xFFb"), "a\u{FFFD}b");
        assert_eq!(s.push(b"c"), "c");
    }

    #[test]
    fn finish_flushes_an_incomplete_tail() {
        let mut s = Utf8Stream::new();
        assert_eq!(s.push(&"é".as_bytes()[..1]), "");
        assert_eq!(s.finish(), "\u{FFFD}");
    }
}
