//! Kestrel core: the shared vocabulary every other crate depends on.
//! Knows nothing about SQLite, llama.cpp or the filesystem (Ch. 24.4).

pub mod error;
pub mod normalize;
pub mod prob;
pub mod tier_model;
pub mod types;

pub use error::KestrelError;
pub use tier_model::TierModel;
pub use types::{Intent, Prediction, Request, Tier};
