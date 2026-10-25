//! `kestrel ask --notes DIR`: carry a routed request out (Ch. 54 "working application"; Ch. 8 steps 5-12).
//!
//! The request's template plan (ADR-023), or for a request that joins several (the routing signal,
//! ADR-017) its routed parts' template plans joined (ADR-025), is validated (`plan::parse`) and run by
//! the tool runner: notes and documents are read through the sandbox of the notes folder, tasks and
//! preferences go into the memory file (a second connection to the one `ask` personalizes with). The
//! tool policy asks before a delete, a batch of more than three tasks, or a preference not stated in
//! this turn. What was found or done comes back as lines to print; the runner's own log stays
//! content-free. Not here: plans only the model could make (ADR-025's LM fallback).
//!
//! Stage 2 (ADR-034): a time said in a task request ("at 5 pm", "shaam 6 baje", src/times.rs) is
//! taken out before the template runs and put on the tasks the plan created; the service rings it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kestrel_core::resources::{CancelToken, Deadline};
use kestrel_core::{Intent, KestrelError};
use kestrel_memory::TaskRow;
use kestrel_tools::backend::MemoryBackend;
use kestrel_tools::compound::plan_parts;
use kestrel_tools::date::Date;
use kestrel_tools::plan::{self, Data, Execution, NodeStatus, Op, ValidPlan};
use kestrel_tools::runner::{MarkedItems, Runner};
use kestrel_tools::sandbox::Sandbox;
use kestrel_tools::templates::{self, Context, Proposal};
use kestrel_tools::tools::{Confirm, Policy};

use crate::times::Said;

/// A document is shown up to this many lines; a search lists up to this many notes.
const SHOW_LINES: usize = 40;
const SHOW_NOTES: usize = 10;
/// A batch of more tasks than this asks once (as in the Ch. 8 scenario).
const BATCH_CONFIRM_ABOVE: usize = 3;

pub struct Act {
    backend: MemoryBackend,
    notes: PathBuf,
    /// Seconds east of UTC, for the local date (ADR-034: the UTC date is a day behind in India
    /// from 00:00 to 05:30).
    utc_offset: i64,
}

/// The y/N prompt of `ask`, as the tool runner's confirmation.
pub struct Prompt<'a>(pub RefCell<&'a mut dyn FnMut(&str) -> bool>);

impl Confirm for Prompt<'_> {
    fn ask(&self, q: &str) -> bool {
        (self.0.borrow_mut())(q)
    }
}

impl Act {
    /// The notes folder (the sandbox's only root) and the memory file.
    pub fn open(notes: &Path, memory: &Path) -> Result<Self, KestrelError> {
        let notes = notes
            .canonicalize()
            .map_err(|e| KestrelError::Sandbox(format!("notes folder {}: {e}", notes.display())))?;
        let sandbox = Sandbox::new(std::slice::from_ref(&notes))?;
        let memory =
            crate::memory::open(memory).map_err(|e| KestrelError::Storage(e.to_string()))?;
        Ok(Self {
            backend: MemoryBackend::new(sandbox, memory),
            notes,
            utc_offset: crate::clock::utc_offset(),
        })
    }

    /// Dates relative to a zone `seconds` east of UTC instead of the system's (tests pin it).
    #[cfg(test)]
    pub fn with_utc_offset(mut self, seconds: i64) -> Self {
        self.utc_offset = seconds;
        self
    }

    /// The local date at Unix time `now`.
    pub fn today(&self, now: i64) -> Date {
        crate::clock::local_date(now, self.utc_offset)
    }

    /// A task command (ADR-034) on the memory file; what was done, as lines to print.
    pub fn command(&mut self, cmd: &crate::commands::Command, now: i64) -> Vec<String> {
        let today = self.today(now);
        let minute = crate::clock::local_minute(now, self.utc_offset);
        crate::commands::run(cmd, self.backend.memory_mut(), today, minute)
            .unwrap_or_else(|e| vec![format!("that did not work: {e}")])
    }

    /// Every note and document name under the notes folder (relative, '/'-separated, sorted).
    pub fn documents(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk(&self.notes, &self.notes, &mut out);
        out.sort();
        out
    }

    /// One routed part: its template; several: their plans joined (ADR-025).
    pub fn propose(&self, parts: &[(Intent, String)], today: Date) -> Proposal {
        let documents = self.documents();
        let ctx = Context {
            today,
            documents: &documents,
        };
        match parts {
            [] => Proposal::NeedsModel,
            [(intent, text)] => templates::propose(*intent, text, &ctx),
            parts => plan_parts(parts, &ctx),
        }
    }

    /// Plans and runs the request; `routed` is None when the cascade (or one part) abstained.
    pub fn carry_out(
        &mut self,
        routed: Option<&[(Intent, String)]>,
        id: u64,
        now: i64,
        confirm: &dyn Confirm,
    ) -> Vec<String> {
        let Some(parts) = routed.filter(|p| !p.is_empty()) else {
            return vec!["I'm not sure what you mean. Try saying it another way.".into()];
        };
        // Each task part's time (or None), in order; the parts without their time words.
        let mut said = Vec::new();
        let parts: Vec<(Intent, String)> = parts
            .iter()
            .map(|(intent, text)| match intent {
                Intent::CreateTask => match crate::times::take(text) {
                    Some((rest, s)) => {
                        said.push(Some(s));
                        (*intent, rest)
                    }
                    None => {
                        said.push(None);
                        (*intent, text.clone())
                    }
                },
                _ => (*intent, text.clone()),
            })
            .collect();
        match self.propose(&parts, self.today(now)) {
            Proposal::Plan(mut wire) => {
                resolve_documents(&mut wire, &self.documents());
                let stated = matches!(parts[..], [(Intent::SetPreference, _)]);
                self.run(&wire.to_string(), stated, id, now, confirm, &said)
            }
            Proposal::Clarify(slot) => vec![clarify(slot)],
            // One part no template fits: usually notes it cannot pick out; several: a plan only the model
            // could make (ADR-025's fallback, not in `ask`).
            Proposal::NeedsModel if parts.len() == 1 => vec![
                "I couldn't tell which notes you mean. Name a note, or say \"my last 3 notes\" or \"my meeting notes\"."
                    .into(),
            ],
            Proposal::NeedsModel => {
                vec!["That needs several steps I can't plan yet. Try asking for one thing at a time.".into()]
            }
            Proposal::NoAction => vec![no_action(&parts)],
        }
    }

    fn run(
        &mut self,
        wire: &str,
        stated: bool,
        id: u64,
        now: i64,
        confirm: &dyn Confirm,
        said: &[Option<Said>],
    ) -> Vec<String> {
        let plan = match plan::parse(wire) {
            Ok(p) => p,
            Err(e) => return vec![format!("I made a plan I can't run ({e}).")],
        };
        self.backend.begin_request(id, now);
        let before: HashSet<i64> = self.task_ids();
        let mut text = MarkedItems;
        let mut runner = Runner {
            store: &mut self.backend,
            text: &mut text,
            confirm,
            policy: Policy {
                user_stated_preference: stated,
            },
            batch_confirm_above: BATCH_CONFIRM_ABOVE,
            log: vec![],
        };
        let ex = plan::execute(
            &plan,
            &CancelToken::new(),
            &Deadline::after(Duration::from_secs(60)),
            |n, i, a| runner.run(n, i, a),
        );
        let new = |a: &Self| -> Vec<TaskRow> {
            a.backend
                .memory()
                .tasks()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| !before.contains(&t.id))
                .collect()
        };
        let note = self.set_times(&new(self), said, now);
        let mut out = render(&plan, &ex, &new(self));
        out.extend(note);
        out
    }

    /// Puts the times said onto the tasks just created: one each, in order, when there are as many
    /// tasks as task parts; else one time on all of them. A task without a date gets today's, or
    /// tomorrow's once the time has passed. A line when the times could not be matched.
    fn set_times(
        &mut self,
        created: &[TaskRow],
        said: &[Option<Said>],
        now: i64,
    ) -> Option<String> {
        let times: Vec<Said> = said.iter().flatten().copied().collect();
        if times.is_empty() || created.is_empty() {
            return None;
        }
        let each: Vec<Option<Said>> = if said.len() == created.len() {
            said.to_vec()
        } else if times.iter().all(|t| *t == times[0]) {
            vec![Some(times[0]); created.len()]
        } else {
            return Some("I couldn't tell which time goes with which task.".into());
        };
        let (today, minute) = (
            self.today(now),
            crate::clock::local_minute(now, self.utc_offset),
        );
        let memory = self.backend.memory_mut();
        for (t, s) in created.iter().zip(each) {
            let Some(s) = s else { continue };
            let due = t.due.as_deref().and_then(Date::parse);
            let (date, m) = crate::times::resolve(s, due, today, minute);
            if due != Some(date)
                && let Err(e) = memory.update_task(t.id, None, Some(Some(&date.to_string())))
            {
                return Some(format!("could not set the date of task #{}: {e}", t.id));
            }
            if let Err(e) = memory.set_task_time(t.id, Some(&crate::times::hhmm(m))) {
                return Some(format!("could not set the time of task #{}: {e}", t.id));
            }
        }
        None
    }

    fn task_ids(&self) -> HashSet<i64> {
        self.backend
            .memory()
            .tasks()
            .unwrap_or_default()
            .iter()
            .map(|t| t.id)
            .collect()
    }
}

/// What the plan found or did, step by step: every step's failure or refusal, and the output of the
/// steps nothing else consumed (a summary, a search's notes, a document), plus the tasks created.
fn render(plan: &ValidPlan, ex: &Execution, created: &[TaskRow]) -> Vec<String> {
    let consumed: HashSet<usize> = plan
        .nodes
        .iter()
        .flat_map(|n| n.deps.iter().copied())
        .collect();
    let mut out = Vec::new();
    let mut tasks_shown = false;
    for &i in &plan.order {
        let node = &plan.nodes[i];
        let what = node.op.name().replace('_', " ");
        match ex.status[i] {
            NodeStatus::Done => match &node.op {
                Op::CreateTask { .. } if !tasks_shown => {
                    tasks_shown = true;
                    out.extend(created.iter().map(task_line));
                }
                Op::CreateTask { .. } => {}
                Op::DeleteTask { id } => out.push(format!("deleted task #{id}")),
                Op::UpdatePreference { key, value } => {
                    out.push(format!("saved preference: {key} = {value}"))
                }
                op if !consumed.contains(&i) => out.extend(show(op, ex.outputs[i].as_ref())),
                _ => {}
            },
            NodeStatus::Declined => out.push(match &node.op {
                Op::DeleteTask { id } => format!("kept task #{id} (you said no)"),
                _ => format!("did not {what} (you said no)"),
            }),
            NodeStatus::Failed => out.push(format!(
                "could not {what}: {}",
                ex.errors[i].as_deref().unwrap_or("it failed")
            )),
            NodeStatus::SkippedDueToDependency => {
                out.push(format!("skipped {what} (an earlier step failed)"))
            }
            _ => {}
        }
    }
    if let Some(why) = ex.stopped {
        out.push(format!("stopped early: {why}"));
    }
    if out.is_empty() {
        out.push("done".into());
    }
    out
}

fn show(op: &Op, data: Option<&Data>) -> Vec<String> {
    match (op, data) {
        (Op::ReadDocument { .. }, Some(Data::Notes(notes))) => notes
            .iter()
            .flat_map(|n| {
                let lines: Vec<&str> = n.text.lines().collect();
                let mut v = vec![format!("── {} ──", n.name)];
                v.extend(lines.iter().take(SHOW_LINES).map(|l| l.to_string()));
                if lines.len() > SHOW_LINES {
                    v.push(format!("… ({} more lines)", lines.len() - SHOW_LINES));
                }
                v
            })
            .collect(),
        (_, Some(Data::Notes(notes))) if notes.is_empty() => vec!["no notes matched".into()],
        (_, Some(Data::Notes(notes))) => {
            let mut v = vec![format!(
                "found {} note{}:",
                notes.len(),
                if notes.len() == 1 { "" } else { "s" }
            )];
            v.extend(
                notes
                    .iter()
                    .take(SHOW_NOTES)
                    .map(|n| format!("- {}", n.name)),
            );
            if notes.len() > SHOW_NOTES {
                v.push(format!("… and {} more", notes.len() - SHOW_NOTES));
            }
            v
        }
        (Op::Summarize, Some(Data::Text(t))) => vec![format!("summary: {t}")],
        (Op::RetrieveMemory { .. }, Some(Data::Text(t))) if t.trim().is_empty() => {
            vec!["I don't remember anything about that.".into()]
        }
        (Op::RetrieveMemory { .. }, Some(Data::Text(t))) => vec![format!("from memory: {t}")],
        (_, Some(Data::Text(t))) => vec![t.clone()],
        (_, Some(Data::Items(items))) if items.is_empty() => vec!["none found".into()],
        (Op::ExtractActionItems, Some(Data::Items(items))) => {
            std::iter::once("action items:".to_string())
                .chain(items.iter().map(|i| format!("- {i}")))
                .collect()
        }
        (_, Some(Data::Items(items))) => items.iter().map(|i| format!("- {i}")).collect(),
        _ => vec![],
    }
}

fn task_line(t: &TaskRow) -> String {
    match (&t.due, &t.due_time) {
        (Some(d), Some(h)) => format!("created task #{}: {} (due {d} {h})", t.id, t.title),
        (Some(d), None) => format!("created task #{}: {} (due {d})", t.id, t.title),
        _ => format!("created task #{}: {}", t.id, t.title),
    }
}

fn clarify(slot: &str) -> String {
    match slot {
        "title" => "What should the task say? For example: remind me to call Asha tomorrow.".into(),
        "query" => "What should I search for? For example: search my notes for budget.".into(),
        "document" => "Which document? Say its name, for example: open project-plan.md.".into(),
        "task id" => "Which task? Say its number (`kestrel memory tasks` lists them).".into(),
        "terms" => "What should I look up in memory?".into(),
        "preference" => "Which preference? For example: I prefer short answers.".into(),
        other => format!("I need the {other} to do that."),
    }
}

fn no_action(parts: &[(Intent, String)]) -> String {
    if parts.iter().any(|(i, _)| *i == Intent::Unsupported) {
        "I can't do that. I can find, open and summarize your notes, keep your tasks, and remember your \
         preferences."
            .into()
    } else {
        "Hi! I can find, open and summarize your notes, and keep your tasks.".into()
    }
}

/// A `read_document` path given as a bare file name ("project-plan.md") becomes the one document of
/// that name in the notes folder ("docs/project-plan.md"). Here rather than in the templates, which
/// the experiments use as frozen (E4, E6).
fn resolve_documents(wire: &mut serde_json::Value, documents: &[String]) {
    let Some(nodes) = wire.get_mut("nodes").and_then(|n| n.as_array_mut()) else {
        return;
    };
    for node in nodes.iter_mut().filter(|n| n["op"] == "read_document") {
        let Some(path) = node["args"]["path"].as_str().map(str::to_lowercase) else {
            continue;
        };
        if path.contains('/') || documents.iter().any(|d| d.to_lowercase() == path) {
            continue;
        }
        let mut hits = documents
            .iter()
            .filter(|d| d.rsplit('/').next().unwrap_or(d).to_lowercase() == path);
        if let (Some(d), None) = (hits.next(), hits.next()) {
            node["args"]["path"] = serde_json::Value::String(d.clone());
        }
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const NOW: i64 = 1_792_713_600; // 2026-10-23T00:00:00Z

    struct Say(bool, RefCell<Vec<String>>);

    impl Confirm for Say {
        fn ask(&self, q: &str) -> bool {
            self.1.borrow_mut().push(q.to_string());
            self.0
        }
    }

    fn yes() -> Say {
        Say(true, RefCell::default())
    }

    /// A notes folder (four meeting notes, a document, a grocery note) and a memory file.
    fn setup(name: &str) -> Act {
        let d = std::env::temp_dir().join(format!("kestrel_act_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let notes = d.join("notes");
        fs::create_dir_all(notes.join("meetings")).unwrap();
        fs::create_dir_all(notes.join("docs")).unwrap();
        let write = |n: &str, t: &str| fs::write(notes.join(n), t).unwrap();
        write(
            "meetings/2026-10-15-kickoff.md",
            "tags: meeting\nKickoff.\n- [ ] not in the last three\n",
        );
        write(
            "meetings/2026-10-20-standup.md",
            "tags: meeting\nStandup: budget review moved.\n- [ ] email the budget to asha\n",
        );
        write(
            "meetings/2026-10-21-design.md",
            "tags: meeting\nDesign review.\n- [ ] book a room for the demo\nTODO: send the slides\n",
        );
        write(
            "meetings/2026-10-22-planning.md",
            "tags: meeting\nPlanning.\n- [ ] email the budget to asha\naction: draft the q4 plan\n",
        );
        write(
            "docs/project-plan.md",
            "Project plan\nMilestone 1: the router.\n",
        );
        write("2026-10-22-groceries.md", "milk, eggs\n- [ ] buy milk\n");
        Act::open(&notes, &d.join("memory.db"))
            .unwrap()
            .with_utc_offset(0)
    }

    fn one(intent: Intent, text: &str) -> Vec<(Intent, String)> {
        vec![(intent, text.to_string())]
    }

    fn joined(lines: &[String]) -> String {
        lines.join("\n")
    }

    #[test]
    fn tomorrow_is_counted_from_the_local_date() {
        // 2026-10-24 20:00 UTC = 2026-10-25 01:30 in India: "tomorrow" is the 26th, not the 25th.
        let mut a = setup("local").with_utc_offset(19_800);
        let late = Date::parse("2026-10-24").unwrap().days() * 86_400 + 20 * 3600;
        let out = a.carry_out(
            Some(&one(Intent::CreateTask, "remind me to call asha tomorrow")),
            1,
            late,
            &yes(),
        );
        assert!(joined(&out).contains("2026-10-26"), "{out:?}");
    }

    #[test]
    fn a_task_is_created_with_its_due_date() {
        let mut a = setup("create");
        let out = a.carry_out(
            Some(&one(Intent::CreateTask, "remind me to call asha tomorrow")),
            1,
            NOW,
            &yes(),
        );
        assert_eq!(out, ["created task #1: call asha (due 2026-10-24)"]);
    }

    #[test]
    fn a_time_said_goes_onto_the_task_not_into_its_title() {
        let ten_am = NOW + 10 * 3600;
        let mut a = setup("time");
        let create = |a: &mut Act, text: &str, now: i64| {
            a.carry_out(Some(&one(Intent::CreateTask, text)), 1, now, &yes())
        };
        assert_eq!(
            create(&mut a, "remind me to call asha at 5 pm", ten_am),
            ["created task #1: call asha (due 2026-10-23 17:00)"]
        );
        // Already past today: tomorrow.
        assert_eq!(
            create(&mut a, "remind me to buy milk at 9:30 am", ten_am),
            ["created task #2: buy milk (due 2026-10-24 09:30)"]
        );
        assert_eq!(
            create(
                &mut a,
                "remind me to pay rent tomorrow at 6 o'clock",
                ten_am
            ),
            ["created task #3: pay rent (due 2026-10-24 18:00)"]
        );
        assert_eq!(
            create(
                &mut a,
                "kal shaam 6 baje asha ko call karna yaad dilana",
                ten_am
            ),
            ["created task #4: asha ko call karna (due 2026-10-24 18:00)"]
        );
        let row = a.backend.memory().tasks().unwrap().remove(0);
        assert_eq!(
            (row.due_time.as_deref(), row.notified_at),
            (Some("17:00"), None)
        );
    }

    #[test]
    fn two_tasks_get_their_own_times_and_other_requests_keep_theirs() {
        let ten_am = NOW + 10 * 3600;
        let mut a = setup("two_times");
        let parts = vec![
            (
                Intent::CreateTask,
                "remind me to call asha at 5 pm".to_string(),
            ),
            (Intent::CreateTask, "buy milk at 7 pm".to_string()),
        ];
        let out = a.carry_out(Some(&parts), 1, ten_am, &yes());
        assert_eq!(
            out,
            [
                "created task #1: call asha (due 2026-10-23 17:00)",
                "created task #2: buy milk (due 2026-10-23 19:00)"
            ]
        );
        // Only task requests lose their time words.
        let out = joined(&a.carry_out(
            Some(&one(
                Intent::SearchNotes,
                "search my notes for budget at 5 pm",
            )),
            2,
            ten_am,
            &yes(),
        ));
        assert_eq!(out, "no notes matched", "the query kept \"at 5 pm\"");
    }

    #[test]
    fn a_search_lists_the_notes_it_found() {
        let mut a = setup("search");
        let out = joined(&a.carry_out(
            Some(&one(Intent::SearchNotes, "search my notes for budget")),
            1,
            NOW,
            &yes(),
        ));
        assert!(out.starts_with("found 2 notes:"), "{out}");
        assert!(
            out.contains("meetings/2026-10-20-standup.md")
                && out.contains("meetings/2026-10-22-planning.md")
        );
    }

    #[test]
    fn a_document_is_opened_and_shown() {
        let mut a = setup("read");
        assert!(a.documents().contains(&"docs/project-plan.md".to_string()));
        let out = joined(&a.carry_out(
            Some(&one(Intent::ReadDocument, "open project-plan.md")),
            1,
            NOW,
            &yes(),
        ));
        assert!(
            out.contains("docs/project-plan.md") && out.contains("Milestone 1: the router."),
            "{out}"
        );
    }

    #[test]
    fn a_delete_asks_first_and_a_no_keeps_the_task() {
        let mut a = setup("delete");
        a.carry_out(
            Some(&one(Intent::CreateTask, "remind me to buy milk")),
            1,
            NOW,
            &yes(),
        );
        let no = Say(false, RefCell::default());
        let out = a.carry_out(Some(&one(Intent::DeleteTask, "delete task 1")), 2, NOW, &no);
        assert_eq!(out, ["kept task #1 (you said no)"]);
        assert_eq!(no.1.borrow().len(), 1, "asked once");
        let out = a.carry_out(
            Some(&one(Intent::DeleteTask, "delete task 1")),
            3,
            NOW,
            &yes(),
        );
        assert_eq!(out, ["deleted task #1"]);
    }

    #[test]
    fn a_missing_slot_is_asked_for() {
        let mut a = setup("clarify");
        let out = a.carry_out(Some(&one(Intent::CreateTask, "remind me")), 1, NOW, &yes());
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("What should the task say?"), "{out:?}");
    }

    #[test]
    fn a_compound_request_runs_its_joined_plan() {
        let mut a = setup("compound");
        let parts = vec![
            (
                Intent::SummarizeNotes,
                "summarize my last three meeting notes".to_string(),
            ),
            (Intent::CreateTask, "create a task list".to_string()),
        ];
        let asked = yes();
        let out = joined(&a.carry_out(Some(&parts), 1, NOW, &asked));
        assert!(
            out.contains("summary: Planning. Design review. Standup: budget review moved."),
            "{out}"
        );
        assert!(
            out.contains("created task #1: email the budget to asha"),
            "{out}"
        );
        assert!(out.contains("created task #4: send the slides"), "{out}");
        assert_eq!(asked.1.borrow().as_slice(), ["create 4 tasks?"]);
    }

    #[test]
    fn an_empty_memory_answer_says_so() {
        let mut a = setup("memory");
        let out = a.carry_out(
            Some(&one(
                Intent::RetrieveMemory,
                "what do you remember about asha",
            )),
            1,
            NOW,
            &yes(),
        );
        assert_eq!(out, ["I don't remember anything about that."]);
    }

    #[test]
    fn a_single_request_no_template_fits_asks_for_the_notes() {
        let mut a = setup("unclear");
        let out = joined(&a.carry_out(
            Some(&one(Intent::SummarizeNotes, "summarize my planning")),
            1,
            NOW,
            &yes(),
        ));
        assert!(
            out.starts_with("I couldn't tell which notes you mean."),
            "{out}"
        );
    }

    #[test]
    fn chitchat_out_of_scope_and_unrouted_requests_run_nothing() {
        let mut a = setup("none");
        let hi = joined(&a.carry_out(Some(&one(Intent::Chitchat, "thanks")), 1, NOW, &yes()));
        assert!(hi.contains("notes") && hi.contains("tasks"), "{hi}");
        let oos = joined(&a.carry_out(
            Some(&one(Intent::Unsupported, "book me a flight")),
            2,
            NOW,
            &yes(),
        ));
        assert!(oos.starts_with("I can't do that"), "{oos}");
        let unsure = joined(&a.carry_out(None, 3, NOW, &yes()));
        assert!(unsure.starts_with("I'm not sure"), "{unsure}");
    }
}
