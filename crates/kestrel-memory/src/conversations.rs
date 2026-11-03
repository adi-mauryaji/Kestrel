//! Conversations (v4 B, blueprint §4.5): the turns of the chat so far, on this device only. Tables
//! added beside the others, with the schema version left as it was (src/documents.rs says why).

use kestrel_core::KestrelError;
use rusqlite::{OptionalExtension, params};

use crate::Memory;
use crate::store::storage;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS conversations(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  started_at INTEGER NOT NULL,
  last_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS turns(
  conversation INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  n INTEGER NOT NULL,
  role TEXT NOT NULL CHECK(role IN ('person', 'kestrel')),
  text TEXT NOT NULL,
  at INTEGER NOT NULL,
  PRIMARY KEY(conversation, n)
);";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Person,
    Kestrel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

impl Memory {
    /// The conversation in progress: the latest one if its last turn is at most `gap` seconds old,
    /// else a new one.
    pub fn current_conversation(&mut self, now: i64, gap: i64) -> Result<i64, KestrelError> {
        let latest: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, last_at FROM conversations ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        match latest {
            Some((id, last)) if now - last <= gap => Ok(id),
            _ => self.new_conversation(now),
        }
    }

    /// A new conversation from now on.
    pub fn new_conversation(&mut self, now: i64) -> Result<i64, KestrelError> {
        self.conn
            .execute(
                "INSERT INTO conversations(started_at, last_at) VALUES(?1, ?1)",
                [now],
            )
            .map_err(storage)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn add_turn(
        &mut self,
        conversation: i64,
        role: Role,
        text: &str,
        now: i64,
    ) -> Result<(), KestrelError> {
        let role = match role {
            Role::Person => "person",
            Role::Kestrel => "kestrel",
        };
        let tx = self.conn.transaction().map_err(storage)?;
        tx.execute(
            "INSERT INTO turns(conversation, n, role, text, at)
             VALUES(?1, (SELECT COALESCE(MAX(n), 0) + 1 FROM turns WHERE conversation = ?1), ?2, ?3, ?4)",
            params![conversation, role, text, now],
        )
        .map_err(storage)?;
        tx.execute(
            "UPDATE conversations SET last_at = ?2 WHERE id = ?1",
            params![conversation, now],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)
    }

    /// The conversation's turns, oldest first.
    pub fn turns(&self, conversation: i64) -> Result<Vec<Turn>, KestrelError> {
        let mut q = self
            .conn
            .prepare("SELECT role, text FROM turns WHERE conversation = ?1 ORDER BY n")
            .map_err(storage)?;
        q.query_map([conversation], |r| {
            let role: String = r.get(0)?;
            Ok(Turn {
                role: if role == "person" {
                    Role::Person
                } else {
                    Role::Kestrel
                },
                text: r.get(1)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(storage)
    }

    /// (conversations, turns) kept.
    pub fn conversation_counts(&self) -> Result<(u64, u64), KestrelError> {
        let count = |sql: &str| self.conn.query_row(sql, [], |r| r.get::<_, i64>(0));
        let of = |n: Result<i64, rusqlite::Error>| n.map(|n| n as u64).map_err(storage);
        Ok((
            of(count("SELECT COUNT(*) FROM conversations"))?,
            of(count("SELECT COUNT(*) FROM turns"))?,
        ))
    }

    /// Everything a backup needs (`kestrel memory export`, blueprint §6): memories, tasks, documents,
    /// conversations.
    pub fn export_all_json(&self) -> Result<String, KestrelError> {
        let documents: Vec<serde_json::Value> = self
            .documents()?
            .iter()
            .map(
                |d| serde_json::json!({"path": d.path, "kind": d.kind, "expires_on": d.expires_on}),
            )
            .collect();
        let mut q = self
            .conn
            .prepare("SELECT id, started_at FROM conversations ORDER BY id")
            .map_err(storage)?;
        let ids: Vec<(i64, i64)> = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .and_then(Iterator::collect)
            .map_err(storage)?;
        let mut conversations = Vec::new();
        for (id, started_at) in ids {
            let turns: Vec<serde_json::Value> = self
                .turns(id)?
                .iter()
                .map(|t| {
                    let role = match t.role {
                        Role::Person => "person",
                        Role::Kestrel => "kestrel",
                    };
                    serde_json::json!({"role": role, "text": t.text})
                })
                .collect();
            conversations.push(serde_json::json!({"started_at": started_at, "turns": turns}));
        }
        let all = serde_json::json!({
            "memories": self.items(crate::Filter::All)?,
            "tasks": self.tasks()?,
            "documents": documents,
            "conversations": conversations,
            "contacts": self.contacts()?,
            "outbound": self.outbound(u32::MAX)?,
        });
        serde_json::to_string_pretty(&all).map_err(|e| KestrelError::Storage(e.to_string()))
    }

    /// Removes every conversation.
    pub fn forget_conversations(&mut self) -> Result<(), KestrelError> {
        self.conn
            .execute_batch("DELETE FROM turns; DELETE FROM conversations;")
            .map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_stay_in_order_within_a_conversation_and_a_long_gap_starts_a_new_one() {
        let mut m = Memory::in_memory().unwrap();
        let a = m.current_conversation(100, 50).unwrap();
        m.add_turn(a, Role::Person, "my sister is Meera", 100)
            .unwrap();
        m.add_turn(a, Role::Kestrel, "Nice.", 101).unwrap();
        assert_eq!(
            m.current_conversation(140, 50).unwrap(),
            a,
            "within the gap"
        );
        let t = m.turns(a).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(
            (t[0].role, t[0].text.as_str()),
            (Role::Person, "my sister is Meera")
        );
        assert_eq!(t[1].role, Role::Kestrel);
        let b = m.current_conversation(1000, 50).unwrap();
        assert_ne!(a, b, "past the gap");
        assert!(m.turns(b).unwrap().is_empty());
        let c = m.new_conversation(1001).unwrap();
        assert_ne!(b, c);
        assert_eq!(m.current_conversation(1002, 50).unwrap(), c);
        assert_eq!(m.conversation_counts().unwrap(), (3, 2));
    }

    #[test]
    fn forgetting_removes_every_turn() {
        let mut m = Memory::in_memory().unwrap();
        let a = m.current_conversation(1, 50).unwrap();
        m.add_turn(a, Role::Person, "x", 1).unwrap();
        m.forget_conversations().unwrap();
        assert_eq!(m.conversation_counts().unwrap(), (0, 0));
    }

    #[test]
    fn the_export_holds_everything_a_backup_needs_and_no_deleted_turn() {
        let mut m = Memory::in_memory().unwrap();
        let c = m.current_conversation(1, 50).unwrap();
        m.add_turn(c, Role::Person, "hello there", 1).unwrap();
        m.create_task("buy milk", None, 1).unwrap();
        m.sync_documents(&[crate::FoundDocument {
            path: "p.pdf".into(),
            kind: "passport".into(),
            sha256: "a".into(),
            found_on: None,
        }])
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&m.export_all_json().unwrap()).unwrap();
        assert_eq!(v["tasks"][0]["title"], "buy milk");
        assert_eq!(v["documents"][0]["path"], "p.pdf");
        assert_eq!(v["conversations"][0]["turns"][0]["text"], "hello there");
        assert!(v["memories"].as_array().unwrap().is_empty());
        m.forget_conversations().unwrap();
        assert!(!m.export_all_json().unwrap().contains("hello there"));
    }
}
