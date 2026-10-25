//! The app's task commands (ADR-034): listing, finishing, renaming and rescheduling tasks, in
//! English and Romanized Hinglish, recognized by rule before the cascade runs. The research
//! classifier, its datasets and the frozen configs know nothing of them.
//!
//! - "show my tasks", "mere tasks dikhao", "what's due today", "aaj kya karna hai", "overdue tasks"
//! - "task 2 done", "mark task 2 as done", "task 3 ho gaya", "task 2 aur 5 ho gaye": finished tasks
//!   leave the list without a question (the user said it; delete still asks y/N)
//! - "rename task 3 to call ravi", "task 4 ka naam doctor ko call karna rakho"
//! - "move task 3 to friday", "task 5 kal tak kar do", "task 3 ki date hata do"
//! - "task 3 at 5 pm", "move task 3 to friday at 5 pm", "task 2 kal shaam 6 baje kar do": a time
//!   (src/times.rs), with or without a new date
//!
//! Anything that says delete / remove / hata (without "date") stays the cascade's, as do requests
//! that create tasks. `join_words` turns ", also" / ", now" / ", plus" / "; " into "and" so the
//! router's multi-intent signal sees two requests (the research conjunction list is unchanged).

use kestrel_core::KestrelError;
use kestrel_memory::{Memory, TaskRow};
use kestrel_tools::date::Date;

use crate::times::{self, Said};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    All,
    Today,
    Overdue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    List(Which),
    Done(Vec<i64>),
    Rename(i64, String),
    /// None clears the date.
    Reschedule(i64, Option<Date>),
    /// A time, and a new date if one was said.
    At(i64, Option<Date>, Said),
}

pub fn join_words(text: &str) -> String {
    let mut s = text.to_string();
    for (from, to) in [
        (", also ", " and "),
        (", Also ", " and "),
        (", now ", " and "),
        (", Now ", " and "),
        (", plus ", " and "),
        ("; ", " and "),
    ] {
        s = s.replace(from, to);
    }
    s
}

/// Lowercased words without surrounding punctuation; "#3" is "3".
fn words(text: &str) -> Vec<String> {
    let trim = |c: char| matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '"' | '(' | ')');
    text.split_whitespace()
        .map(|w| w.trim_matches(trim).trim_start_matches('#').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

const TASK: &[&str] = &["task", "tasks", "todo", "todos", "to-do"];
const DELETE: &[&str] = &[
    "delete", "remove", "hata", "hatao", "mita", "mitao", "cancel", "drop",
];
const DATE: &[&str] = &["date", "due", "deadline"];
const CREATE: &[&str] = &[
    "add", "create", "make", "remind", "set", "put", "bana", "banao", "banana", "daal", "daalo",
    "likh", "likho", "yaad", "turn",
];
const DONE: &[&str] = &[
    "done",
    "finished",
    "finish",
    "complete",
    "completed",
    "khatam",
    "did",
];
const SHOW: &[&str] = &[
    "show", "list", "what", "whats", "what's", "dikhao", "batao", "kya", "kaun", "pending", "see",
    "view", "display",
];
/// "my tasks", "mere tasks": a list without a verb.
const BARE_LIST: &[&str] = &[
    "my", "mere", "meri", "mera", "all", "the", "tasks", "task", "todo", "todos", "sab", "saare",
];
const EN_EDIT: &[&str] = &[
    "rename",
    "change",
    "edit",
    "update",
    "move",
    "postpone",
    "push",
    "reschedule",
    "shift",
];
/// Words that may stand around a new date ("to friday", "kal tak kar do").
const DATE_FILLER: &[&str] = &[
    "to", "for", "until", "till", "on", "by", "the", "ko", "tak", "kar", "do", "karo", "shift",
    "karna", "ki", "date", "par", "pe", "move",
];

pub fn parse(text: &str, today: Date) -> Option<Command> {
    let Some((rest, said)) = times::take(text) else {
        return parse_words(&words(text), today);
    };
    let rest = words(&rest);
    if let Some(cmd) = timed(&rest, today, said) {
        return Some(cmd);
    }
    // Otherwise a time is never read as a task number; a new title keeps all its words.
    match parse_words(&rest, today)? {
        Command::Rename(..) => parse_words(&words(text), today),
        cmd => Some(cmd),
    }
}

/// "task 3 at 5 pm", "set task 3 for 5pm", "move task 3 to friday at 5 pm", "task 2 kal shaam 6
/// baje kar do": the words left once the time is out are one task and maybe a date.
fn timed(w: &[String], today: Date, said: Said) -> Option<Command> {
    let i = w.iter().position(|x| x == "task")?;
    let id = w.get(i + 1)?.parse::<i64>().ok().filter(|n| *n > 0)?;
    let lead = |x: &String| x != "rename" && (EN_EDIT.contains(&x.as_str()) || x == "set");
    if !w[..i].iter().all(lead) {
        return None;
    }
    let (due, left) = kestrel_tools::templates::split_due(&w[i + 2..].join(" "), today);
    left.iter()
        .all(|x| DATE_FILLER.contains(&x.as_str()) || x == "time")
        .then_some(Command::At(id, due, said))
}

fn parse_words(w: &[String], today: Date) -> Option<Command> {
    let any = |set: &[&str]| w.iter().any(|x| set.contains(&x.as_str()));
    let pair =
        |a: &str, b: &[&str]| (1..w.len()).any(|i| w[i - 1] == a && b.contains(&w[i].as_str()));
    let ids: Vec<i64> = w
        .iter()
        .filter_map(|x| x.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .collect();
    let tasky = any(TASK);

    if tasky && any(DATE) && any(DELETE) && ids.len() == 1 {
        return Some(Command::Reschedule(ids[0], None));
    }
    if any(DELETE) || any(CREATE) {
        return None; // the cascade's: delete asks y/N, creation has its templates
    }
    let done = any(DONE)
        || pair("ho", &["gaya", "gayi", "gaye", "chuka", "chuki"])
        || pair("kar", &["liya", "li", "diya", "di"]);
    if tasky && done && !ids.is_empty() {
        return Some(Command::Done(ids));
    }
    if let Some(cmd) = edit(w, today) {
        return Some(cmd);
    }
    if !ids.is_empty() {
        return None;
    }
    if any(&["overdue", "late", "missed"]) && (tasky || any(SHOW)) {
        return Some(Command::List(Which::Overdue));
    }
    let today_word = any(&["today", "aaj", "today's"]);
    if today_word && (tasky || any(&["due"]) || (any(&["kya"]) && any(&["karna"]))) {
        return Some(Command::List(Which::Today));
    }
    let bare = tasky && w.iter().all(|x| BARE_LIST.contains(&x.as_str()));
    if (tasky && any(SHOW)) || bare || (any(&["pending"]) && any(&["kya"])) {
        return Some(Command::List(Which::All));
    }
    None
}

/// Rename / reschedule one task: "rename task 3 to X", "move task 3 to friday", "task 4 ka naam X
/// rakho", "task 5 kal tak kar do". A "change task N to ..." whose rest is a date reschedules.
fn edit(w: &[String], today: Date) -> Option<Command> {
    let id = |i: usize| w.get(i).and_then(|x| x.parse::<i64>().ok());
    let is_date = |rest: &[String]| {
        let (due, left) = kestrel_tools::templates::split_due(&rest.join(" "), today);
        due.filter(|_| left.iter().all(|x| DATE_FILLER.contains(&x.as_str())))
    };
    if w.len() >= 3 && EN_EDIT.contains(&w[0].as_str()) && w[1] == "task" {
        let n = id(2)?;
        let mut rest = &w[3..];
        if rest
            .first()
            .is_some_and(|x| matches!(x.as_str(), "to" | "as"))
        {
            rest = &rest[1..];
        }
        if rest.is_empty() {
            return None;
        }
        if w[0] != "rename"
            && let Some(d) = is_date(rest)
        {
            return Some(Command::Reschedule(n, Some(d)));
        }
        return matches!(w[0].as_str(), "rename" | "change" | "edit" | "update")
            .then(|| Command::Rename(n, rest.join(" ")));
    }
    if w.len() >= 3 && w[0] == "task" {
        let n = id(1)?;
        if w.get(2).map(String::as_str) == Some("ka")
            && w.get(3).map(String::as_str) == Some("naam")
        {
            let body = &w[4..];
            let cut = [&["rakh", "do"][..], &["kar", "do"], &["rakho"], &["karo"]]
                .iter()
                .find(|s| body.ends_with(&s.iter().map(|x| x.to_string()).collect::<Vec<_>>()))
                .map_or(0, |s| s.len());
            let title = &body[..body.len() - cut];
            return (!title.is_empty()).then(|| Command::Rename(n, title.join(" ")));
        }
        return is_date(&w[2..]).map(|d| Command::Reschedule(n, Some(d)));
    }
    None
}

/// `minute`: minutes after local midnight now.
pub fn run(
    cmd: &Command,
    memory: &mut Memory,
    today: Date,
    minute: u32,
) -> Result<Vec<String>, KestrelError> {
    let tasks = memory.tasks()?;
    let title_of = |id: i64| tasks.iter().find(|t| t.id == id).map(|t| t.title.clone());
    Ok(match cmd {
        Command::List(which) => list(&tasks, *which, today),
        Command::Done(ids) => {
            let mut out = Vec::new();
            for &id in ids {
                match title_of(id) {
                    Some(t) => {
                        memory.delete_task(id)?;
                        out.push(format!("done: #{id} {t}"));
                    }
                    None => out.push(format!("there is no task {id}")),
                }
            }
            out
        }
        Command::Rename(id, title) => match kestrel_tools::args::clean_text(title, 200, "title") {
            Err(e) => vec![format!("that title won't do: {e}")],
            Ok(t) if memory.update_task(*id, Some(&t), None)? => {
                vec![format!("task #{id} is now: {t}")]
            }
            Ok(_) => vec![format!("there is no task {id}")],
        },
        Command::Reschedule(id, due) => {
            let d = due.map(|d| d.to_string());
            if !memory.update_task(*id, None, Some(d.as_deref()))? {
                vec![format!("there is no task {id}")]
            } else if let Some(d) = d {
                vec![format!("task #{id} now due {d}")]
            } else {
                vec![format!("task #{id} has no date now")]
            }
        }
        Command::At(id, due, said) => {
            let Some(t) = tasks.iter().find(|t| t.id == *id) else {
                return Ok(vec![format!("there is no task {id}")]);
            };
            // An overdue date is no anchor: the time is counted as for an undated task.
            let had = t.due.as_deref().and_then(Date::parse);
            let base = due.or(had.filter(|d| *d >= today));
            let (date, m) = times::resolve(*said, base, today, minute);
            if had != Some(date) {
                memory.update_task(*id, None, Some(Some(&date.to_string())))?;
            }
            let hhmm = times::hhmm(m);
            memory.set_task_time(*id, Some(&hhmm))?;
            vec![format!("task #{id} now due {date} {hhmm}")]
        }
    })
}

fn list(tasks: &[TaskRow], which: Which, today: Date) -> Vec<String> {
    let mut dated: Vec<(Date, &TaskRow)> = tasks
        .iter()
        .filter_map(|t| t.due.as_deref().and_then(Date::parse).map(|d| (d, t)))
        .collect();
    // Within a day, timed tasks first, by time.
    dated.sort_by_key(|(d, t)| (*d, t.due_time.is_none(), t.due_time.clone(), t.id));
    let group = |pick: &dyn Fn(Date) -> bool, with_date: bool| -> Vec<String> {
        dated
            .iter()
            .filter(|(d, _)| pick(*d))
            .map(|(d, t)| {
                let when: Vec<String> = [with_date.then(|| d.to_string()), t.due_time.clone()]
                    .into_iter()
                    .flatten()
                    .collect();
                if when.is_empty() {
                    format!("  #{} {}", t.id, t.title)
                } else {
                    format!("  #{} {} ({})", t.id, t.title, when.join(" "))
                }
            })
            .collect()
    };
    let undated: Vec<String> = tasks
        .iter()
        .filter(|t| t.due.as_deref().and_then(Date::parse).is_none())
        .map(|t| format!("  #{} {}", t.id, t.title))
        .collect();
    let mut sections = vec![("overdue:", group(&|d| d < today, true))];
    if which != Which::Overdue {
        sections.push(("today:", group(&|d| d == today, false)));
    }
    if which == Which::All {
        sections.push(("upcoming:", group(&|d| d > today, true)));
        sections.push(("no date:", undated));
    }
    let mut out = Vec::new();
    for (head, lines) in sections.into_iter().filter(|(_, l)| !l.is_empty()) {
        out.push(head.to_string());
        out.extend(lines);
    }
    if out.is_empty() {
        out.push(
            match which {
                Which::All => "no tasks",
                Which::Today => "nothing due today",
                Which::Overdue => "nothing overdue",
            }
            .to_string(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Sunday.
    fn today() -> Date {
        Date::parse("2026-10-25").unwrap()
    }

    fn p(text: &str) -> Option<Command> {
        parse(text, today())
    }

    fn d(s: &str) -> Option<Date> {
        Date::parse(s)
    }

    #[test]
    fn listing_in_both_languages() {
        for t in [
            "show my tasks",
            "my tasks",
            "list tasks",
            "what are my tasks",
            "mere tasks dikhao",
            "tasks dikhao",
            "kya kya pending hai",
        ] {
            assert_eq!(p(t), Some(Command::List(Which::All)), "{t}");
        }
        for t in [
            "what's due today",
            "aaj ke tasks",
            "aaj kya karna hai",
            "today's tasks",
            "show tasks for today",
        ] {
            assert_eq!(p(t), Some(Command::List(Which::Today)), "{t}");
        }
        for t in ["what's overdue", "overdue tasks", "show my overdue tasks"] {
            assert_eq!(p(t), Some(Command::List(Which::Overdue)), "{t}");
        }
    }

    #[test]
    fn finishing_one_or_several_tasks() {
        for (t, ids) in [
            ("task 2 done", vec![2]),
            ("Mark task 2 as done.", vec![2]),
            ("i finished task 4", vec![4]),
            ("task 3 ho gaya", vec![3]),
            ("task 2 aur 5 ho gaye", vec![2, 5]),
            ("done with task 7", vec![7]),
            ("task #3 complete", vec![3]),
        ] {
            assert_eq!(p(t), Some(Command::Done(ids)), "{t}");
        }
    }

    #[test]
    fn renaming_and_rescheduling() {
        assert_eq!(
            p("rename task 3 to call ravi"),
            Some(Command::Rename(3, "call ravi".into()))
        );
        assert_eq!(
            p("change task 2 to buy oat milk"),
            Some(Command::Rename(2, "buy oat milk".into()))
        );
        assert_eq!(
            p("task 4 ka naam doctor ko call karna rakho"),
            Some(Command::Rename(4, "doctor ko call karna".into()))
        );
        assert_eq!(
            p("move task 3 to friday"),
            Some(Command::Reschedule(3, d("2026-10-30")))
        );
        assert_eq!(
            p("change task 2 to tomorrow"),
            Some(Command::Reschedule(2, d("2026-10-26")))
        );
        assert_eq!(
            p("task 5 kal tak kar do"),
            Some(Command::Reschedule(5, d("2026-10-26")))
        );
        assert_eq!(
            p("postpone task 1 to 2026-11-02"),
            Some(Command::Reschedule(1, d("2026-11-02")))
        );
        assert_eq!(
            p("remove the date from task 3"),
            Some(Command::Reschedule(3, None))
        );
        assert_eq!(
            p("task 3 ki date hata do"),
            Some(Command::Reschedule(3, None))
        );
    }

    #[test]
    fn a_time_for_one_task() {
        let five = Said::At(17 * 60);
        assert_eq!(p("task 3 at 5 pm"), Some(Command::At(3, None, five)));
        assert_eq!(p("set task 3 for 5pm"), Some(Command::At(3, None, five)));
        assert_eq!(
            p("move task 3 to friday at 5 pm"),
            Some(Command::At(3, d("2026-10-30"), five))
        );
        assert_eq!(
            p("task 2 kal shaam 6 baje kar do"),
            Some(Command::At(2, d("2026-10-26"), Said::At(18 * 60)))
        );
        assert_eq!(
            p("task 4 ko 6 baje"),
            Some(Command::At(4, None, Said::Either(360)))
        );
        // A rename keeps the words; a delete and a new task stay the cascade's.
        assert_eq!(
            p("rename task 3 to call ravi at 5 pm"),
            Some(Command::Rename(3, "call ravi at 5 pm".into()))
        );
        for t in [
            "delete task 2 at 5 pm",
            "add a task to buy milk at 5 pm",
            "remind me to call asha at 5 pm",
            "at 5 pm",
        ] {
            assert_eq!(p(t), None, "{t}");
        }
    }

    #[test]
    fn deletes_and_new_tasks_stay_the_cascades() {
        for t in [
            "delete task 2",
            "task 2 hata do",
            "i finished task 1, delete it",
            "add a task to buy milk",
            "make tasks from my groceries note",
            "create a task list",
            "remind me to call asha tomorrow",
            "search my notes for tasks",
            "summarize my last three meeting notes",
        ] {
            assert_eq!(p(t), None, "{t}");
        }
    }

    #[test]
    fn also_and_now_join_two_requests() {
        assert_eq!(
            join_words("find my budget notes, also remind me to call ravi"),
            "find my budget notes and remind me to call ravi"
        );
        assert_eq!(
            join_words("summarize my latest note, now delete task 2"),
            "summarize my latest note and delete task 2"
        );
        assert_eq!(
            join_words("open the plan; search budget"),
            "open the plan and search budget"
        );
        assert_eq!(
            join_words("remind me to call asha"),
            "remind me to call asha"
        );
    }

    fn memory() -> Memory {
        let mut m = Memory::in_memory().unwrap();
        m.create_task("call the plumber", None, 0).unwrap(); // 1
        m.create_task("pay rent", Some("2026-10-25"), 0).unwrap(); // 2
        m.create_task("renew passport", Some("2026-11-01"), 0)
            .unwrap(); // 3
        m.create_task("file taxes", Some("2026-10-20"), 0).unwrap(); // 4
        m
    }

    #[test]
    fn lists_group_tasks_by_when_they_are_due() {
        let mut m = memory();
        assert_eq!(
            run(&Command::List(Which::All), &mut m, today(), 600).unwrap(),
            [
                "overdue:",
                "  #4 file taxes (2026-10-20)",
                "today:",
                "  #2 pay rent",
                "upcoming:",
                "  #3 renew passport (2026-11-01)",
                "no date:",
                "  #1 call the plumber"
            ]
        );
        assert_eq!(
            run(&Command::List(Which::Today), &mut m, today(), 600).unwrap(),
            [
                "overdue:",
                "  #4 file taxes (2026-10-20)",
                "today:",
                "  #2 pay rent"
            ]
        );
        assert_eq!(
            run(&Command::List(Which::Overdue), &mut m, today(), 600).unwrap(),
            ["overdue:", "  #4 file taxes (2026-10-20)"]
        );
        let mut empty = Memory::in_memory().unwrap();
        assert_eq!(
            run(&Command::List(Which::All), &mut empty, today(), 600).unwrap(),
            ["no tasks"]
        );
        assert_eq!(
            run(&Command::List(Which::Today), &mut empty, today(), 600).unwrap(),
            ["nothing due today"]
        );
    }

    #[test]
    fn done_rename_and_reschedule_change_the_memory_file() {
        let mut m = memory();
        assert_eq!(
            run(&Command::Done(vec![2, 9]), &mut m, today(), 600).unwrap(),
            ["done: #2 pay rent", "there is no task 9"]
        );
        assert_eq!(
            run(
                &Command::Rename(3, "renew passport online".into()),
                &mut m,
                today(),
                600
            )
            .unwrap(),
            ["task #3 is now: renew passport online"]
        );
        assert_eq!(
            run(
                &Command::Reschedule(1, d("2026-10-30")),
                &mut m,
                today(),
                600
            )
            .unwrap(),
            ["task #1 now due 2026-10-30"]
        );
        assert_eq!(
            run(&Command::Reschedule(4, None), &mut m, today(), 600).unwrap(),
            ["task #4 has no date now"]
        );
        let rows: Vec<_> = m
            .tasks()
            .unwrap()
            .into_iter()
            .map(|t| (t.id, t.title, t.due))
            .collect();
        assert_eq!(
            rows,
            [
                (1, "call the plumber".into(), Some("2026-10-30".into())),
                (3, "renew passport online".into(), Some("2026-11-01".into())),
                (4, "file taxes".into(), None)
            ]
        );
    }

    #[test]
    fn a_time_is_set_and_listed_in_time_order() {
        let mut m = memory();
        let ten_am = 600;
        let at = |id, due, said| Command::At(id, due, said);
        // Undated: today while the time is ahead.
        assert_eq!(
            run(&at(1, None, Said::At(17 * 60)), &mut m, today(), ten_am).unwrap(),
            ["task #1 now due 2026-10-25 17:00"]
        );
        // Dated today: stays today.
        assert_eq!(
            run(&at(2, None, Said::Either(9 * 60)), &mut m, today(), ten_am).unwrap(),
            ["task #2 now due 2026-10-25 21:00"]
        );
        // Dated later: keeps its date, daytime reading.
        assert_eq!(
            run(&at(3, None, Said::Either(9 * 60)), &mut m, today(), ten_am).unwrap(),
            ["task #3 now due 2026-11-01 09:00"]
        );
        // Overdue: counted like an undated task.
        assert_eq!(
            run(&at(4, None, Said::At(8 * 60)), &mut m, today(), ten_am).unwrap(),
            ["task #4 now due 2026-10-26 08:00"]
        );
        assert_eq!(
            run(&at(9, None, Said::At(8 * 60)), &mut m, today(), ten_am).unwrap(),
            ["there is no task 9"]
        );
        m.create_task("buy milk", Some("2026-10-25"), 0).unwrap(); // 5, no time
        assert_eq!(
            run(&Command::List(Which::All), &mut m, today(), ten_am).unwrap(),
            [
                "today:",
                "  #1 call the plumber (17:00)",
                "  #2 pay rent (21:00)",
                "  #5 buy milk",
                "upcoming:",
                "  #4 file taxes (2026-10-26 08:00)",
                "  #3 renew passport (2026-11-01 09:00)"
            ]
        );
    }
}
