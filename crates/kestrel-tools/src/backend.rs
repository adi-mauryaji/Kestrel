//! The Week-9 fixture backend (ADR-023): notes are files under the sandbox roots, tasks and
//! preferences live in memory. Week 10 puts SQLite behind the same `Backend` trait.
//!
//! Notes: `*.md` / `*.txt` files, named `YYYY-MM-DD-<slug>.md` so "recent" is the date prefix
//! (deterministic, unlike file times); an optional first line `tags: a, b` tags a note. Every file
//! is reached through `Sandbox::resolve`, so a symlink out of the notes folder is skipped.
//! Search is lexical: a note matches when it contains every query term (case-insensitive), ranked
//! by how often the terms occur, then most recent first. Creating a task is idempotent within one request
//! (Ch. 16): the same title and due date again return the first task's id.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use kestrel_core::KestrelError;

use crate::sandbox::Sandbox;
use crate::tools::{Backend, Note, ToolOutput, ValidatedCall};

/// Largest note or document read, in bytes.
pub const MAX_DOC_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub due: Option<String>,
}

pub struct FixtureBackend {
    sandbox: Sandbox,
    tasks: BTreeMap<u64, Task>,
    next_id: u64,
    prefs: BTreeMap<String, String>,
    memories: Vec<String>,
    request: u64,
    /// (request, lowercased title, due) -> task id.
    created: HashMap<(u64, String, Option<String>), u64>,
}

impl FixtureBackend {
    pub fn new(sandbox: Sandbox) -> Self {
        Self {
            sandbox,
            tasks: BTreeMap::new(),
            next_id: 1,
            prefs: BTreeMap::new(),
            memories: Vec::new(),
            request: 0,
            created: HashMap::new(),
        }
    }

    /// Seeds the fixture state (Kestrel-Tasks-v0 "fixture state").
    pub fn with_tasks(mut self, tasks: Vec<Task>) -> Self {
        for t in tasks {
            self.next_id = self.next_id.max(t.id + 1);
            self.tasks.insert(t.id, t);
        }
        self
    }

    pub fn with_memories(mut self, memories: Vec<String>) -> Self {
        self.memories = memories;
        self
    }

    /// Idempotency keys are per request.
    pub fn begin_request(&mut self, id: u64) {
        self.request = id;
    }

    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    pub fn tasks(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    pub fn prefs(&self) -> &BTreeMap<String, String> {
        &self.prefs
    }

    /// The `n` most recent notes (by name), optionally only those tagged `tag`.
    pub fn recent(&self, n: u32, tag: Option<&str>) -> Result<Vec<Note>, KestrelError> {
        let tag = tag.map(str::to_lowercase);
        let mut notes: Vec<Entry> = self
            .notes()
            .into_iter()
            .filter(|e| tag.as_ref().is_none_or(|t| e.tags.contains(t)))
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

    fn search(&self, query: &str, limit: u32) -> Vec<Note> {
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

    fn create(&mut self, title: &str, due: &Option<String>) -> u64 {
        let key = (self.request, title.to_lowercase(), due.clone());
        if let Some(&id) = self.created.get(&key)
            && self.tasks.contains_key(&id)
        {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.insert(
            id,
            Task {
                id,
                title: title.to_string(),
                due: due.clone(),
            },
        );
        self.created.insert(key, id);
        id
    }

    fn retrieve(&self, terms: &str) -> String {
        let terms: Vec<String> = terms.split_whitespace().map(str::to_lowercase).collect();
        let prefs = self.prefs.iter().map(|(k, v)| format!("{k}: {v}"));
        prefs
            .chain(self.memories.iter().cloned())
            .filter(|line| {
                let l = line.to_lowercase();
                terms.iter().all(|t| l.contains(t.as_str()))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Backend for FixtureBackend {
    fn run(&mut self, c: &ValidatedCall) -> Result<ToolOutput, KestrelError> {
        Ok(match c {
            ValidatedCall::SearchNotes { query, limit } => {
                ToolOutput::Notes(self.search(query, *limit))
            }
            ValidatedCall::ReadDocument { path } => {
                let root = self.sandbox.roots().iter().find(|r| path.starts_with(r));
                let root = root.ok_or_else(|| {
                    KestrelError::Sandbox("path escapes the approved directories".into())
                })?;
                let note = read_note(&self.sandbox, path)?;
                ToolOutput::Notes(vec![Entry::new(note, root, path).note])
            }
            ValidatedCall::CreateTask { title, due } => ToolOutput::TaskId(self.create(title, due)),
            ValidatedCall::DeleteTask { id } => {
                self.tasks
                    .remove(id)
                    .ok_or_else(|| KestrelError::Invalid(format!("there is no task {id}")))?;
                ToolOutput::Done
            }
            ValidatedCall::RetrieveMemory { terms } => ToolOutput::Text(self.retrieve(terms)),
            ValidatedCall::UpdatePreference { key, value } => {
                self.prefs.insert(key.clone(), value.clone());
                ToolOutput::Done
            }
        })
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
    use std::fs;
    use std::path::PathBuf;

    fn notes_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_backend_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("meetings")).unwrap();
        fs::write(
            d.join("meetings/2026-10-20-standup.md"),
            "tags: meeting\nBudget review moved.\n- [ ] email the budget to asha",
        )
        .unwrap();
        fs::write(
            d.join("meetings/2026-10-21-retro.md"),
            "tags: meeting, retro\nRetro notes. Budget budget.",
        )
        .unwrap();
        fs::write(d.join("2026-10-22-groceries.txt"), "milk, eggs").unwrap();
        fs::write(d.join("2026-10-19-old.md"), "tags: meeting\nOld meeting.").unwrap();
        fs::write(d.join("image.png"), "not a note").unwrap();
        d
    }

    fn backend(name: &str) -> FixtureBackend {
        let d = notes_dir(name);
        FixtureBackend::new(Sandbox::new(&[d]).unwrap())
    }

    #[test]
    fn recent_notes_follow_the_date_in_the_name_and_the_tag() {
        let b = backend("a");
        let names = |v: Vec<Note>| v.into_iter().map(|n| n.name).collect::<Vec<_>>();
        assert_eq!(
            names(b.recent(3, Some("meeting")).unwrap()),
            [
                "meetings/2026-10-21-retro.md",
                "meetings/2026-10-20-standup.md",
                "2026-10-19-old.md"
            ]
        );
        assert_eq!(
            names(b.recent(1, None).unwrap()),
            ["2026-10-22-groceries.txt"]
        );
        assert!(b.recent(5, Some("nothing")).unwrap().is_empty());
    }

    #[test]
    fn search_needs_every_term_and_ranks_by_occurrences() {
        let mut b = backend("b");
        let out = b
            .run(&ValidatedCall::SearchNotes {
                query: "BUDGET".into(),
                limit: 10,
            })
            .unwrap();
        let ToolOutput::Notes(notes) = out else {
            panic!("{out:?}")
        };
        let names: Vec<_> = notes.iter().map(|n| n.name.as_str()).collect();
        // Both have "budget" twice, so the more recent note comes first.
        assert_eq!(
            names,
            [
                "meetings/2026-10-21-retro.md",
                "meetings/2026-10-20-standup.md"
            ]
        );
        let out = b
            .run(&ValidatedCall::SearchNotes {
                query: "budget asha".into(),
                limit: 10,
            })
            .unwrap();
        assert!(
            matches!(out, ToolOutput::Notes(ref n) if n.len() == 1 && n[0].name.ends_with("standup.md"))
        );
        let out = b
            .run(&ValidatedCall::SearchNotes {
                query: "budget".into(),
                limit: 1,
            })
            .unwrap();
        assert!(matches!(out, ToolOutput::Notes(ref n) if n.len() == 1));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_notes_folder_is_never_read() {
        let d = notes_dir("c");
        let outside =
            std::env::temp_dir().join(format!("kestrel_backend_c_out_{}", std::process::id()));
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("2026-12-31-secret.md"), "budget secret").unwrap();
        std::os::unix::fs::symlink(&outside, d.join("leak")).unwrap();
        let mut b = FixtureBackend::new(Sandbox::new(&[d]).unwrap());
        let out = b
            .run(&ValidatedCall::SearchNotes {
                query: "secret".into(),
                limit: 10,
            })
            .unwrap();
        assert_eq!(out, ToolOutput::Notes(vec![]));
        assert!(
            b.recent(10, None)
                .unwrap()
                .iter()
                .all(|n| !n.text.contains("secret"))
        );
    }

    #[test]
    fn reading_a_document_returns_its_text() {
        let d = notes_dir("d");
        let sb = Sandbox::new(std::slice::from_ref(&d)).unwrap();
        let path = sb.resolve("2026-10-22-groceries.txt").unwrap();
        let mut b = FixtureBackend::new(sb);
        let out = b.run(&ValidatedCall::ReadDocument { path }).unwrap();
        assert_eq!(
            out,
            ToolOutput::Notes(vec![Note {
                name: "2026-10-22-groceries.txt".into(),
                text: "milk, eggs".into()
            }])
        );
        fs::write(d.join("big.md"), "x".repeat(MAX_DOC_BYTES as usize + 1)).unwrap();
        let big = Sandbox::new(std::slice::from_ref(&d))
            .unwrap()
            .resolve("big.md")
            .unwrap();
        assert!(matches!(
            b.run(&ValidatedCall::ReadDocument { path: big }),
            Err(KestrelError::Invalid(_))
        ));
    }

    #[test]
    fn tasks_are_created_idempotently_per_request_and_deleted() {
        let mut b = backend("e").with_tasks(vec![Task {
            id: 7,
            title: "old".into(),
            due: None,
        }]);
        b.begin_request(1);
        let call = ValidatedCall::CreateTask {
            title: "Call Asha".into(),
            due: Some("2026-10-23".into()),
        };
        let first = b.run(&call).unwrap();
        assert_eq!(first, ToolOutput::TaskId(8)); // after the seeded id 7
        let again = ValidatedCall::CreateTask {
            title: "call asha".into(),
            due: Some("2026-10-23".into()),
        };
        assert_eq!(
            b.run(&again).unwrap(),
            first,
            "a duplicate submission is a no-op"
        );
        assert_eq!(b.tasks().len(), 2);
        b.begin_request(2);
        assert_eq!(
            b.run(&call).unwrap(),
            ToolOutput::TaskId(9),
            "a new request may create it again"
        );
        assert_eq!(
            b.run(&ValidatedCall::DeleteTask { id: 7 }).unwrap(),
            ToolOutput::Done
        );
        assert!(matches!(
            b.run(&ValidatedCall::DeleteTask { id: 7 }),
            Err(KestrelError::Invalid(_))
        ));
        assert_eq!(b.tasks().iter().map(|t| t.id).collect::<Vec<_>>(), [8, 9]);
    }

    #[test]
    fn preferences_and_memories_are_retrieved_by_term() {
        let mut b = backend("f").with_memories(vec![
            "Asha prefers calls after 6 pm".into(),
            "The budget is due Friday".into(),
        ]);
        let set = ValidatedCall::UpdatePreference {
            key: "answer_length".into(),
            value: "short".into(),
        };
        assert_eq!(b.run(&set).unwrap(), ToolOutput::Done);
        assert_eq!(
            b.prefs().get("answer_length").map(String::as_str),
            Some("short")
        );
        let out = b
            .run(&ValidatedCall::RetrieveMemory {
                terms: "asha".into(),
            })
            .unwrap();
        assert_eq!(
            out,
            ToolOutput::Text("Asha prefers calls after 6 pm".into())
        );
        let out = b
            .run(&ValidatedCall::RetrieveMemory {
                terms: "answer length".into(),
            })
            .unwrap();
        assert_eq!(out, ToolOutput::Text("answer_length: short".into()));
        assert_eq!(
            b.run(&ValidatedCall::RetrieveMemory {
                terms: "zebra".into()
            })
            .unwrap(),
            ToolOutput::Text(String::new())
        );
    }
}
