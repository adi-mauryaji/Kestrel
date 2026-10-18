//! Kestrel router: sends each request through the tiers, cheapest first, and stops at the first
//! one confident enough (blueprint Ch. 13). Depends on core only; tiers plug in via `TierModel`.

pub mod router;

pub use router::{AbstainReason, Cascade, Outcome, RouterConfig, Trace};
