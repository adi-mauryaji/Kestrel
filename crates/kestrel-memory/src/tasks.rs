//! Tasks in the memory file (ADR-023/026): the SQLite side of the `create_task` / `delete_task`
//! tools. Titles are validated by kestrel-tools before they get here. Ids are AUTOINCREMENT, so
//! "delete task 3" can never hit a newer task that reused a deleted id.

use kestrel_core::KestrelError;
use rusqlite::params;
use serde::Serialize;

use crate::store::{Memory, storage};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskRow {
    pub id: i64,
    pub title: String,
    pub due: Option<String>,
    pub created_at: i64,
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

    /// A new title and/or due date (`Some(None)` clears the date; `None` leaves a field as it is).
    /// False if there was no such task.
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
                .execute("UPDATE tasks SET due = ?1 WHERE id = ?2", params![d, id])
                .map_err(storage)?;
        }
        Ok(true)
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
            .prepare("SELECT id, title, due, created_at FROM tasks ORDER BY id")
            .map_err(storage)?;
        let rows = st.query_map([], |r| {
            Ok(TaskRow {
                id: r.get(0)?,
                title: r.get(1)?,
                due: r.get(2)?,
                created_at: r.get(3)?,
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
