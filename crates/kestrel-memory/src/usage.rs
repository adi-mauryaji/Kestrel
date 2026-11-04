//! How much Kestrel is used, day by day, for the pilot (v4, blueprint §4.12): counts and nothing else.
//! `what` is a `&'static str`, a name written in the program, so nothing a person says can end up
//! here. Nothing here leaves by itself: Settings shows it, and the person copies or forgets it. A table
//! added beside the others, with the schema version left as it was (src/documents.rs says why).

use kestrel_core::KestrelError;
use rusqlite::params;

use crate::Memory;
use crate::store::storage;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS usage(
  day TEXT NOT NULL,
  what TEXT NOT NULL,
  n INTEGER NOT NULL,
  PRIMARY KEY(day, what)
);";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UsageRow {
    pub day: String,
    pub what: String,
    pub n: i64,
}

impl Memory {
    /// One more of `what` on the local day of `now`. A count that cannot be written is lost: it must
    /// never fail what it counts.
    pub fn count_use(&mut self, now: i64, what: &'static str) {
        let _ = self.conn.execute(
            "INSERT INTO usage(day, what, n) VALUES(date(?1, 'unixepoch', 'localtime'), ?2, 1)
             ON CONFLICT(day, what) DO UPDATE SET n = n + 1",
            params![now, what],
        );
    }

    /// `n` more of `what` today.
    pub(crate) fn count_today(&mut self, what: &'static str, n: i64) {
        let _ = self.conn.execute(
            "INSERT INTO usage(day, what, n) VALUES(date('now', 'localtime'), ?1, ?2)
             ON CONFLICT(day, what) DO UPDATE SET n = n + excluded.n",
            params![what, n],
        );
    }

    /// Every count, oldest day first.
    pub fn usage(&self) -> Result<Vec<UsageRow>, KestrelError> {
        let mut q = self
            .conn
            .prepare("SELECT day, what, n FROM usage ORDER BY day, what")
            .map_err(storage)?;
        q.query_map([], |r| {
            Ok(UsageRow {
                day: r.get(0)?,
                what: r.get(1)?,
                n: r.get(2)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(storage)
    }

    /// Removes every count.
    pub fn forget_usage(&mut self) -> Result<(), KestrelError> {
        self.conn
            .execute("DELETE FROM usage", [])
            .map(|_| ())
            .map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FoundDocument;

    const DAY: i64 = 86_400;
    const NOW: i64 = 20_000 * DAY;

    fn counted(m: &Memory, what: &str) -> i64 {
        m.usage()
            .unwrap()
            .iter()
            .filter(|r| r.what == what)
            .map(|r| r.n)
            .sum()
    }

    #[test]
    fn counts_add_up_by_day_and_name() {
        let mut m = Memory::in_memory().unwrap();
        assert_eq!(m.usage().unwrap(), []);
        m.count_use(NOW, "chat");
        m.count_use(NOW, "chat");
        m.count_use(NOW, "brief");
        m.count_use(NOW + 2 * DAY, "chat");
        let rows = m.usage().unwrap();
        let said: Vec<(&str, i64)> = rows.iter().map(|r| (r.what.as_str(), r.n)).collect();
        assert_eq!(said, [("brief", 1), ("chat", 2), ("chat", 1)]);
        assert_eq!(rows[0].day, rows[1].day);
        assert!(rows[1].day < rows[2].day, "{rows:?}");
        assert_eq!(rows[0].day.len(), "2024-10-04".len(), "{rows:?}");
    }

    #[test]
    fn a_new_task_is_counted_whoever_made_it() {
        let mut m = Memory::in_memory().unwrap();
        m.create_task("CANARY-7f3a call asha", None, NOW).unwrap();
        m.create_task("book the room", Some("2029-03-11"), NOW)
            .unwrap();
        assert_eq!(counted(&m, "task_added"), 2);
        assert!(!format!("{:?}", m.usage().unwrap()).contains("CANARY"));
    }

    #[test]
    fn a_document_is_counted_when_it_is_first_found_and_not_on_each_rescan() {
        let found = |path: &str, sha: &str| FoundDocument {
            path: path.into(),
            kind: "passport".into(),
            sha256: sha.into(),
            found_on: None,
        };
        let mut m = Memory::in_memory().unwrap();
        m.sync_documents(&[found("p.pdf", "a"), found("q.pdf", "b")])
            .unwrap();
        assert_eq!(counted(&m, "document_added"), 2);
        m.sync_documents(&[found("p.pdf", "a"), found("q.pdf", "changed")])
            .unwrap();
        assert_eq!(counted(&m, "document_added"), 2);
        m.sync_documents(&[
            found("p.pdf", "a"),
            found("q.pdf", "b"),
            found("r.pdf", "c"),
        ])
        .unwrap();
        assert_eq!(counted(&m, "document_added"), 3);
        assert!(!format!("{:?}", m.usage().unwrap()).contains(".pdf"));
    }

    #[test]
    fn counts_can_be_forgotten_and_a_backup_holds_them() {
        let mut m = Memory::in_memory().unwrap();
        m.count_use(NOW, "chat");
        let v: serde_json::Value = serde_json::from_str(&m.export_all_json().unwrap()).unwrap();
        assert_eq!(v["usage"][0]["what"], "chat");
        assert_eq!(v["usage"][0]["n"], 1);
        m.forget_usage().unwrap();
        assert_eq!(m.usage().unwrap(), []);
    }
}
