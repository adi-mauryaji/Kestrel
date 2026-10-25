//! `kestrel remind [--notify] [--db PATH]` (ADR-034): the tasks due today and the overdue ones,
//! from the memory file, by the local date. With `--notify` the same goes to a desktop
//! notification (notify-send; nothing when nothing is due), which is how the systemd user timer
//! of scripts/install_reminders.sh runs it every morning and at login. No model is loaded.

use std::error::Error;
use std::io::Write;

use kestrel_memory::TaskRow;
use kestrel_tools::date::Date;

/// At most this many tasks are listed in a notification; the rest are counted.
pub const NOTIFY_MAX: usize = 6;

/// The tasks with a due date on or before `today`: overdue first (oldest first), then today's.
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
    dated.sort_by_key(|(d, t)| (*d, t.id));
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
            out.push(format!(
                "  #{} {} (due {})",
                t.id,
                t.title,
                t.due.as_deref().unwrap_or("?")
            ));
        }
    }
    if !due.today.is_empty() {
        out.push("due today:".to_string());
        out.extend(due.today.iter().map(|t| format!("  #{} {}", t.id, t.title)));
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
    let mut body: Vec<String> = all.iter().take(NOTIFY_MAX).cloned().collect();
    if all.len() > NOTIFY_MAX {
        body.push(format!("and {} more", all.len() - NOTIFY_MAX));
    }
    Some((title, body.join("\n")))
}

pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let notify = args.iter().any(|a| a == "--notify");
    let db = match args.iter().position(|a| a == "--db") {
        Some(i) => args
            .get(i + 1)
            .map(std::path::PathBuf::from)
            .ok_or("--db needs a path")?,
        None => crate::memory::default_db(&|k| std::env::var_os(k))?,
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
        let status = std::process::Command::new("notify-send")
            .args([
                "--app-name=Kestrel",
                "--icon=appointment-soon",
                &title,
                &body,
            ])
            .status()?;
        if !status.success() {
            return Err(format!("notify-send failed: {status}").into());
        }
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
