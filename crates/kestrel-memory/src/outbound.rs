//! What leaves this device (v4 C, blueprint §4.5, §4.8): the outbound log, written before anything is
//! sent and shown on the page, and the contacts mail may go to. Tables added beside the others, with the
//! schema version left as it was (src/documents.rs says why).

use kestrel_core::KestrelError;
use rusqlite::{OptionalExtension, params};

use crate::Memory;
use crate::store::storage;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS outbound(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  at INTEGER NOT NULL,
  kind TEXT NOT NULL,
  host TEXT NOT NULL,
  what TEXT NOT NULL,
  approved INTEGER NOT NULL,
  result TEXT
);
CREATE TABLE IF NOT EXISTS contacts(
  name TEXT PRIMARY KEY,
  email TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS briefs(
  day TEXT PRIMARY KEY,
  at INTEGER NOT NULL
);";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct OutboundRow {
    pub id: i64,
    pub at: i64,
    pub kind: String,
    pub host: String,
    pub what: String,
    pub approved: bool,
    /// None while it is on its way; a request that never finished stays visible as such.
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Contact {
    pub name: String,
    pub email: String,
}

impl Memory {
    /// Writes a request to the log, before it is sent; its id.
    pub fn log_outbound(
        &mut self,
        now: i64,
        kind: &str,
        host: &str,
        what: &str,
        approved: bool,
    ) -> Result<i64, KestrelError> {
        self.conn
            .execute(
                "INSERT INTO outbound(at, kind, host, what, approved) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![now, kind, host, what, approved],
            )
            .map_err(storage)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// How a logged request ended.
    pub fn finish_outbound(&mut self, id: i64, result: &str) -> Result<(), KestrelError> {
        self.conn
            .execute(
                "UPDATE outbound SET result = ?2 WHERE id = ?1",
                params![id, result],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// The latest `limit` requests, newest first.
    pub fn outbound(&self, limit: u32) -> Result<Vec<OutboundRow>, KestrelError> {
        let mut q = self
            .conn
            .prepare(
                "SELECT id, at, kind, host, what, approved, result FROM outbound
                 ORDER BY id DESC LIMIT ?1",
            )
            .map_err(storage)?;
        q.query_map([limit], |r| {
            Ok(OutboundRow {
                id: r.get(0)?,
                at: r.get(1)?,
                kind: r.get(2)?,
                host: r.get(3)?,
                what: r.get(4)?,
                approved: r.get(5)?,
                result: r.get(6)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(storage)
    }

    /// Has the morning brief been given on `day`?
    pub fn brief_given(&self, day: &str) -> Result<bool, KestrelError> {
        self.conn
            .query_row("SELECT 1 FROM briefs WHERE day = ?1", [day], |_| Ok(()))
            .optional()
            .map(|found| found.is_some())
            .map_err(storage)
    }

    /// The morning brief is given on `day`.
    pub fn mark_brief(&mut self, day: &str, now: i64) -> Result<(), KestrelError> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO briefs(day, at) VALUES(?1, ?2)",
                params![day, now],
            )
            .map(|_| ())
            .map_err(storage)
    }

    /// A contact, by name in any case; a new address replaces the old.
    pub fn add_contact(&mut self, name: &str, email: &str) -> Result<(), KestrelError> {
        self.remove_contact(name)?;
        self.conn
            .execute(
                "INSERT INTO contacts(name, email) VALUES(?1, ?2)",
                params![name, email],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn remove_contact(&mut self, name: &str) -> Result<bool, KestrelError> {
        let gone = self
            .conn
            .execute("DELETE FROM contacts WHERE lower(name) = lower(?1)", [name])
            .map_err(storage)?;
        Ok(gone > 0)
    }

    pub fn contact(&self, name: &str) -> Result<Option<Contact>, KestrelError> {
        self.conn
            .query_row(
                "SELECT name, email FROM contacts WHERE lower(name) = lower(?1)",
                [name],
                |r| {
                    Ok(Contact {
                        name: r.get(0)?,
                        email: r.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(storage)
    }

    /// Every contact, by name.
    pub fn contacts(&self) -> Result<Vec<Contact>, KestrelError> {
        let mut q = self
            .conn
            .prepare("SELECT name, email FROM contacts ORDER BY lower(name)")
            .map_err(storage)?;
        q.query_map([], |r| {
            Ok(Contact {
                name: r.get(0)?,
                email: r.get(1)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_each_request_and_how_it_ended_newest_first() {
        let mut m = Memory::in_memory().unwrap();
        let a = m
            .log_outbound(10, "mail", "smtp.gmail.com", "to r@x.in: Leave", true)
            .unwrap();
        let b = m
            .log_outbound(20, "fetch", "example.com", "https://example.com/", true)
            .unwrap();
        assert_eq!(m.outbound(10).unwrap()[0].result, None, "on its way");
        m.finish_outbound(a, "sent").unwrap();
        let rows = m.outbound(10).unwrap();
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [b, a]);
        assert_eq!(rows[1].result.as_deref(), Some("sent"));
        assert_eq!(
            (rows[1].kind.as_str(), rows[1].host.as_str()),
            ("mail", "smtp.gmail.com")
        );
        assert!(rows[1].approved);
        assert_eq!(m.outbound(1).unwrap().len(), 1);
    }

    #[test]
    fn a_contact_is_found_in_any_case_and_a_new_address_replaces_the_old() {
        let mut m = Memory::in_memory().unwrap();
        m.add_contact("Ravi", "ravi@x.in").unwrap();
        assert_eq!(m.contact("RAVI").unwrap().unwrap().email, "ravi@x.in");
        assert_eq!(m.contact("ravi").unwrap().unwrap().name, "Ravi");
        m.add_contact("ravi", "new@x.in").unwrap();
        assert_eq!(m.contacts().unwrap().len(), 1);
        assert_eq!(m.contact("ravi").unwrap().unwrap().email, "new@x.in");
        assert_eq!(m.contact("asha").unwrap(), None);
        assert!(m.remove_contact("Ravi").unwrap());
        assert!(!m.remove_contact("Ravi").unwrap());
    }

    #[test]
    fn a_brief_is_marked_by_its_day() {
        let mut m = Memory::in_memory().unwrap();
        assert!(!m.brief_given("2026-11-04").unwrap());
        m.mark_brief("2026-11-04", 5).unwrap();
        assert!(m.brief_given("2026-11-04").unwrap());
        assert!(!m.brief_given("2026-11-05").unwrap());
    }

    #[test]
    fn the_export_holds_the_log_and_the_contacts() {
        let mut m = Memory::in_memory().unwrap();
        m.add_contact("Ravi", "ravi@x.in").unwrap();
        m.log_outbound(1, "mail", "smtp.gmail.com", "to ravi@x.in", true)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&m.export_all_json().unwrap()).unwrap();
        assert_eq!(v["contacts"][0]["email"], "ravi@x.in");
        assert_eq!(v["outbound"][0]["host"], "smtp.gmail.com");
    }
}
