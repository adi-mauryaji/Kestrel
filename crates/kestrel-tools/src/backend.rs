//! Tool backends behind the `Backend` trait (ADR-023). Notes are files under the sandbox roots
//! in both (`notes.rs`). The Week-9 fixture keeps tasks, preferences and memories in RAM (the
//! Kestrel-Tasks-v0 evaluation uses it); the Week-10 memory backend keeps them in the SQLite memory
//! file (kestrel-memory, ADR-026). Creating a task is idempotent within one request (Ch. 16): the
//! same title and due date again return the first task's id.

use std::collections::{BTreeMap, HashMap};

use kestrel_core::KestrelError;
use kestrel_memory::retrieval::{self, DAY};
use kestrel_memory::{Draft, Kind, Memory, Source, Weights, context};

pub use crate::notes::MAX_DOC_BYTES;
use crate::notes::Notes;
use crate::sandbox::Sandbox;
use crate::tools::{Backend, Note, ToolOutput, ValidatedCall};

/// How long a task-history memory is kept (Ch. 17.1).
pub const TASK_HISTORY_DAYS: i64 = 90;
/// How many memories `retrieve_memory` returns.
pub const RETRIEVE_K: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub due: Option<String>,
}

pub struct FixtureBackend {
    notes: Notes,
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
            notes: Notes::new(sandbox),
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
        self.notes.sandbox()
    }

    pub fn tasks(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    pub fn prefs(&self) -> &BTreeMap<String, String> {
        &self.prefs
    }

    pub fn recent(&self, n: u32, tag: Option<&str>) -> Result<Vec<Note>, KestrelError> {
        self.notes.recent(n, tag)
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
                ToolOutput::Notes(self.notes.search(query, *limit))
            }
            ValidatedCall::ReadDocument { path } => ToolOutput::Notes(vec![self.notes.read(path)?]),
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

/// The Week-10 backend: notes are files as in the fixture; tasks, preferences and memories live
/// in the memory file. A created task leaves a task-history memory that expires (Ch. 8 step 13);
/// `retrieve_memory` returns provenance-tagged lines (Ch. 18.2); a preference reaches it either
/// stated this turn (stored as the user's) or confirmed at the prompt (`run_confirmed`, ADR-026).
pub struct MemoryBackend {
    notes: Notes,
    memory: Memory,
    request: u64,
    now: i64,
    /// (request, lowercased title, due) -> task id.
    created: HashMap<(u64, String, Option<String>), u64>,
}

impl MemoryBackend {
    pub fn new(sandbox: Sandbox, memory: Memory) -> Self {
        Self {
            notes: Notes::new(sandbox),
            memory,
            request: 0,
            now: 0,
            created: HashMap::new(),
        }
    }

    /// Idempotency keys are per request; `now` (Unix seconds) stamps what the request writes.
    pub fn begin_request(&mut self, id: u64, now: i64) {
        self.request = id;
        self.now = now;
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut Memory {
        &mut self.memory
    }

    pub fn sandbox(&self) -> &Sandbox {
        self.notes.sandbox()
    }

    pub fn recent(&self, n: u32, tag: Option<&str>) -> Result<Vec<Note>, KestrelError> {
        self.notes.recent(n, tag)
    }

    fn create(&mut self, title: &str, due: &Option<String>) -> Result<u64, KestrelError> {
        let key = (self.request, title.to_lowercase(), due.clone());
        if let Some(&id) = self.created.get(&key)
            && self.memory.tasks()?.iter().any(|t| t.id as u64 == id)
        {
            return Ok(id);
        }
        let row = self.memory.create_task(title, due.as_deref(), self.now)?;
        let id = u64::try_from(row).map_err(|_| KestrelError::Storage(format!("task id {row}")))?;
        self.created.insert(key, id);
        let history_key = format!("task_{id}");
        let history = Draft::new(Kind::TaskHistory, &history_key, title)
            .importance(2)
            .expires_at(self.now + TASK_HISTORY_DAYS * DAY);
        // A title that looks like a secret creates the task but leaves no history.
        match self
            .memory
            .propose(&history, Source::UserExplicit, self.now)
        {
            Ok(_) | Err(KestrelError::Denied(_)) => Ok(id),
            Err(e) => Err(e),
        }
    }

    fn retrieve(&self, terms: &str) -> Result<String, KestrelError> {
        let terms = retrieval::terms(terms);
        let hits = self
            .memory
            .retrieve(&terms, self.now, RETRIEVE_K, &Weights::default())?;
        let lines: Vec<String> = hits.iter().map(|s| context::line(&s.item)).collect();
        Ok(lines.join("\n"))
    }

    fn preference(
        &mut self,
        key: &str,
        value: &str,
        confirmed: bool,
    ) -> Result<ToolOutput, KestrelError> {
        let d = Draft::new(Kind::Preference, key, value).importance(4);
        if confirmed {
            self.memory.confirm(&d, Source::ModelInferred, self.now)?;
        } else {
            self.memory.propose(&d, Source::UserExplicit, self.now)?;
        }
        Ok(ToolOutput::Done)
    }
}

impl Backend for MemoryBackend {
    fn run(&mut self, c: &ValidatedCall) -> Result<ToolOutput, KestrelError> {
        Ok(match c {
            ValidatedCall::SearchNotes { query, limit } => {
                ToolOutput::Notes(self.notes.search(query, *limit))
            }
            ValidatedCall::ReadDocument { path } => ToolOutput::Notes(vec![self.notes.read(path)?]),
            ValidatedCall::CreateTask { title, due } => {
                ToolOutput::TaskId(self.create(title, due)?)
            }
            ValidatedCall::DeleteTask { id } => {
                let deleted = match i64::try_from(*id) {
                    Ok(row) => self.memory.delete_task(row)?,
                    Err(_) => false,
                };
                if !deleted {
                    return Err(KestrelError::Invalid(format!("there is no task {id}")));
                }
                ToolOutput::Done
            }
            ValidatedCall::RetrieveMemory { terms } => ToolOutput::Text(self.retrieve(terms)?),
            // Without confirmation a preference reaches the backend only when the user stated it
            // this turn (tools::requires_confirmation).
            ValidatedCall::UpdatePreference { key, value } => self.preference(key, value, false)?,
        })
    }

    fn run_confirmed(&mut self, c: &ValidatedCall) -> Result<ToolOutput, KestrelError> {
        match c {
            ValidatedCall::UpdatePreference { key, value } => self.preference(key, value, true),
            other => self.run(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Confirm, Policy, execute};
    use kestrel_memory::Filter;
    use std::fs;
    use std::path::{Path, PathBuf};

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
    fn only_dated_notes_are_recent_but_documents_stay_searchable() {
        let d = notes_dir("g");
        fs::create_dir(d.join("docs")).unwrap();
        fs::write(
            d.join("docs/project-plan.md"),
            "tags: meeting\nThe project plan.",
        )
        .unwrap();
        fs::write(
            d.join("2026-1-5-bad-date.md"),
            "tags: meeting\nnot a date prefix",
        )
        .unwrap();
        let mut b = FixtureBackend::new(Sandbox::new(&[d]).unwrap());
        let recent: Vec<String> = b
            .recent(10, Some("meeting"))
            .unwrap()
            .into_iter()
            .map(|n| n.name)
            .collect();
        assert_eq!(recent.len(), 3, "{recent:?}");
        assert!(
            recent
                .iter()
                .all(|n| !n.contains("project-plan") && !n.contains("bad-date"))
        );
        let out = b
            .run(&ValidatedCall::SearchNotes {
                query: "project plan".into(),
                limit: 10,
            })
            .unwrap();
        assert!(
            matches!(out, ToolOutput::Notes(ref n) if n.len() == 1 && n[0].name == "docs/project-plan.md")
        );
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

    const NOW: i64 = 1_792_713_600; // 2026-10-23T00:00:00Z

    fn db(name: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("kestrel_backend_{name}_{}.db", std::process::id()));
        let _ = fs::remove_file(&p);
        p
    }

    fn memory_backend(name: &str, db: &Path) -> MemoryBackend {
        let notes = Sandbox::new(&[notes_dir(name)]).unwrap();
        MemoryBackend::new(notes, Memory::open(db).unwrap())
    }

    struct Says(bool);
    impl Confirm for Says {
        fn ask(&self, _: &str) -> bool {
            self.0
        }
    }

    fn pref(value: &str) -> ValidatedCall {
        ValidatedCall::UpdatePreference {
            key: "answer_length".into(),
            value: value.into(),
        }
    }

    #[test]
    fn the_memory_backend_reads_notes_like_the_fixture() {
        let mut b = memory_backend("m0", &db("m0"));
        let out = b
            .run(&ValidatedCall::SearchNotes {
                query: "budget asha".into(),
                limit: 10,
            })
            .unwrap();
        assert!(
            matches!(out, ToolOutput::Notes(ref n) if n.len() == 1 && n[0].name.ends_with("standup.md"))
        );
        let recent: Vec<String> = b
            .recent(1, None)
            .unwrap()
            .into_iter()
            .map(|n| n.name)
            .collect();
        assert_eq!(recent, ["2026-10-22-groceries.txt"]);
    }

    #[test]
    fn tasks_and_preferences_persist_across_opens() {
        let path = db("m1");
        let mut b = memory_backend("m1", &path);
        b.begin_request(1, NOW);
        let call = ValidatedCall::CreateTask {
            title: "Call Asha".into(),
            due: Some("2026-10-24".into()),
        };
        assert_eq!(b.run(&call).unwrap(), ToolOutput::TaskId(1));
        let again = ValidatedCall::CreateTask {
            title: "call asha".into(),
            due: Some("2026-10-24".into()),
        };
        assert_eq!(
            b.run(&again).unwrap(),
            ToolOutput::TaskId(1),
            "a duplicate is a no-op"
        );
        b.run(&pref("short")).unwrap();
        drop(b);
        let mut b = memory_backend("m1", &path);
        b.begin_request(2, NOW + 60);
        assert_eq!(b.memory().tasks().unwrap().len(), 1);
        assert_eq!(
            b.run(&call).unwrap(),
            ToolOutput::TaskId(2),
            "a new request may create it again"
        );
        let out = b
            .run(&ValidatedCall::RetrieveMemory {
                terms: "answer length".into(),
            })
            .unwrap();
        assert_eq!(
            out,
            ToolOutput::Text("[pref|user_explicit|2026-10-23] answer_length: short".into())
        );
    }

    #[test]
    fn a_confirmed_change_is_the_users_and_a_declined_one_changes_nothing() {
        let mut b = memory_backend("m2", &db("m2"));
        b.begin_request(1, NOW);
        let stated = Policy {
            user_stated_preference: true,
        };
        execute(&pref("short"), stated, &Says(false), &mut b).unwrap();
        let declined = execute(&pref("detailed"), Policy::default(), &Says(false), &mut b);
        assert!(matches!(declined, Err(KestrelError::Denied(_))));
        execute(&pref("detailed"), Policy::default(), &Says(true), &mut b).unwrap();
        let items = b.memory().items(Filter::All).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0].value.as_str(), items[0].source),
            ("detailed", Source::UserExplicit)
        );
        let ops: Vec<String> = b
            .memory()
            .audit()
            .unwrap()
            .into_iter()
            .map(|e| e.op)
            .collect();
        assert_eq!(ops, ["store", "confirm"]);
    }

    #[test]
    fn a_created_task_leaves_history_that_expires() {
        let mut b = memory_backend("m3", &db("m3"));
        b.begin_request(1, NOW);
        let create = |title: &str| ValidatedCall::CreateTask {
            title: title.into(),
            due: None,
        };
        let ToolOutput::TaskId(id) = b.run(&create("Email the budget to Asha")).unwrap() else {
            panic!("a task id")
        };
        let history = b.memory().items(Filter::Approved).unwrap();
        assert_eq!(history.len(), 1);
        let h = &history[0];
        assert_eq!(
            (h.kind, h.key.clone(), h.value.as_str(), h.expires_at),
            (
                Kind::TaskHistory,
                format!("task_{id}"),
                "Email the budget to Asha",
                Some(NOW + 90 * DAY)
            )
        );
        b.memory_mut().purge_expired(NOW + 91 * DAY).unwrap();
        assert!(b.memory().items(Filter::All).unwrap().is_empty());
        assert_eq!(
            b.memory().tasks().unwrap().len(),
            1,
            "the task itself stays"
        );
        b.run(&create("change the wifi password")).unwrap();
        assert_eq!(b.memory().tasks().unwrap().len(), 2);
        assert!(
            b.memory().items(Filter::All).unwrap().is_empty(),
            "a secret-looking title keeps no history"
        );
    }

    #[test]
    fn deleting_a_task_in_the_memory_file() {
        let mut b = memory_backend("m4", &db("m4"));
        b.begin_request(1, NOW);
        let ToolOutput::TaskId(id) = b
            .run(&ValidatedCall::CreateTask {
                title: "Buy milk".into(),
                due: None,
            })
            .unwrap()
        else {
            panic!("a task id")
        };
        let del = |id| ValidatedCall::DeleteTask { id };
        assert_eq!(b.run(&del(id)).unwrap(), ToolOutput::Done);
        for missing in [id, u64::MAX] {
            assert!(matches!(
                b.run(&del(missing)),
                Err(KestrelError::Invalid(_))
            ));
        }
    }
}
