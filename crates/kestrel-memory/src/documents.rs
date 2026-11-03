//! Documents (v4 B, blueprint §4.5–4.6): which files in the notes folder are ID cards, policies, bills
//! and so on, and the expiry date the user confirmed. Only this is kept: no document text and no ID
//! number. A table added beside the others, with the schema version left as it was: v3's binary, which
//! still rings the morning reminder and is the way back (ADR-057), refuses a newer version.

use kestrel_core::KestrelError;
use rusqlite::params;

use crate::Memory;
use crate::store::storage;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS documents(
  path TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  found_on TEXT,
  expires_on TEXT,
  answered INTEGER NOT NULL DEFAULT 0,
  reminded_30 INTEGER,
  reminded_7 INTEGER
);";

/// A document as found in the notes folder now.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundDocument {
    pub path: String,
    pub kind: String,
    pub sha256: String,
    /// The expiry date the rules read in it, not yet confirmed.
    pub found_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentRow {
    pub path: String,
    pub kind: String,
    pub found_on: Option<String>,
    /// The date the user confirmed; only this one rings.
    pub expires_on: Option<String>,
    /// The user has said yes or no to `found_on`.
    pub answered: bool,
    pub reminded_30: bool,
    pub reminded_7: bool,
}

impl Memory {
    /// Makes the table match what is in the folder now: new files added, gone ones removed, and a
    /// changed file asked about again.
    pub fn sync_documents(&mut self, found: &[FoundDocument]) -> Result<(), KestrelError> {
        let tx = self.conn.transaction().map_err(storage)?;
        {
            let mut known = tx.prepare("SELECT path FROM documents").map_err(storage)?;
            let paths: Vec<String> = known
                .query_map([], |r| r.get(0))
                .and_then(Iterator::collect)
                .map_err(storage)?;
            for gone in paths
                .iter()
                .filter(|p| !found.iter().any(|f| &f.path == *p))
            {
                tx.execute("DELETE FROM documents WHERE path = ?1", [gone])
                    .map_err(storage)?;
            }
            for f in found {
                tx.execute(
                    "INSERT INTO documents(path, kind, sha256, found_on) VALUES(?1, ?2, ?3, ?4)
                     ON CONFLICT(path) DO UPDATE SET
                       expires_on = iif(sha256 = excluded.sha256, expires_on, NULL),
                       answered = iif(sha256 = excluded.sha256, answered, 0),
                       reminded_30 = iif(sha256 = excluded.sha256, reminded_30, NULL),
                       reminded_7 = iif(sha256 = excluded.sha256, reminded_7, NULL),
                       kind = excluded.kind,
                       sha256 = excluded.sha256,
                       found_on = excluded.found_on",
                    params![f.path, f.kind, f.sha256, f.found_on],
                )
                .map_err(storage)?;
            }
        }
        tx.commit().map_err(storage)
    }

    /// Every document, by path.
    pub fn documents(&self) -> Result<Vec<DocumentRow>, KestrelError> {
        let mut q = self
            .conn
            .prepare(
                "SELECT path, kind, found_on, expires_on, answered,
                        reminded_30 IS NOT NULL, reminded_7 IS NOT NULL
                 FROM documents ORDER BY path",
            )
            .map_err(storage)?;
        q.query_map([], |r| {
            Ok(DocumentRow {
                path: r.get(0)?,
                kind: r.get(1)?,
                found_on: r.get(2)?,
                expires_on: r.get(3)?,
                answered: r.get(4)?,
                reminded_30: r.get(5)?,
                reminded_7: r.get(6)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(storage)
    }

    /// The user's answer about `path`'s date: Some confirms it, None declines; not asked again.
    pub fn answer_document(&mut self, path: &str, date: Option<&str>) -> Result<(), KestrelError> {
        self.conn
            .execute(
                "UPDATE documents SET expires_on = ?2, answered = 1,
                 reminded_30 = NULL, reminded_7 = NULL WHERE path = ?1",
                params![path, date],
            )
            .map(|_| ())
            .map_err(storage)
    }

    /// `path`'s reminder `days` (30 or 7) before expiry has rung, at `now`.
    pub fn mark_document_reminded(
        &mut self,
        path: &str,
        days: u32,
        now: i64,
    ) -> Result<(), KestrelError> {
        let column = if days == 30 {
            "reminded_30"
        } else {
            "reminded_7"
        };
        self.conn
            .execute(
                &format!("UPDATE documents SET {column} = ?2 WHERE path = ?1"),
                params![path, now],
            )
            .map(|_| ())
            .map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(path: &str, sha: &str, date: Option<&str>) -> FoundDocument {
        FoundDocument {
            path: path.into(),
            kind: "passport".into(),
            sha256: sha.into(),
            found_on: date.map(String::from),
        }
    }

    #[test]
    fn the_table_follows_the_folder_and_an_answer_stays_until_the_file_changes() {
        let mut m = Memory::in_memory().unwrap();
        let date = Some("2029-03-11");
        m.sync_documents(&[found("p.pdf", "a", date), found("q.pdf", "b", None)])
            .unwrap();
        let rows = m.documents().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (
                rows[0].found_on.as_deref(),
                rows[0].expires_on.clone(),
                rows[0].answered
            ),
            (date, None, false),
            "a date found is not a date confirmed"
        );

        m.answer_document("p.pdf", date).unwrap();
        m.mark_document_reminded("p.pdf", 30, 5).unwrap();
        m.sync_documents(&[found("p.pdf", "a", date)]).unwrap();
        let rows = m.documents().unwrap();
        assert_eq!(rows.len(), 1, "q.pdf is gone from the folder");
        assert_eq!(rows[0].expires_on.as_deref(), date);
        assert!(rows[0].answered && rows[0].reminded_30 && !rows[0].reminded_7);

        // A new version of the file is a new question.
        m.sync_documents(&[found("p.pdf", "c", Some("2039-01-01"))])
            .unwrap();
        let r = &m.documents().unwrap()[0];
        assert_eq!(
            (
                r.found_on.as_deref(),
                r.expires_on.clone(),
                r.answered,
                r.reminded_30
            ),
            (Some("2039-01-01"), None, false, false)
        );

        m.answer_document("p.pdf", None).unwrap();
        let r = &m.documents().unwrap()[0];
        assert!(r.answered && r.expires_on.is_none());
    }

    #[test]
    fn the_schema_version_stays_so_v3_s_binary_still_opens_the_file() {
        let m = Memory::in_memory().unwrap();
        let v: i64 = m
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
    }
}
