//! The contract every cascade tier implements (blueprint Ch. 7.3).
//! Lives in core, not the router, so tier crates don't depend on the router (Ch. 24.2).

use crate::{Intent, KestrelError, Request, Tier};

/// Contract: (1) probabilities are calibrated, or the tier returns none;
/// (2) errors are values, never panics; (3) cost comes from the benchmark harness;
/// (4) an empty vector means "no opinion", and the router escalates.
pub trait TierModel {
    fn tier(&self) -> Tier;
    /// Measured p95 cost in milliseconds, not a guess.
    fn est_cost_ms(&self) -> u32;
    fn classify(&self, req: &Request) -> Result<Vec<(Intent, f32)>, KestrelError>;
}
