//! Notes are files under the sandbox roots, shared by every backend (moved out of backend.rs in
//! Week 10 so the fixture and the memory backend read notes the same way).
//!
//! `*.md` / `*.txt` files, named `YYYY-MM-DD-<slug>.md` so "recent" is the date prefix
//! (deterministic, unlike file times); an optional first line `tags: a, b` tags a note. Every file
//! is reached through `Sandbox::resolve`, so a symlink out of the notes folder is skipped.
//! Search is lexical: a note matches when it contains every query term (case-insensitive), ranked
//! by how often the terms occur, then most recent first.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kestrel_core::KestrelError;

use crate::args::iso_date;
use crate::sandbox::Sandbox;
use crate::tools::Note;

/// Largest note or document read, in bytes.
pub const MAX_DOC_BYTES: u64 = 64 * 1024;

pub struct Notes {
    sandbox: Sandbox,
}

impl Notes {
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }

    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    /// The `n` most recent dated notes (`YYYY-MM-DD-...`, by name), optionally only those tagged
    /// `tag`. Undated documents are never "recent", but search and read_document still find them.
    pub fn recent(&self, n: u32, tag: Option<&str>) -> Result<Vec<Note>, KestrelError> {
        let tag = tag.map(str::to_lowercase);
        let dated = |e: &Entry| {
            let f = e.file_name.as_str();
            f.get(..10).is_some_and(|d| iso_date(d).is_ok()) && f.get(10..11) == Some("-")
        };
        let mut notes: Vec<Entry> = self
            .notes()
            .into_iter()
            .filter(|e| dated(e) && tag.as_ref().is_none_or(|t| e.tags.contains(t)))
            .collect();
        notes.sort_by(|a, b| b.newest_first_key().cmp(&a.newest_first_key()));
        Ok(notes.into_iter().take(n as usize).map(|e| e.note).collect())
    }

    /// Every readable note under the roots, through the sandbox (symlink escapes are skipped,
    /// loops are visited once).
    fn notes(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for root in self.sandbox.roots() {
            self.walk(root, root, &mut seen, &mut out);
        }
        out
    }

    fn walk(&self, root: &Path, dir: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<Entry>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            let Ok(real) = self.sandbox.resolve(&p.to_string_lossy()) else {
                continue;
            };
            if !seen.insert(real.clone()) {
                continue;
            }
            if real.is_dir() {
                self.walk(root, &real, seen, out);
            } else if is_note(&real)
                && let Ok(note) = read_note(&self.sandbox, &real)
            {
                out.push(Entry::new(note, root, &real));
            }
        }
    }

    pub fn search(&self, query: &str, limit: u32) -> Vec<Note> {
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut hits: Vec<(usize, Entry)> = self
            .notes()
            .into_iter()
            .filter_map(|e| {
                let text = e.note.text.to_lowercase();
                let counts: Vec<usize> = terms
                    .iter()
                    .map(|t| text.matches(t.as_str()).count())
                    .collect();
                counts
                    .iter()
                    .all(|&c| c > 0)
                    .then(|| (counts.iter().sum(), e))
            })
            .collect();
        hits.sort_by(|(ca, a), (cb, b)| {
            cb.cmp(ca)
                .then_with(|| b.newest_first_key().cmp(&a.newest_first_key()))
        });
        hits.into_iter()
            .take(limit as usize)
            .map(|(_, e)| e.note)
            .collect()
    }

    /// Every note, for the page's Notes view: dated ones newest first, then the rest by name.
    pub fn all(&self) -> Vec<Note> {
        let dated = |e: &Entry| {
            let f = e.file_name.as_str();
            f.get(..10).is_some_and(|d| iso_date(d).is_ok()) && f.get(10..11) == Some("-")
        };
        let (mut with_date, mut rest): (Vec<Entry>, Vec<Entry>) =
            self.notes().into_iter().partition(|e| dated(e));
        with_date.sort_by(|a, b| b.newest_first_key().cmp(&a.newest_first_key()));
        rest.sort_by(|a, b| a.note.name.cmp(&b.note.name));
        with_date.into_iter().chain(rest).map(|e| e.note).collect()
    }

    /// The note listed under `name` (a path inside the notes folder, as `all` names it); nothing
    /// outside the list is read.
    pub fn by_name(&self, name: &str) -> Result<Note, KestrelError> {
        self.notes()
            .into_iter()
            .find(|e| e.note.name == name)
            .map(|e| e.note)
            .ok_or_else(|| KestrelError::Invalid(format!("no note named {name}")))
    }

    /// One document by its validated path (read_document).
    pub fn read(&self, path: &Path) -> Result<Note, KestrelError> {
        let root = self.sandbox.roots().iter().find(|r| path.starts_with(r));
        let root = root
            .ok_or_else(|| KestrelError::Sandbox("path escapes the approved directories".into()))?;
        let note = read_note(&self.sandbox, path)?;
        Ok(Entry::new(note, root, path).note)
    }
}

/// A note with what listing and ranking need.
struct Entry {
    note: Note,
    file_name: String,
    tags: Vec<String>,
}

impl Entry {
    fn new(note: Note, root: &Path, real: &Path) -> Self {
        let name = real
            .strip_prefix(root)
            .unwrap_or(real)
            .to_string_lossy()
            .replace('\\', "/");
        let file_name = real
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (tags, text) = split_tags(&note.text);
        Self {
            note: Note { name, text },
            file_name,
            tags,
        }
    }

    /// File names start with the date, so this orders newest first when sorted descending.
    fn newest_first_key(&self) -> (&str, &str) {
        (&self.file_name, &self.note.name)
    }
}

fn is_note(p: &Path) -> bool {
    p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt"))
}

/// The file's text, if it is inside the sandbox, at most MAX_DOC_BYTES and UTF-8.
fn read_note(sandbox: &Sandbox, real: &Path) -> Result<Note, KestrelError> {
    let real = sandbox.resolve(&real.to_string_lossy())?;
    let meta = std::fs::metadata(&real).map_err(|e| KestrelError::Storage(e.to_string()))?;
    if meta.len() > MAX_DOC_BYTES {
        return Err(KestrelError::Invalid(format!(
            "document larger than {} KiB",
            MAX_DOC_BYTES / 1024
        )));
    }
    let bytes = std::fs::read(&real).map_err(|e| KestrelError::Storage(e.to_string()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| KestrelError::Invalid("document is not UTF-8 text".into()))?;
    Ok(Note {
        name: String::new(),
        text,
    })
}

/// (lowercased tags from a first line `tags: a, b`, the text without that line).
fn split_tags(text: &str) -> (Vec<String>, String) {
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    match first.strip_prefix("tags:") {
        Some(tags) => (
            tags.split(',')
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect(),
            rest.to_string(),
        ),
        None => (Vec::new(), text.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, files: &[(&str, &str)]) -> (PathBuf, Notes) {
        let d = std::env::temp_dir().join(format!("kestrel_notes_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (f, text) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let notes = Notes::new(Sandbox::new(std::slice::from_ref(&d)).unwrap());
        (d, notes)
    }

    #[test]
    fn all_notes_dated_newest_first_then_the_rest_by_name() {
        let (d, notes) = folder(
            "all",
            &[
                ("2026-10-01-a.md", "a"),
                ("2026-10-05-b.md", "tags: work\nb"),
                ("plan.txt", "p"),
                ("sub/2026-09-01-c.md", "c"),
                ("about.md", "x"),
                ("photo.png", "not a note"),
            ],
        );
        let names: Vec<String> = notes.all().into_iter().map(|n| n.name).collect();
        assert_eq!(
            names,
            [
                "2026-10-05-b.md",
                "2026-10-01-a.md",
                "sub/2026-09-01-c.md",
                "about.md",
                "plan.txt"
            ]
        );
        assert_eq!(notes.all()[0].text, "b", "the tags line is not text");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_note_is_read_by_its_listed_name_only() {
        let (d, notes) = folder("byname", &[("sub/2026-09-01-c.md", "c"), ("plan.txt", "p")]);
        assert_eq!(notes.by_name("sub/2026-09-01-c.md").unwrap().text, "c");
        assert!(notes.by_name("../../etc/passwd").is_err());
        assert!(notes.by_name("missing.md").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
