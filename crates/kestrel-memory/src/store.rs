//! The memory store (blueprint Ch. 17, A.11, ADR-026): one SQLite file. Every write goes through
//! `propose` (or `confirm`) with a `Source`; model-inferred items start pending and stay
//! invisible until approved; the audit table logs the operation, never the content.
//!
//! Changes from A.11:
//! - Precedence is decided in one transaction and returned (`Proposed`): a model-inferred value
//!   only ever replaces a pending model-inferred value. A.11 ignored a guess against a user value
//!   silently (but logged it as a proposal) and let a new guess un-approve an approved one.
//!   Triggers enforce the same rule in SQL (Ch. 18.2), so raw SQL cannot break it either.
//! - A value the user confirms at the prompt is the user's (`confirm`, ADR-026): `user_explicit`,
//!   audited as `confirm`.
//! - Ids are AUTOINCREMENT, so a deleted memory's id is never reused and the audit stays
//!   unambiguous. `purge_expired` audits each row; a proposal meeting an expired row expires it.
//! - Obvious secrets are refused (Ch. 17.1); the file is 0600 (Ch. 19.2); the rollback journal
//!   is used, not WAL (Ch. 17.3: a WAL file keeps old pages); export carries approval and expiry.

use std::path::Path;

use kestrel_core::KestrelError;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, Params, ToSql, Transaction, params};
use serde::Serialize;

use crate::context::{self, Context};
use crate::retrieval::{Scored, Weights, rank, terms};
use crate::secrets;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Preference,
    Fact,
    Summary,
    TaskHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    UserExplicit,
    Correction,
    ModelInferred,
}

/// A stored memory, as retrieval, the CLI and export see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub id: i64,
    pub kind: Kind,
    pub key: String,
    pub value: String,
    /// A stated exception ("unless I ask for details", Ch. 20).
    pub exception: Option<String>,
    pub source: Source,
    pub importance: u8,
    pub approved: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
}

/// What a write asks to store.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft<'a> {
    pub kind: Kind,
    pub key: &'a str,
    pub value: &'a str,
    pub exception: Option<&'a str>,
    pub importance: u8,
    pub expires_at: Option<i64>,
}

impl<'a> Draft<'a> {
    pub fn new(kind: Kind, key: &'a str, value: &'a str) -> Self {
        Self {
            kind,
            key,
            value,
            exception: None,
            importance: 3,
            expires_at: None,
        }
    }

    pub fn exception(self, exception: &'a str) -> Self {
        Self {
            exception: Some(exception),
            ..self
        }
    }

    pub fn importance(self, importance: u8) -> Self {
        Self { importance, ..self }
    }

    pub fn expires_at(self, at: i64) -> Self {
        Self {
            expires_at: Some(at),
            ..self
        }
    }
}

/// What a proposal did, with the row's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proposed {
    /// The user's (or a correction's) value is stored and approved.
    Stored(i64),
    /// A model-inferred value is stored but invisible until the user approves it.
    Pending(i64),
    /// A model-inferred value met an approved one and was not stored.
    Rejected(i64),
}

impl Proposed {
    pub fn id(self) -> i64 {
        match self {
            Self::Stored(id) | Self::Pending(id) | Self::Rejected(id) => id,
        }
    }
}

/// One audit row: which memory, what happened, when, caused by whom. No content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub memory_id: i64,
    pub op: String,
    pub at: i64,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Approved,
    Pending,
}

pub struct Memory {
    pub(crate) conn: Connection,
}

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS memories(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL CHECK(kind IN ('preference','fact','summary','task_history')),
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  exception TEXT,
  source TEXT NOT NULL CHECK(source IN ('user_explicit','correction','model_inferred')),
  importance INTEGER NOT NULL DEFAULT 3 CHECK(importance BETWEEN 1 AND 5),
  approved INTEGER NOT NULL DEFAULT 0 CHECK(approved IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  expires_at INTEGER,
  UNIQUE(kind, key)
);
CREATE TABLE IF NOT EXISTS memory_events(  -- audit trail WITHOUT content
  id INTEGER PRIMARY KEY,
  memory_id INTEGER NOT NULL,
  op TEXT NOT NULL
    CHECK(op IN ('store','pending','reject','approve','confirm','delete','expire')),
  at INTEGER NOT NULL,
  source TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  title TEXT NOT NULL,
  due TEXT,
  created_at INTEGER NOT NULL
);
CREATE TRIGGER IF NOT EXISTS guess_starts_pending BEFORE INSERT ON memories
WHEN NEW.source = 'model_inferred' AND NEW.approved = 1
BEGIN SELECT RAISE(ABORT, 'a model-inferred memory starts pending'); END;
CREATE TRIGGER IF NOT EXISTS guess_never_replaces_approved
BEFORE UPDATE OF value, source ON memories
WHEN NEW.source = 'model_inferred' AND OLD.approved = 1
BEGIN SELECT RAISE(ABORT, 'a model-inferred value cannot replace an approved one'); END;
";

const COLUMNS: &str = "id, kind, key, value, exception, source, importance, approved, created_at, updated_at, expires_at";

pub const MAX_KEY: usize = 64;
pub const MAX_VALUE: usize = 500;
pub const MAX_EXCEPTION: usize = 200;

impl Kind {
    pub const ALL: [Kind; 4] = [
        Kind::Preference,
        Kind::Fact,
        Kind::Summary,
        Kind::TaskHistory,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Fact => "fact",
            Self::Summary => "summary",
            Self::TaskHistory => "task_history",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

impl Source {
    pub const ALL: [Source; 3] = [
        Source::UserExplicit,
        Source::Correction,
        Source::ModelInferred,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserExplicit => "user_explicit",
            Self::Correction => "correction",
            Self::ModelInferred => "model_inferred",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

impl ToSql for Kind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Kind {
    fn column_result(v: ValueRef<'_>) -> FromSqlResult<Self> {
        Kind::parse(v.as_str()?).ok_or(FromSqlError::InvalidType)
    }
}

impl ToSql for Source {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Source {
    fn column_result(v: ValueRef<'_>) -> FromSqlResult<Self> {
        Source::parse(v.as_str()?).ok_or(FromSqlError::InvalidType)
    }
}

pub(crate) fn storage(e: rusqlite::Error) -> KestrelError {
    KestrelError::Storage(e.to_string())
}

impl Memory {
    /// Opens or creates the memory file, made private to the user (0600) before SQLite opens it.
    pub fn open(path: &Path) -> Result<Self, KestrelError> {
        make_private(path)?;
        Self::init(Connection::open(path).map_err(storage)?)
    }

    /// A memory that lives as long as the value (tests, evaluation).
    pub fn in_memory() -> Result<Self, KestrelError> {
        Self::init(Connection::open_in_memory().map_err(storage)?)
    }

    fn init(conn: Connection) -> Result<Self, KestrelError> {
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)?;
        if version > SCHEMA_VERSION {
            return Err(KestrelError::Storage(format!(
                "memory file has schema version {version}; this build knows {SCHEMA_VERSION}"
            )));
        }
        // Freed pages are overwritten (best effort, Ch. 17.3), and a rollback journal is used
        // rather than WAL, whose file keeps old pages until a checkpoint.
        conn.pragma_update(None, "secure_delete", true)
            .and_then(|_| conn.pragma_update(None, "journal_mode", "DELETE"))
            .and_then(|_| conn.execute_batch(SCHEMA))
            .and_then(|_| crate::tasks::add_time_columns(&conn))
            .and_then(|_| conn.pragma_update(None, "user_version", SCHEMA_VERSION))
            .map_err(storage)?;
        Ok(Self { conn })
    }

    /// Stores a draft with its provenance. A user-stated or corrected value is stored approved
    /// and replaces any earlier one; a model-inferred value is stored pending, and only ever
    /// replaces a pending value (`Rejected` against an approved one). Every outcome is audited.
    pub fn propose(
        &mut self,
        d: &Draft,
        source: Source,
        now: i64,
    ) -> Result<Proposed, KestrelError> {
        check(d)?;
        write(&mut self.conn, d, source, now, false).map_err(storage)
    }

    /// The user said yes to this value at the prompt: stored approved, audited as `confirm`
    /// (Ch. 20: every durable store has a confirmation event). A correction keeps its source; a
    /// model proposal the user confirmed is the user's (`user_explicit`, ADR-026).
    pub fn confirm(&mut self, d: &Draft, source: Source, now: i64) -> Result<i64, KestrelError> {
        check(d)?;
        let source = match source {
            Source::ModelInferred => Source::UserExplicit,
            s => s,
        };
        write(&mut self.conn, d, source, now, true)
            .map(Proposed::id)
            .map_err(storage)
    }

    /// Approves a pending item (the review UI). Error if `id` is not pending.
    pub fn approve(&mut self, id: i64, now: i64) -> Result<(), KestrelError> {
        let sql =
            "UPDATE memories SET approved = 1, updated_at = ?2 WHERE id = ?1 AND approved = 0";
        let done = logged(&mut self.conn, id, "approve", now, |tx| {
            tx.execute(sql, params![id, now])
        })
        .map_err(storage)?;
        if done {
            Ok(())
        } else {
            Err(KestrelError::Invalid(format!("no pending memory {id}")))
        }
    }

    /// Deletes an item; false if there was none. The audit keeps the id and the operation only.
    pub fn delete(&mut self, id: i64, now: i64) -> Result<bool, KestrelError> {
        let sql = "DELETE FROM memories WHERE id = ?1";
        logged(&mut self.conn, id, "delete", now, |tx| {
            tx.execute(sql, [id])
        })
        .map_err(storage)
    }

    /// Deletes every item expired at `now`, auditing each.
    pub fn purge_expired(&mut self, now: i64) -> Result<usize, KestrelError> {
        purge(&mut self.conn, now).map_err(storage)
    }

    /// Stored items by id, expired or not (for the CLI and export).
    pub fn items(&self, filter: Filter) -> Result<Vec<Item>, KestrelError> {
        let filter = match filter {
            Filter::All => "",
            Filter::Approved => " WHERE approved = 1",
            Filter::Pending => " WHERE approved = 0",
        };
        let sql = format!("SELECT {COLUMNS} FROM memories{filter} ORDER BY id");
        select(&self.conn, &sql, []).map_err(storage)
    }

    /// Approved items not expired at `now`: what retrieval and context may use.
    fn live(&self, now: i64) -> Result<Vec<Item>, KestrelError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM memories
             WHERE approved = 1 AND (expires_at IS NULL OR expires_at > ?1) ORDER BY id"
        );
        select(&self.conn, &sql, [now]).map_err(storage)
    }

    /// The `limit` best live items for `terms` (retrieval::terms of the query).
    pub fn retrieve(
        &self,
        terms: &[String],
        now: i64,
        limit: usize,
        w: &Weights,
    ) -> Result<Vec<Scored>, KestrelError> {
        w.check()?;
        Ok(rank(self.live(now)?, terms, now, limit, w))
    }

    /// The context for a request (Ch. 8 step 3, Ch. 18.2): every live preference, then the `k`
    /// best other live items for the request's terms, within `budget` as counted by `count`.
    pub fn context(
        &self,
        request: &str,
        now: i64,
        k: usize,
        budget: usize,
        count: &dyn Fn(&str) -> usize,
    ) -> Result<Context, KestrelError> {
        let (prefs, others): (Vec<Item>, Vec<Item>) = self
            .live(now)?
            .into_iter()
            .partition(|i| i.kind == Kind::Preference);
        let hits = rank(others, &terms(request), now, k, &Weights::default());
        Ok(context::build(prefs, hits, budget, count))
    }

    /// Every stored item, approved and pending, with provenance, approval and expiry (Ch. 17.3).
    pub fn export_json(&self) -> Result<String, KestrelError> {
        serde_json::to_string_pretty(&self.items(Filter::All)?)
            .map_err(|e| KestrelError::Storage(e.to_string()))
    }

    /// Rebuilds the file without its freed pages (Ch. 19.2 "VACUUM on request"). Old copies may
    /// still exist on the disk itself (Ch. 17.3).
    pub fn vacuum(&self) -> Result<(), KestrelError> {
        self.conn.execute_batch("VACUUM").map_err(storage)
    }

    pub fn audit(&self) -> Result<Vec<Event>, KestrelError> {
        let mut st = self
            .conn
            .prepare("SELECT memory_id, op, at, source FROM memory_events ORDER BY id")
            .map_err(storage)?;
        let rows = st.query_map([], |r| {
            Ok(Event {
                memory_id: r.get(0)?,
                op: r.get(1)?,
                at: r.get(2)?,
                source: r.get(3)?,
            })
        });
        rows.and_then(|rows| rows.collect()).map_err(storage)
    }
}

/// Key `[a-z][a-z0-9_]*` up to MAX_KEY; value and exception non-blank, bounded, without control
/// characters; importance 1..=5; no secret (Ch. 17.1).
fn check(d: &Draft) -> Result<(), KestrelError> {
    let key_ok = (1..=MAX_KEY).contains(&d.key.len())
        && d.key.starts_with(|c: char| c.is_ascii_lowercase())
        && d.key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !key_ok {
        return Err(KestrelError::Invalid(format!("memory key {:?}", d.key)));
    }
    text(d.value, MAX_VALUE, "value")?;
    if let Some(e) = d.exception {
        text(e, MAX_EXCEPTION, "exception")?;
    }
    if !(1..=5).contains(&d.importance) {
        return Err(KestrelError::Invalid(format!(
            "importance {}",
            d.importance
        )));
    }
    let said = format!("{} {}", d.value, d.exception.unwrap_or(""));
    match secrets::refusal(d.key, &said) {
        Some(why) => Err(KestrelError::Denied(format!("not stored: {why}"))),
        None => Ok(()),
    }
}

fn text(s: &str, max: usize, what: &str) -> Result<(), KestrelError> {
    let n = s.trim().chars().count();
    if n == 0 || n > max || s.chars().any(char::is_control) {
        Err(KestrelError::Invalid(format!(
            "memory {what} must be 1..={max} characters without control characters"
        )))
    } else {
        Ok(())
    }
}

fn write(
    conn: &mut Connection,
    d: &Draft,
    source: Source,
    now: i64,
    confirmed: bool,
) -> rusqlite::Result<Proposed> {
    let tx = conn.transaction()?;
    let found: Option<(i64, bool, Option<i64>)> = tx
        .query_row(
            "SELECT id, approved, expires_at FROM memories WHERE kind = ?1 AND key = ?2",
            params![d.kind, d.key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    // An expired row is gone for every purpose, so it expires now instead of meeting the draft.
    let found = match found {
        Some((id, _, Some(at))) if at <= now => {
            tx.execute("DELETE FROM memories WHERE id = ?1", [id])?;
            log(&tx, id, "expire", now, "system")?;
            None
        }
        other => other.map(|(id, approved, _)| (id, approved)),
    };
    let guess = source == Source::ModelInferred;
    let kept = |id| {
        if guess {
            Proposed::Pending(id)
        } else {
            Proposed::Stored(id)
        }
    };
    let (value, exception) = (d.value.trim(), d.exception.map(str::trim));
    let outcome = match found {
        Some((id, true)) if guess => Proposed::Rejected(id),
        Some((id, _)) => {
            tx.execute(
                "UPDATE memories SET value = ?2, exception = ?3, source = ?4, importance = ?5,
                   approved = ?6, updated_at = ?7, expires_at = ?8 WHERE id = ?1",
                params![
                    id,
                    value,
                    exception,
                    source,
                    d.importance,
                    !guess,
                    now,
                    d.expires_at
                ],
            )?;
            kept(id)
        }
        None => {
            tx.execute(
                "INSERT INTO memories(kind, key, value, exception, source, importance, approved,
                   created_at, updated_at, expires_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
                params![d.kind, d.key, value, exception, source, d.importance, !guess, now, d.expires_at],
            )?;
            kept(tx.last_insert_rowid())
        }
    };
    let op = match outcome {
        Proposed::Stored(_) if confirmed => "confirm",
        Proposed::Stored(_) => "store",
        Proposed::Pending(_) => "pending",
        Proposed::Rejected(_) => "reject",
    };
    log(&tx, outcome.id(), op, now, source.as_str())?;
    tx.commit()?;
    Ok(outcome)
}

/// Runs `change` in a transaction and audits it as the user's `op` if it changed a row.
fn logged(
    conn: &mut Connection,
    id: i64,
    op: &str,
    now: i64,
    change: impl FnOnce(&Transaction) -> rusqlite::Result<usize>,
) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    if change(&tx)? == 0 {
        return Ok(false);
    }
    log(&tx, id, op, now, Source::UserExplicit.as_str())?;
    tx.commit()?;
    Ok(true)
}

fn purge(conn: &mut Connection, now: i64) -> rusqlite::Result<usize> {
    let tx = conn.transaction()?;
    let ids: Vec<i64> = tx
        .prepare(
            "SELECT id FROM memories WHERE expires_at IS NOT NULL AND expires_at <= ?1 ORDER BY id",
        )?
        .query_map([now], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for &id in &ids {
        tx.execute("DELETE FROM memories WHERE id = ?1", [id])?;
        log(&tx, id, "expire", now, "system")?;
    }
    tx.commit()?;
    Ok(ids.len())
}

fn log(conn: &Connection, id: i64, op: &str, at: i64, by: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO memory_events(memory_id, op, at, source) VALUES(?1, ?2, ?3, ?4)",
        params![id, op, at, by],
    )?;
    Ok(())
}

fn select(conn: &Connection, sql: &str, p: impl Params) -> rusqlite::Result<Vec<Item>> {
    let mut st = conn.prepare(sql)?;
    let rows = st.query_map(p, |r| {
        Ok(Item {
            id: r.get(0)?,
            kind: r.get(1)?,
            key: r.get(2)?,
            value: r.get(3)?,
            exception: r.get(4)?,
            source: r.get(5)?,
            importance: r.get(6)?,
            approved: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
            expires_at: r.get(10)?,
        })
    })?;
    rows.collect()
}

/// Creates the file 0600 if it is missing and tightens an existing one (Ch. 19.2).
#[cfg(unix)]
fn make_private(path: &Path) -> Result<(), KestrelError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let io = |e: std::io::Error| KestrelError::Storage(format!("{}: {e}", path.display()));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(io)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(io)
}

#[cfg(not(unix))]
fn make_private(_: &Path) -> Result<(), KestrelError> {
    Ok(())
}

#[cfg(test)]
pub(crate) fn temp_db(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("kestrel_memory_{name}_{}.db", std::process::id()));
    for suffix in ["", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", p.display()));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::{DAY, terms};

    fn mem() -> Memory {
        Memory::in_memory().unwrap()
    }

    fn pref(value: &str) -> Draft<'_> {
        Draft::new(Kind::Preference, "answer_length", value)
    }

    fn visible(m: &Memory, query: &str, now: i64) -> Vec<Item> {
        m.retrieve(&terms(query), now, 10, &Weights::default())
            .unwrap()
            .into_iter()
            .map(|s| s.item)
            .collect()
    }

    fn ops(m: &Memory) -> Vec<String> {
        m.audit().unwrap().into_iter().map(|e| e.op).collect()
    }

    #[test]
    fn model_inferred_is_invisible_until_approved() {
        let mut m = mem();
        let p = m.propose(&pref("long"), Source::ModelInferred, 0).unwrap();
        assert!(matches!(p, Proposed::Pending(_)));
        assert!(visible(&m, "answer", 10).is_empty());
        m.approve(p.id(), 11).unwrap();
        assert_eq!(visible(&m, "answer", 12).len(), 1);
        assert!(
            m.approve(p.id(), 13).is_err(),
            "nothing is pending any more"
        );
        assert_eq!(ops(&m), ["pending", "approve"]);
    }

    #[test]
    fn upsert_deduplicates_and_correction_overwrites() {
        let mut m = mem();
        let a = m.propose(&pref("long"), Source::UserExplicit, 0).unwrap();
        let b = m
            .propose(
                &pref("concise").exception("i ask for details").importance(4),
                Source::Correction,
                5,
            )
            .unwrap();
        assert_eq!(b, Proposed::Stored(a.id()));
        let hits = visible(&m, "answer_length", 6);
        assert_eq!(hits.len(), 1);
        let h = &hits[0];
        assert_eq!(
            (
                h.value.as_str(),
                h.source,
                h.exception.as_deref(),
                h.importance
            ),
            ("concise", Source::Correction, Some("i ask for details"), 4)
        );
        assert_eq!((h.created_at, h.updated_at), (0, 5));
    }

    #[test]
    fn model_inferred_cannot_overwrite_user_stated_value() {
        let mut m = mem();
        let id = m
            .propose(&pref("concise"), Source::UserExplicit, 0)
            .unwrap()
            .id();
        assert_eq!(
            m.propose(&pref("long"), Source::ModelInferred, 5).unwrap(),
            Proposed::Rejected(id)
        );
        let hits = visible(&m, "answer_length", 6);
        assert_eq!(hits.len(), 1, "the user's approved value must stay visible");
        assert_eq!(
            (hits[0].value.as_str(), hits[0].source),
            ("concise", Source::UserExplicit)
        );
        assert_eq!(ops(&m), ["store", "reject"]);
    }

    #[test]
    fn a_new_guess_never_hides_an_approved_one() {
        // A.11 let a second guess overwrite an approved guess and set approved = 0 again.
        let mut m = mem();
        let id = m
            .propose(&pref("long"), Source::ModelInferred, 0)
            .unwrap()
            .id();
        m.approve(id, 1).unwrap();
        assert_eq!(
            m.propose(&pref("very long"), Source::ModelInferred, 2)
                .unwrap(),
            Proposed::Rejected(id)
        );
        assert_eq!(visible(&m, "answer", 3)[0].value, "long");
    }

    #[test]
    fn a_pending_guess_is_replaced_by_a_newer_guess() {
        let mut m = mem();
        let id = m
            .propose(&pref("long"), Source::ModelInferred, 0)
            .unwrap()
            .id();
        assert_eq!(
            m.propose(&pref("short"), Source::ModelInferred, 1).unwrap(),
            Proposed::Pending(id)
        );
        let pending = m.items(Filter::Pending).unwrap();
        assert_eq!((pending.len(), pending[0].value.as_str()), (1, "short"));
        assert!(m.items(Filter::Approved).unwrap().is_empty());
    }

    #[test]
    fn a_confirmed_value_is_the_users() {
        // ADR-026 (4): the user said yes at the prompt, so it replaces their earlier value.
        let mut m = mem();
        let id = m
            .propose(&pref("concise"), Source::UserExplicit, 0)
            .unwrap()
            .id();
        assert_eq!(
            m.confirm(&pref("detailed"), Source::ModelInferred, 5)
                .unwrap(),
            id
        );
        let h = &visible(&m, "answer", 6)[0];
        assert_eq!(
            (h.value.as_str(), h.source, h.approved),
            ("detailed", Source::UserExplicit, true)
        );
        // A confirmed correction keeps its provenance (Ch. 20 step 4).
        m.confirm(&pref("short"), Source::Correction, 7).unwrap();
        assert_eq!(visible(&m, "answer", 8)[0].source, Source::Correction);
        let events: Vec<(String, String)> = m
            .audit()
            .unwrap()
            .into_iter()
            .map(|e| (e.op, e.source))
            .collect();
        let ev = |op: &str, by: &str| (op.to_string(), by.to_string());
        assert_eq!(
            events,
            [
                ev("store", "user_explicit"),
                ev("confirm", "user_explicit"),
                ev("confirm", "correction")
            ]
        );
    }

    #[test]
    fn the_schema_enforces_kinds_and_precedence() {
        // Ch. 18.2 "enforced in SQL": raw SQL cannot break the rules either.
        let mut m = mem();
        let raw = |m: &Memory, sql: &str| m.conn.execute(sql, []);
        assert!(
            raw(
                &m,
                "INSERT INTO memories(kind,key,value,source,created_at,updated_at)
                 VALUES('password','k','v','user_explicit',0,0)"
            )
            .is_err()
        );
        assert!(
            raw(
                &m,
                "INSERT INTO memories(kind,key,value,source,approved,created_at,updated_at)
                 VALUES('fact','k','v','model_inferred',1,0,0)"
            )
            .is_err(),
            "a guess starts pending"
        );
        let id = m
            .propose(&pref("concise"), Source::UserExplicit, 0)
            .unwrap()
            .id();
        let sql =
            format!("UPDATE memories SET value='long', source='model_inferred' WHERE id={id}");
        assert!(raw(&m, &sql).is_err());
        assert_eq!(visible(&m, "answer", 1)[0].value, "concise");
    }

    #[test]
    fn expiry_and_ranking() {
        let mut m = mem();
        let deadline = Draft::new(
            Kind::Fact,
            "project_deadline",
            "Kestrel draft due in week 16",
        );
        m.propose(&deadline.importance(5), Source::UserExplicit, 0)
            .unwrap();
        let meeting = Draft::new(Kind::Fact, "old_meeting", "Kestrel sync with advisor");
        let old = m
            .propose(
                &meeting.importance(1).expires_at(DAY),
                Source::UserExplicit,
                0,
            )
            .unwrap()
            .id();
        let hits = visible(&m, "kestrel deadline", 2 * DAY);
        assert_eq!(hits.len(), 1, "an expired item is never returned");
        assert_eq!(hits[0].key, "project_deadline");
        assert_eq!(m.purge_expired(2 * DAY).unwrap(), 1);
        let last = m.audit().unwrap().pop().unwrap();
        assert_eq!(
            (last.memory_id, last.op.as_str(), last.source.as_str()),
            (old, "expire", "system")
        );
        assert_eq!(m.purge_expired(3 * DAY).unwrap(), 0);
    }

    #[test]
    fn a_proposal_meeting_an_expired_row_starts_fresh() {
        let mut m = mem();
        let old = m
            .propose(&pref("concise").expires_at(DAY), Source::UserExplicit, 0)
            .unwrap()
            .id();
        let p = m
            .propose(&pref("long"), Source::ModelInferred, 2 * DAY)
            .unwrap();
        assert!(
            matches!(p, Proposed::Pending(id) if id > old),
            "{p:?}: a new row, a new id"
        );
        assert_eq!(ops(&m), ["store", "expire", "pending"]);
    }

    #[test]
    fn deletion_removes_content_and_audit_has_no_content() {
        let mut m = mem();
        let d = Draft::new(Kind::Fact, "home_address", "very-private-value");
        let id = m.propose(&d, Source::UserExplicit, 0).unwrap().id();
        assert!(m.delete(id, 1).unwrap());
        assert!(!m.export_json().unwrap().contains("very-private-value"));
        assert_eq!(ops(&m), ["store", "delete"]);
        assert!(!m.delete(id, 2).unwrap());
        let audit = serde_json::to_string(&m.audit().unwrap()).unwrap();
        assert!(
            !audit.contains("private") && !audit.contains("home"),
            "{audit}"
        );
        let again = m.propose(&d, Source::UserExplicit, 3).unwrap().id();
        assert!(again > id, "a deleted id is never reused");
    }

    #[test]
    fn deleted_content_is_not_left_in_the_file() {
        // Ch. 42 test 8 (A.20b): with secure_delete the canary is overwritten in the database file.
        let path = temp_db("remanence");
        let canary = "canary-7f3a9c-remanence";
        let file_has = |p: &Path| {
            let bytes = std::fs::read(p).unwrap();
            bytes.windows(canary.len()).any(|w| w == canary.as_bytes())
        };
        let mut m = Memory::open(&path).unwrap();
        let id = m
            .propose(
                &Draft::new(Kind::Fact, "canary", canary),
                Source::UserExplicit,
                0,
            )
            .unwrap()
            .id();
        assert!(file_has(&path), "the value is in the file before deletion");
        m.delete(id, 1).unwrap();
        drop(m);
        assert!(!file_has(&path));
    }

    #[test]
    fn memories_persist_across_opens() {
        let path = temp_db("persist");
        {
            let mut m = Memory::open(&path).unwrap();
            m.propose(&pref("concise"), Source::UserExplicit, 0)
                .unwrap();
        }
        let m = Memory::open(&path).unwrap();
        assert_eq!(visible(&m, "answer", 1)[0].value, "concise");
    }

    #[test]
    fn vacuum_gives_freed_pages_back() {
        // Ch. 19.2: VACUUM on request rebuilds the file without the freed pages.
        let path = temp_db("vacuum");
        let mut m = Memory::open(&path).unwrap();
        let value = "x".repeat(400);
        let mut ids = Vec::new();
        for i in 0..200 {
            let key = format!("fact_{i}");
            let d = Draft::new(Kind::Fact, &key, &value);
            ids.push(m.propose(&d, Source::UserExplicit, 0).unwrap().id());
        }
        for id in ids {
            m.delete(id, 1).unwrap();
        }
        let size = |p: &Path| std::fs::metadata(p).unwrap().len();
        let before = size(&path);
        m.vacuum().unwrap();
        // ~124 KiB of mostly freed pages -> the schema and 400 audit rows (~36 KiB here).
        assert!(size(&path) < before / 2, "{} -> {}", before, size(&path));
        assert!(
            m.audit().unwrap().len() == 400,
            "the audit survives a vacuum"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_database_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let fresh = temp_db("mode_new");
        let _a = Memory::open(&fresh).unwrap();
        assert_eq!(mode(&fresh), 0o600);
        let loose = temp_db("mode_loose");
        std::fs::write(&loose, b"").unwrap();
        std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _b = Memory::open(&loose).unwrap();
        assert_eq!(mode(&loose), 0o600, "an existing file is tightened");
    }

    #[test]
    fn secrets_and_bad_drafts_are_refused_without_a_trace() {
        let mut m = mem();
        let secret = Draft::new(Kind::Fact, "wifi_password", "hunter2");
        assert!(matches!(
            m.propose(&secret, Source::UserExplicit, 0),
            Err(KestrelError::Denied(_))
        ));
        for d in [
            Draft::new(Kind::Fact, "", "v"),
            Draft::new(Kind::Fact, "Bad Key", "v"),
            Draft::new(Kind::Fact, "k", "  "),
            Draft::new(Kind::Fact, "k", "line\u{0}break"),
            Draft::new(Kind::Fact, "k", "v").importance(0),
            Draft::new(Kind::Fact, "k", "v").importance(6),
            Draft::new(Kind::Fact, "k", "v").exception(""),
        ] {
            assert!(
                matches!(
                    m.propose(&d, Source::UserExplicit, 0),
                    Err(KestrelError::Invalid(_))
                ),
                "{d:?}"
            );
        }
        assert!(m.items(Filter::All).unwrap().is_empty());
        assert!(
            m.audit().unwrap().is_empty(),
            "a refused write leaves no event"
        );
    }

    #[test]
    fn export_carries_provenance_approval_and_expiry() {
        let mut m = mem();
        m.propose(&pref("concise"), Source::UserExplicit, 0)
            .unwrap();
        let advisor = Draft::new(Kind::Fact, "advisor", "Dr. Rao").expires_at(9 * DAY);
        m.propose(&advisor, Source::ModelInferred, 1).unwrap();
        let v: serde_json::Value = serde_json::from_str(&m.export_json().unwrap()).unwrap();
        assert_eq!(
            (&v[0]["kind"], &v[0]["source"], &v[0]["approved"]),
            (&"preference".into(), &"user_explicit".into(), &true.into())
        );
        assert_eq!(
            (&v[1]["source"], &v[1]["approved"], &v[1]["expires_at"]),
            (&"model_inferred".into(), &false.into(), &(9 * DAY).into())
        );
    }

    #[test]
    fn context_has_the_preferences_and_the_relevant_approved_items() {
        let mut m = mem();
        let p = m
            .propose(&pref("concise"), Source::UserExplicit, 0)
            .unwrap()
            .id();
        let advisor = Draft::new(Kind::Fact, "advisor", "Dr. Rao reviews the budget");
        let a = m.propose(&advisor, Source::UserExplicit, 0).unwrap().id();
        m.propose(
            &Draft::new(Kind::Fact, "groceries", "milk and eggs"),
            Source::UserExplicit,
            0,
        )
        .unwrap();
        let guess = Draft::new(Kind::Fact, "budget_guess", "budget due monday");
        m.propose(&guess, Source::ModelInferred, 0).unwrap();
        let c = m
            .context("summarize my budget notes", DAY, 5, 1000, &|s| s.len())
            .unwrap();
        assert_eq!(c.ids, [p, a]);
        assert_eq!(
            c.text,
            "[pref|user_explicit|1970-01-01] answer_length: concise\n\
             [fact|user_explicit|1970-01-01] advisor: Dr. Rao reviews the budget"
        );
    }
}
