//! `kestrel remind [--notify] [--db PATH]` (ADR-034): the tasks due today and the overdue ones,
//! from the memory file, by the local date. With `--notify` the same goes to a desktop
//! notification (`send`; nothing when nothing is due), which is how the systemd user timer
//! of scripts/install_reminders.sh runs it every morning and at login. No model is loaded.
//!
//! A task with a time of day rings at that time from `kestrel serve`: `ring_forever` checks every
//! half minute on its own connection to the memory file, so a long `kestrel chat` does not hold it
//! up, and each task rings once (`notified_at`).

use std::error::Error;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use kestrel_memory::{Memory, TaskRow};
use kestrel_tools::date::Date;

/// At most this many tasks are listed in a notification; the rest are counted.
pub const NOTIFY_MAX: usize = 6;
/// How often the service looks for tasks whose time has come.
pub const RING_EVERY: Duration = Duration::from_secs(30);

/// Shows a notification: (title, body).
pub type Notify<'a> = dyn FnMut(&str, &str) -> Result<(), Box<dyn Error>> + 'a;

/// The tasks with a due date on or before `today`: overdue first (oldest first), then today's;
/// within a day, timed tasks first, by time.
pub struct Due<'a> {
    pub overdue: Vec<&'a TaskRow>,
    pub today: Vec<&'a TaskRow>,
}

pub fn due(tasks: &[TaskRow], today: Date) -> Due<'_> {
    let mut dated: Vec<(Date, &TaskRow)> = tasks
        .iter()
        .filter_map(|t| t.due.as_deref().and_then(Date::parse).map(|d| (d, t)))
        .filter(|(d, _)| *d <= today)
        .collect();
    dated.sort_by_key(|(d, t)| (*d, t.due_time.is_none(), t.due_time.clone(), t.id));
    let (overdue, today_): (Vec<_>, Vec<_>) = dated.into_iter().partition(|(d, _)| *d < today);
    Due {
        overdue: overdue.into_iter().map(|(_, t)| t).collect(),
        today: today_.into_iter().map(|(_, t)| t).collect(),
    }
}

/// What `kestrel remind` prints.
pub fn lines(due: &Due) -> Vec<String> {
    let mut out = Vec::new();
    if !due.overdue.is_empty() {
        out.push("overdue:".to_string());
        for t in &due.overdue {
            let when = [t.due.as_deref(), t.due_time.as_deref()];
            let when: Vec<&str> = when.into_iter().flatten().collect();
            out.push(format!("  #{} {} (due {})", t.id, t.title, when.join(" ")));
        }
    }
    if !due.today.is_empty() {
        out.push("due today:".to_string());
        out.extend(due.today.iter().map(|t| match &t.due_time {
            Some(h) => format!("  #{} {} ({h})", t.id, t.title),
            None => format!("  #{} {}", t.id, t.title),
        }));
    }
    if out.is_empty() {
        out.push("nothing due today".to_string());
    }
    out
}

/// (title, body) of the notification, or None when nothing is due.
pub fn notification(due: &Due) -> Option<(String, String)> {
    let (n_today, n_over) = (due.today.len(), due.overdue.len());
    let title = match (n_today, n_over) {
        (0, 0) => return None,
        (t, 0) => format!("Kestrel: {t} due today"),
        (0, o) => format!("Kestrel: {o} overdue"),
        (t, o) => format!("Kestrel: {t} due today, {o} overdue"),
    };
    let all: Vec<String> = due
        .overdue
        .iter()
        .map(|t| format!("overdue: {}", t.title))
        .chain(due.today.iter().map(|t| t.title.clone()))
        .collect();
    Some((title, capped(&all)))
}

/// (title, body) of the notification for the tasks ringing now, or None.
pub fn alarm(ringing: &[&TaskRow], today: Date) -> Option<(String, String)> {
    let title = match ringing.len() {
        0 => return None,
        1 => "Kestrel reminder".to_string(),
        n => format!("Kestrel: {n} reminders"),
    };
    let all: Vec<String> = ringing
        .iter()
        .map(|t| {
            let time = t.due_time.as_deref().unwrap_or("");
            match t.due.as_deref() {
                Some(d) if Date::parse(d) != Some(today) => format!("{d} {time} {}", t.title),
                _ => format!("{time} {}", t.title),
            }
        })
        .collect();
    Some((title, capped(&all)))
}

/// The first `NOTIFY_MAX` lines and how many more there are.
fn capped(all: &[String]) -> String {
    let mut body: Vec<String> = all.iter().take(NOTIFY_MAX).cloned().collect();
    if all.len() > NOTIFY_MAX {
        body.push(format!("and {} more", all.len() - NOTIFY_MAX));
    }
    body.join("\n")
}

/// The service's check: one notification for the timed tasks whose time has come, each marked so
/// it rings once (only once the notification was shown). The ids rung.
pub fn ring(
    memory: &mut Memory,
    today: Date,
    minute: u32,
    now: i64,
    send: &mut Notify,
) -> Result<Vec<i64>, Box<dyn Error>> {
    let tasks = memory.tasks()?;
    let mut come: Vec<((Date, u32), &TaskRow)> = tasks
        .iter()
        .filter(|t| t.notified_at.is_none())
        .filter_map(|t| {
            let date = Date::parse(t.due.as_deref()?)?;
            let m = crate::times::minutes(t.due_time.as_deref()?)?;
            Some(((date, m), t))
        })
        .filter(|(at, _)| *at <= (today, minute))
        .collect();
    come.sort_by_key(|(at, t)| (*at, t.id));
    let ringing: Vec<&TaskRow> = come.into_iter().map(|(_, t)| t).collect();
    let Some((title, body)) = alarm(&ringing, today) else {
        return Ok(vec![]);
    };
    send(&title, &body)?;
    for t in &ringing {
        memory.mark_notified(t.id, now)?;
    }
    Ok(ringing.iter().map(|t| t.id).collect())
}

/// The service's reminder thread: `ring` every `RING_EVERY` until the process ends. Its log has
/// task ids, never titles.
pub fn ring_forever(db: &Path) {
    let mut memory = match crate::memory::open(db) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("timed reminders are off: {e}");
            return;
        }
    };
    let offset = crate::clock::utc_offset();
    loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let today = crate::clock::local_date(now, offset);
        let minute = crate::clock::local_minute(now, offset);
        match ring(&mut memory, today, minute, now, &mut send) {
            Ok(ids) if !ids.is_empty() => eprintln!("reminded: task {ids:?}"),
            Ok(_) => {}
            Err(e) => eprintln!("a reminder could not be shown (trying again): {e}"),
        }
        std::thread::sleep(RING_EVERY);
    }
}

/// A desktop notification (kestrel-platform: notify-send on Linux, osascript on macOS; on Windows
/// the Kestrel app shows them, v4 phase A3).
pub fn send(title: &str, body: &str) -> Result<(), Box<dyn Error>> {
    let mut c = kestrel_platform::notify(kestrel_platform::Os::current(), title, body)
        .ok_or("desktop notifications on this system come with the Kestrel app (v4 phase A3)")?;
    let status = c.status()?;
    if !status.success() {
        return Err(format!("{} failed: {status}", c.get_program().to_string_lossy()).into());
    }
    Ok(())
}

pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let notify = args.iter().any(|a| a == "--notify");
    let db = match args.iter().position(|a| a == "--db") {
        Some(i) => args
            .get(i + 1)
            .map(std::path::PathBuf::from)
            .ok_or("--db needs a path")?,
        None => crate::memory::default_db(kestrel_platform::Os::current(), &kestrel_platform::env)?,
    };
    if !db.exists() {
        writeln!(out, "no tasks yet ({} does not exist)", db.display())?;
        return Ok(());
    }
    let tasks = crate::memory::open(&db)?.tasks()?;
    let due = due(&tasks, crate::clock::today());
    for line in lines(&due) {
        writeln!(out, "{line}")?;
    }
    if notify && let Some((title, body)) = notification(&due) {
        send(&title, &body)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: i64, title: &str, due: Option<&str>) -> TaskRow {
        TaskRow {
            id,
            title: title.into(),
            due: due.map(Into::into),
            created_at: 0,
            due_time: None,
            notified_at: None,
        }
    }

    fn timed(id: i64, title: &str, due: &str, time: &str) -> TaskRow {
        TaskRow {
            due_time: Some(time.into()),
            ..task(id, title, Some(due))
        }
    }

    #[test]
    fn times_are_printed_and_come_first_in_their_day() {
        let tasks = vec![
            task(2, "pay rent", Some("2026-10-25")),
            timed(6, "call asha", "2026-10-25", "17:30"),
            timed(7, "standup", "2026-10-25", "09:15"),
            timed(4, "file taxes", "2026-10-20", "08:00"),
        ];
        assert_eq!(
            lines(&due(&tasks, today())),
            [
                "overdue:",
                "  #4 file taxes (due 2026-10-20 08:00)",
                "due today:",
                "  #7 standup (09:15)",
                "  #6 call asha (17:30)",
                "  #2 pay rent"
            ]
        );
    }

    #[test]
    fn a_task_rings_once_when_its_time_has_come() {
        let mut m = kestrel_memory::Memory::in_memory().unwrap();
        let a = m.create_task("call asha", Some("2026-10-25"), 0).unwrap();
        m.set_task_time(a, Some("17:30")).unwrap();
        let b = m.create_task("pay rent", Some("2026-10-24"), 0).unwrap();
        m.set_task_time(b, Some("09:00")).unwrap();
        m.create_task("buy milk", Some("2026-10-25"), 0).unwrap(); // no time: never rings
        let mut sent = Vec::new();
        let mut send = |t: &str, b: &str| -> Result<(), Box<dyn Error>> {
            sent.push(format!("{t} | {b}"));
            Ok(())
        };
        let none: Vec<i64> = vec![];
        // 17:00: only yesterday's has come; 17:30 is this minute; then nothing more.
        assert_eq!(ring(&mut m, today(), 17 * 60, 1, &mut send).unwrap(), [b]);
        assert_eq!(
            ring(&mut m, today(), 17 * 60 + 30, 2, &mut send).unwrap(),
            [a]
        );
        assert_eq!(ring(&mut m, today(), 18 * 60, 3, &mut send).unwrap(), none);
        assert_eq!(m.tasks().unwrap()[0].notified_at, Some(2));
        // A notification that fails rings again on the next check.
        m.set_task_time(a, Some("18:00")).unwrap();
        let mut fail =
            |_: &str, _: &str| -> Result<(), Box<dyn Error>> { Err("no display".into()) };
        assert!(ring(&mut m, today(), 18 * 60, 4, &mut fail).is_err());
        assert_eq!(ring(&mut m, today(), 18 * 60, 5, &mut send).unwrap(), [a]);
        assert_eq!(
            sent,
            [
                "Kestrel reminder | 2026-10-24 09:00 pay rent",
                "Kestrel reminder | 17:30 call asha",
                "Kestrel reminder | 18:00 call asha"
            ]
        );
    }

    #[test]
    fn several_reminders_share_one_notification() {
        let tasks: Vec<TaskRow> = (1..=8)
            .map(|i| timed(i, &format!("task {i}"), "2026-10-25", "09:00"))
            .collect();
        let (title, body) = alarm(&tasks.iter().collect::<Vec<_>>(), today()).unwrap();
        assert_eq!(title, "Kestrel: 8 reminders");
        assert_eq!(body.lines().next(), Some("09:00 task 1"));
        assert_eq!(body.lines().count(), NOTIFY_MAX + 1);
        assert!(body.ends_with("and 2 more"), "{body}");
        assert_eq!(alarm(&[], today()), None);
    }

    fn today() -> Date {
        Date::parse("2026-10-25").unwrap()
    }

    #[test]
    fn overdue_and_today_are_picked_in_date_order() {
        let tasks = vec![
            task(1, "call the plumber", None),
            task(2, "pay rent", Some("2026-10-25")),
            task(3, "renew passport", Some("2026-11-01")),
            task(4, "file taxes", Some("2026-10-20")),
            task(5, "maa ko call karna", Some("2026-10-24")),
            task(6, "buy milk", Some("2026-10-25")),
        ];
        let d = due(&tasks, today());
        let ids = |v: &[&TaskRow]| v.iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(&d.overdue), [4, 5]);
        assert_eq!(ids(&d.today), [2, 6]);
    }

    #[test]
    fn the_printout_says_what_is_due_and_since_when() {
        let tasks = vec![
            task(4, "file taxes", Some("2026-10-20")),
            task(2, "pay rent", Some("2026-10-25")),
        ];
        assert_eq!(
            lines(&due(&tasks, today())),
            [
                "overdue:",
                "  #4 file taxes (due 2026-10-20)",
                "due today:",
                "  #2 pay rent"
            ]
        );
        assert_eq!(
            lines(&due(&[task(3, "later", Some("2026-11-01"))], today())),
            ["nothing due today"]
        );
    }

    #[test]
    fn a_notification_only_when_something_is_due_and_never_too_long() {
        assert_eq!(
            notification(&due(&[task(1, "someday", None)], today())),
            None
        );
        let (title, body) = notification(&due(
            &[
                task(1, "file taxes", Some("2026-10-20")),
                task(2, "pay rent", Some("2026-10-25")),
            ],
            today(),
        ))
        .unwrap();
        assert_eq!(title, "Kestrel: 1 due today, 1 overdue");
        assert_eq!(body, "overdue: file taxes\npay rent");
        let many: Vec<TaskRow> = (1..=9)
            .map(|i| task(i, &format!("task {i}"), Some("2026-10-25")))
            .collect();
        let (title, body) = notification(&due(&many, today())).unwrap();
        assert_eq!(title, "Kestrel: 9 due today");
        assert_eq!(body.lines().count(), NOTIFY_MAX + 1);
        assert!(body.ends_with("and 3 more"), "{body}");
    }
}
