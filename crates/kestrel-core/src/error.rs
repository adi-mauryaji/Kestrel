//! The one failure type every crate returns (blueprint A.1).
//! No `From<rusqlite::Error>` here: core must not depend on SQLite (Ch. 24.4);
//! that conversion belongs in kestrel-memory.

use std::fmt;

use crate::types::Tier;

#[derive(Debug)]
pub enum KestrelError {
    Timeout { tier: Tier },
    ModelUnavailable(Tier),
    Invalid(String),
    Sandbox(String),
    Storage(String),
    Denied(String),
    Cancelled,
}

impl fmt::Display for KestrelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout { tier } => write!(f, "tier {tier:?} timed out"),
            Self::ModelUnavailable(tier) => write!(f, "model for tier {tier:?} unavailable"),
            Self::Invalid(m) => write!(f, "invalid input: {m}"),
            Self::Sandbox(m) => write!(f, "sandbox violation: {m}"),
            Self::Storage(m) => write!(f, "storage error: {m}"),
            Self::Denied(m) => write!(f, "denied: {m}"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for KestrelError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_carry_their_detail() {
        let cases = [
            (
                KestrelError::Timeout {
                    tier: Tier::Encoder,
                },
                "Encoder",
            ),
            (
                KestrelError::ModelUnavailable(Tier::Generative),
                "Generative",
            ),
            (KestrelError::Invalid("empty plan".into()), "empty plan"),
            (KestrelError::Sandbox("../etc".into()), "../etc"),
            (KestrelError::Storage("disk full".into()), "disk full"),
            (KestrelError::Denied("delete_note".into()), "delete_note"),
        ];
        for (err, detail) in cases {
            let msg = err.to_string();
            assert!(msg.contains(detail), "{msg:?} should mention {detail:?}");
        }
    }

    #[test]
    fn usable_as_boxed_std_error() {
        let boxed: Box<dyn std::error::Error> = Box::new(KestrelError::Cancelled);
        assert_eq!(boxed.to_string(), "cancelled");
    }
}
