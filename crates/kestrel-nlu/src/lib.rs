//! Kestrel NLU tiers. Each tier produces calibrated per-intent probabilities, or none (Ch. 24.4).

pub mod rules;

pub use rules::{Matcher, Rule, RuleOutcome, RuleSet, RulesTier};
