//! Kestrel local tools (blueprint Ch. 14-16, ADR-023): the allowlisted tool calls and their
//! argument checks, the path sandbox, and the deterministic validation and execution of a plan.
//! The model only proposes; everything that decides or acts is plain code here.

pub mod args;
pub mod backend;
pub mod date;
pub mod eval;
pub mod plan;
pub mod runner;
pub mod sandbox;
pub mod templates;
pub mod tools;
