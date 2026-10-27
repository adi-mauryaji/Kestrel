//! v3: a request's text saved as a new note (the human, 2026-10-27: "add this to notes" after a long text
//! was refused as "several steps"). A rule on the text as typed (its case kept), before the kind head and
//! the cascade: an instruction at the end ("... add this to notes", "... notes mein save kar do") or at the
//! start before ":" ("add to notes: ..."). The note is the text itself, in a new file
//! `YYYY-MM-DD-<first words>.md` in the notes folder, after the user's yes; an existing file is never
//! written over. Research code is untouched: kestrel-tools only reads notes.

use std::io::Write;
use std::path::Path;

/// What a save request asks for: the text to save, or nothing given.
#[derive(Debug, PartialEq)]
pub enum Asked {
    Save(String),
    Empty,
}

/// The instructions, lowercase, without their trailing words of politeness.
fn instructions() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for verb in ["add", "save", "put"] {
        for obj in ["this", "it", ""] {
            for place in [
                "to notes",
                "to my notes",
                "in notes",
                "in my notes",
                "into my notes",
                "as a note",
            ] {
                if verb == "put" && place.starts_with("to ") {
                    continue;
                }
                out.push(
                    format!("{verb} {obj} {place}")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }
    }
    for obj in ["", "isse ", "ise ", "isko ", "ye ", "yeh "] {
        for prep in ["me", "mein", "main"] {
            for verb in ["add", "save", "daal", "likh", "note"] {
                for end in [
                    "",
                    " karo",
                    " kar do",
                    " kardo",
                    " kar dena",
                    " do",
                    " dena",
                    " lo",
                    " kar lo",
                ] {
                    out.push(format!("{obj}notes {prep} {verb}{end}"));
                }
            }
        }
    }
    // Longest first, so "add this to my notes" wins over "add to notes" inside it.
    out.sort_by_key(|s| std::cmp::Reverse(s.len()));
    out.dedup();
    out
}

/// The request as a save, or None. Matching ignores case, spacing and end punctuation; the saved
/// text keeps the user's own. Text of fewer than three words is no text ("can you add this to notes").
pub fn parse(text: &str) -> Option<Asked> {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let squeezed = |s: &str| -> String {
        let s = s.trim_end_matches(|c: char| END.contains(c));
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    for ins in instructions() {
        let n = ins.split_whitespace().count();
        // At the end: "<text> add this to notes", also "<text>.add this to notes".
        if let Some(cut) = start_of_last_words(trimmed, n)
            && squeezed(&lower[cut..]) == ins
        {
            return Some(body(&trimmed[..cut]));
        }
        // At the start, before ":", "-" or a new line.
        if let Some(after) = end_of_first_words(trimmed, n)
            && squeezed(&lower[..after]) == ins
        {
            let rest = trimmed[after..].trim_start_matches([' ', '\t']);
            if let Some(r) = rest.strip_prefix([':', '-', '\n']) {
                return Some(body(r));
            }
        }
    }
    None
}

/// Punctuation that may end a request or glue the instruction to the text.
const END: &str = ".!?,;: ";

fn body(text: &str) -> Asked {
    let t = text.trim_matches(|c: char| c.is_whitespace() || ":-,;".contains(c));
    if t.split_whitespace().count() < 3 {
        Asked::Empty
    } else {
        Asked::Save(t.to_string())
    }
}

/// The byte index where the last `n` words of `text` begin, punctuation counting as a word break.
fn start_of_last_words(text: &str, n: usize) -> Option<usize> {
    let core = text.trim_end_matches(|c: char| END.contains(c));
    let (mut seen, mut in_word) = (0, false);
    for (i, c) in core.char_indices().rev() {
        let sep = c.is_whitespace() || END.contains(c);
        if sep && in_word {
            seen += 1;
            if seen == n {
                return Some(i + c.len_utf8());
            }
        }
        in_word = !sep;
    }
    (in_word && seen + 1 == n).then_some(0)
}

/// The byte index just after the first `n` whitespace-separated words of `text`.
fn end_of_first_words(text: &str, n: usize) -> Option<usize> {
    let (mut seen, mut in_word) = (0, false);
    for (i, c) in text.char_indices() {
        let sep = c.is_whitespace() || ":-".contains(c);
        if sep && in_word {
            seen += 1;
            if seen == n {
                return Some(i);
            }
        }
        in_word = !sep;
    }
    (in_word && seen + 1 == n).then_some(text.len())
}

/// Up to the first eight words of the text's first sentence, as a file name part ("note" if none).
pub fn slug(text: &str) -> String {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let sentence = first.split(['.', '!', '?']).next().unwrap_or("");
    let mut s = String::new();
    let words = sentence
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .take(8);
    // Whole words only, at most 64 characters.
    for w in words {
        if s.len() + w.len() + 1 > 64 {
            break;
        }
        if !s.is_empty() {
            s.push('-');
        }
        s.push_str(&w);
    }
    if s.is_empty() { "note".into() } else { s }
}

/// A free file name for today's note: `date-slug.md`, else `date-slug-2.md`, ...
pub fn file_name(date: &str, text: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let base = format!("{date}-{}", slug(text));
    (1..)
        .map(|n| {
            if n == 1 {
                format!("{base}.md")
            } else {
                format!("{base}-{n}.md")
            }
        })
        .find(|name| !taken(name))
        .unwrap_or_default()
}

/// The note written as a new file in `folder` (never over an existing one).
pub fn write(folder: &Path, name: &str, text: &str) -> std::io::Result<()> {
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(std::io::Error::other("not a plain file name"));
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(folder.join(name))?;
    f.write_all(text.trim_end().as_bytes())?;
    f.write_all(b"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(t: &str) -> Option<String> {
        match parse(t) {
            Some(Asked::Save(s)) => Some(s),
            _ => None,
        }
    }

    #[test]
    fn the_humans_request_saves_the_text_before_the_instruction() {
        let t = "The provided sources cover **machine learning** algorithms. Together they explain \
                 pipelines.add this to notes";
        assert_eq!(
            saved(t).as_deref(),
            Some(
                "The provided sources cover **machine learning** algorithms. Together they explain pipelines."
            )
        );
    }

    #[test]
    fn instructions_at_the_end_in_english_and_hinglish() {
        assert_eq!(
            saved("Buy milk on Friday. Add this to my notes!").as_deref(),
            Some("Buy milk on Friday.")
        );
        assert_eq!(
            saved("buy milk on friday, save it as a note").as_deref(),
            Some("buy milk on friday")
        );
        assert_eq!(
            saved("Ravi ka number 98765 hai, isse notes mein save kar do").as_deref(),
            Some("Ravi ka number 98765 hai")
        );
        assert_eq!(
            saved("gym monday wednesday friday notes me add karo").as_deref(),
            Some("gym monday wednesday friday")
        );
    }

    #[test]
    fn instructions_at_the_start_before_a_colon_or_a_new_line() {
        assert_eq!(
            saved("add to notes: Wifi password is on the router").as_deref(),
            Some("Wifi password is on the router")
        );
        assert_eq!(
            saved("Save this as a note:\nline one\nline two").as_deref(),
            Some("line one\nline two")
        );
        assert_eq!(
            saved("notes mein save karo - kal 5 baje meeting").as_deref(),
            Some("kal 5 baje meeting")
        );
    }

    #[test]
    fn the_instruction_alone_asks_for_the_text_and_other_notes_requests_pass() {
        assert_eq!(parse("add this to notes"), Some(Asked::Empty));
        assert_eq!(parse("Add to my notes."), Some(Asked::Empty));
        for t in [
            "search my notes for budget",
            "summarize my notes",
            "what do my notes say about rent?",
            "add a task to call asha",
            "notes mein kya likha hai rent ke baare mein?",
            "remind me to add this to notes later tomorrow",
        ] {
            assert_eq!(parse(t), None, "{t}");
        }
    }

    #[test]
    fn the_file_name_is_the_date_and_the_first_words_and_never_taken() {
        let t = "The provided sources cover **essential machine learning** algorithms and more. Second.";
        assert_eq!(
            slug(t),
            "the-provided-sources-cover-essential-machine-learning-algorithms"
        );
        assert_eq!(slug("कल मिलते हैं"), "note");
        let taken = |n: &str| n == "2026-10-27-buy-milk.md";
        assert_eq!(
            file_name("2026-10-27", "Buy milk!", &taken),
            "2026-10-27-buy-milk-2.md"
        );
    }

    #[test]
    fn writing_never_replaces_a_note_or_leaves_the_folder() {
        let d = std::env::temp_dir().join(format!("kestrel_save_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        write(&d, "2026-10-27-a.md", "hello\n\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("2026-10-27-a.md")).unwrap(),
            "hello\n"
        );
        assert!(write(&d, "2026-10-27-a.md", "other").is_err());
        assert!(write(&d, "../x.md", "x").is_err() && write(&d, ".hidden.md", "x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
