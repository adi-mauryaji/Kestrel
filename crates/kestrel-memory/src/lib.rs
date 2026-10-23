//! Kestrel memory (blueprint Ch. 17-18, A.11, ADR-026): the only crate that touches SQLite
//! (Ch. 24.4). Rows carry provenance and approval; the audit table records operations, never
//! content; retrieval is the deterministic lexical baseline; the context builder turns approved
//! items into provenance-tagged lines under a token budget. Tasks live in the same file.

pub mod context;
pub mod eval;
pub mod retrieval;
pub mod secrets;
pub mod store;
pub mod tasks;

pub use context::Context;
pub use retrieval::{Scored, Weights};
pub use store::{Draft, Event, Filter, Item, Kind, Memory, Proposed, Source};
pub use tasks::TaskRow;
