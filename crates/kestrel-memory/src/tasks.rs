//! Tasks in the memory file (ADR-023/026): the SQLite side of the `create_task` / `delete_task`
//! tools. Titles are validated by kestrel-tools before they get here. Ids are AUTOINCREMENT, so
//! "delete task 3" can never hit a newer task that reused a deleted id.
//!
//! Stage 2 (ADR-034) adds a time of day and when its reminder fired. They are added columns, not
//! a new schema version, so main's binary still opens a file this branch has touched.

use kestrel_core::KestrelError;
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::store::{Memory, storage};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskRow {
    pub id: i64,
    pub title: String,
    pub due: Option<String>,
    pub created_at: i64,
    /// "HH:MM", local time, on the `due` date.
    pub due_time: Option<String>,
    /// When the reminder for this date and time was shown (Unix seconds); a new date or time
    /// clears it.
    pub notified_at: Option<i64>,
}

/// Adds the Stage 2 columns to a tasks table made before them (an idempotent step of opening).
pub(crate) fn add_time_columns(conn: &Connection) -> rusqlite::Result<()> {
    let mut st = conn.prepare("SELECT name FROM pragma_table_info('tasks')")?;
    let have: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for (name, ty) in [("due_time", "TEXT"), ("notified_at", "INTEGER")] {
        if !have.iter().any(|h| h == name) {
            conn.execute_batch(&format!("ALTER TABLE tasks ADD COLUMN {name} {ty}"))?;
        }
    }
    Ok(())
}

/// Exactly "HH:MM" on a 24-hour clock.
fn check_time(t: &str) -> Result<(), KestrelError> {
    let b = t.as_bytes();
    let two = |i: usize| -> Option<u32> {
        let (h, l) = (b[i], b[i + 1]);
        (h.is_ascii_digit() && l.is_ascii_digit())
            .then(|| u32::from(h - b'0') * 10 + u32::from(l - b'0'))
    };
    let ok = b.len() == 5
        && b[2] == b':'
        && two(0).is_some_and(|h| h < 24)
        && two(3).is_some_and(|m| m < 60);
    if ok {
        Ok(())
    } else {
        Err(KestrelError::Invalid(format!(
            "task time {t:?} (want HH:MM)"
        )))
    }
}

impl Memory {
    pub fn create_task(
        &mut self,
        title: &str,
        due: Option<&str>,
        now: i64,
    ) -> Result<i64, KestrelError> {
        self.conn
            .execute(
                "INSERT INTO tasks(title, due, created_at) VALUES(?1, ?2, ?3)",
                params![title, due, now],
            )
            .map_err(storage)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// A new title and/or due date (`Some(None)` clears the date and its time; `None` leaves a
    /// field as it is). A new date can ring again. False if there was no such task.
    pub fn update_task(
        &mut self,
        id: i64,
        title: Option<&str>,
        due: Option<Option<&str>>,
    ) -> Result<bool, KestrelError> {
        let exists: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tasks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .map_err(storage)?;
        if exists == 0 {
            return Ok(false);
        }
        if let Some(t) = title {
            self.conn
                .execute("UPDATE tasks SET title = ?1 WHERE id = ?2", params![t, id])
                .map_err(storage)?;
        }
        if let Some(d) = due {
            self.conn
                .execute(
                    "UPDATE tasks SET due = ?1, notified_at = NULL,
                     due_time = CASE WHEN ?1 IS NULL THEN NULL ELSE due_time END WHERE id = ?2",
                    params![d, id],
                )
                .map_err(storage)?;
        }
        Ok(true)
    }

    /// Sets ("HH:MM") or clears the time of day; a new time can ring again. False if there was
    /// no such task.
    pub fn set_task_time(&mut self, id: i64, time: Option<&str>) -> Result<bool, KestrelError> {
        if let Some(t) = time {
            check_time(t)?;
        }
        let n = self
            .conn
            .execute(
                "UPDATE tasks SET due_time = ?1, notified_at = NULL WHERE id = ?2",
                params![time, id],
            )
            .map_err(storage)?;
        Ok(n > 0)
    }

    /// Records that this task's reminder was shown, so it is shown once. False if there was no
    /// such task.
    pub fn mark_notified(&mut self, id: i64, now: i64) -> Result<bool, KestrelError> {
        let n = self
            .conn
            .execute(
                "UPDATE tasks SET notified_at = ?1 WHERE id = ?2",
                params![now, id],
            )
            .map_err(storage)?;
        Ok(n > 0)
    }

    /// False if there was no such task.
    pub fn delete_task(&mut self, id: i64) -> Result<bool, KestrelError> {
        let n = self
            .conn
            .execute("DELETE FROM tasks WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(n > 0)
    }

    pub fn tasks(&self) -> Result<Vec<TaskRow>, KestrelError> {
        let mut st = self
            .conn
            .prepare(
                "SELECT id, title, due, created_at, due_time, notified_at FROM tasks ORDER BY id",
            )
            .map_err(storage)?;
        let rows = st.query_map([], |r| {
            Ok(TaskRow {
                id: r.get(0)?,
                title: r.get(1)?,
                due: r.get(2)?,
                created_at: r.get(3)?,
                due_time: r.get(4)?,
                notified_at: r.get(5)?,
            })
        });
        rows.and_then(|rows| rows.collect()).map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::temp_db;

    #[test]
    fn tasks_are_stored_listed_and_deleted_and_ids_never_reused() {
        let mut m = Memory::in_memory().unwrap();
        let a = m.create_task("Call Asha", Some("2026-10-24"), 0).unwrap();
        let b = m.create_task("asha ko call karna", None, 1).unwrap();
        assert!(m.delete_task(b).unwrap());
        assert!(!m.delete_task(b).unwrap());
        let c = m.create_task("Buy milk", None, 2).unwrap();
        assert!(c > b, "a deleted task's id is not reused");
        let rows: Vec<_> = m
            .tasks()
            .unwrap()
            .into_iter()
            .map(|t| (t.id, t.title, t.due))
            .collect();
        assert_eq!(
            rows,
            [
                (a, "Call Asha".to_string(), Some("2026-10-24".to_string())),
                (c, "Buy milk".to_string(), None)
            ]
        );
    }

    #[test]
    fn a_task_can_be_renamed_rescheduled_or_undated() {
        let mut m = Memory::in_memory().unwrap();
        let a = m.create_task("call asha", Some("2026-10-24"), 0).unwrap();
        assert!(m.update_task(a, Some("call asha at 6"), None).unwrap());
        assert!(m.update_task(a, None, Some(Some("2026-10-30"))).unwrap());
        let t = &m.tasks().unwrap()[0];
        assert_eq!(
            (t.title.as_str(), t.due.as_deref()),
            ("call asha at 6", Some("2026-10-30"))
        );
        assert!(m.update_task(a, None, Some(None)).unwrap());
        assert_eq!(m.tasks().unwrap()[0].due, None);
        assert!(!m.update_task(99, Some("x"), None).unwrap(), "no such task");
        assert!(
            m.update_task(a, None, None).unwrap(),
            "nothing to change still finds the task"
        );
    }

    #[test]
    fn a_task_can_have_a_time_and_be_marked_notified_once() {
        let mut m = Memory::in_memory().unwrap();
        let a = m.create_task("call asha", Some("2026-10-25"), 0).unwrap();
        assert!(m.set_task_time(a, Some("17:30")).unwrap());
        m.mark_notified(a, 1_000).unwrap();
        let t = &m.tasks().unwrap()[0];
        assert_eq!(
            (t.due_time.as_deref(), t.notified_at),
            (Some("17:30"), Some(1_000))
        );
        // A new date or time rings again.
        m.update_task(a, None, Some(Some("2026-10-26"))).unwrap();
        assert_eq!(m.tasks().unwrap()[0].notified_at, None);
        m.mark_notified(a, 2_000).unwrap();
        m.set_task_time(a, Some("09:00")).unwrap();
        assert_eq!(m.tasks().unwrap()[0].notified_at, None);
        for bad in ["25:00", "9:00", "17:60", "noon", ""] {
            assert!(m.set_task_time(a, Some(bad)).is_err(), "{bad}");
        }
        assert!(m.set_task_time(a, None).unwrap());
        assert!(!m.set_task_time(99, Some("10:00")).unwrap());
        // An undated task has no time: clearing the date clears it too.
        m.set_task_time(a, Some("09:00")).unwrap();
        m.update_task(a, None, Some(None)).unwrap();
        assert_eq!(m.tasks().unwrap()[0].due_time, None);
    }

    #[test]
    fn an_older_memory_file_gets_the_time_columns_and_keeps_its_tasks() {
        let p = temp_db("v1_tasks");
        {
            let c = rusqlite::Connection::open(&p).unwrap();
            c.execute_batch(
                "CREATE TABLE tasks(id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, due TEXT,
                 created_at INTEGER NOT NULL); INSERT INTO tasks(title, due, created_at) VALUES('old', NULL, 0);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }
        let mut m = Memory::open(&p).unwrap();
        let t = &m.tasks().unwrap()[0];
        assert_eq!((t.title.as_str(), t.due_time.as_deref()), ("old", None));
        assert!(m.set_task_time(t.id, Some("08:15")).unwrap());
        drop(m);
        assert_eq!(
            Memory::open(&p).unwrap().tasks().unwrap()[0]
                .due_time
                .as_deref(),
            Some("08:15")
        );
    }

    #[test]
    fn tasks_survive_reopening_the_file() {
        let path = temp_db("tasks");
        let id = Memory::open(&path)
            .unwrap()
            .create_task("Call Asha", None, 0)
            .unwrap();
        let m = Memory::open(&path).unwrap();
        assert_eq!(m.tasks().unwrap()[0].id, id);
    }
}
