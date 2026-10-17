//! Cooperative cancellation and deadlines (blueprint A.7), polled between generated tokens and
//! between plan nodes.
//!
//! Change from A.7: an expired deadline is `KestrelError::Timeout { tier }`, not
//! `Invalid("deadline exceeded")`. A timeout is not bad input; the router must be able to tell
//! the two apart (escalate or abstain vs reject). The LRU `ModelRegistry` of A.7 comes later,
//! with the CLI that loads several models.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::{KestrelError, Tier};

/// Shared flag: any clone can cancel, every holder sees it.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    // Relaxed is enough: the flag guards no other data, it only has to become visible.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Deadline(Instant);

impl Deadline {
    pub fn after(budget: Duration) -> Self {
        Self(Instant::now() + budget)
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.0
    }
}

/// Call once per generated token or loop step. Cancellation wins over an expired deadline.
pub fn check(cancel: &CancelToken, deadline: &Deadline, tier: Tier) -> Result<(), KestrelError> {
    if cancel.is_cancelled() {
        return Err(KestrelError::Cancelled);
    }
    if deadline.expired() {
        return Err(KestrelError::Timeout { tier });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn far() -> Deadline {
        Deadline::after(Duration::from_secs(60))
    }

    #[test]
    fn a_fresh_token_with_time_left_passes() {
        assert!(check(&CancelToken::new(), &far(), Tier::Generative).is_ok());
    }

    #[test]
    fn cancelling_any_clone_cancels_all() {
        let a = CancelToken::new();
        let b = a.clone();
        b.cancel();
        assert!(a.is_cancelled());
        assert!(matches!(
            check(&a, &far(), Tier::Generative),
            Err(KestrelError::Cancelled)
        ));
    }

    #[test]
    fn an_expired_deadline_is_a_timeout_for_that_tier() {
        let gone = Deadline::after(Duration::ZERO);
        assert!(gone.expired());
        assert!(matches!(
            check(&CancelToken::new(), &gone, Tier::Encoder),
            Err(KestrelError::Timeout {
                tier: Tier::Encoder
            })
        ));
    }

    #[test]
    fn cancellation_wins_over_timeout() {
        let c = CancelToken::new();
        c.cancel();
        let gone = Deadline::after(Duration::ZERO);
        assert!(matches!(
            check(&c, &gone, Tier::Generative),
            Err(KestrelError::Cancelled)
        ));
    }

    #[test]
    fn cancel_works_across_threads() {
        let c = CancelToken::new();
        let c2 = c.clone();
        std::thread::spawn(move || c2.cancel()).join().unwrap();
        assert!(c.is_cancelled());
    }
}
