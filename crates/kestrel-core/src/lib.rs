//! Kestrel core: the shared vocabulary every other crate depends on.
//! Knows nothing about SQLite, llama.cpp or the filesystem (Ch. 24.4).

pub mod error;
pub mod normalize;
pub mod types;

pub use error::KestrelError;
pub use types::{Intent, Prediction, Request, Tier};
