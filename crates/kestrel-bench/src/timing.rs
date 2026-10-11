//! Timing on a monotonic clock (Ch. 36 end-to-end latency, Ch. 37.1 warm inference).

use std::hint::black_box;
use std::time::{Duration, Instant};

/// Calls `f` `warmup` times untimed, then `reps` times timed, and returns the `reps` durations.
/// Ch. 37.1 "warm inference" needs `warmup >= 5`.
pub fn measure<T>(warmup: usize, reps: usize, mut f: impl FnMut() -> T) -> Vec<Duration> {
    for _ in 0..warmup {
        black_box(f());
    }
    (0..reps)
        .map(|_| {
            let start = Instant::now();
            black_box(f());
            start.elapsed()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn runs_warmup_then_reps_and_returns_only_reps() {
        let mut calls = 0;
        let d = measure(5, 30, || calls += 1);
        assert_eq!((calls, d.len()), (35, 30));
    }

    /// Warm-up calls are instant, timed calls sleep 1 ms: every returned duration must be a timed call.
    #[test]
    fn warmup_calls_are_not_timed() {
        let mut calls = 0;
        let d = measure(2, 3, || {
            calls += 1;
            if calls > 2 {
                sleep(Duration::from_millis(1));
            }
        });
        assert!(d.iter().all(|&x| x >= Duration::from_millis(1)), "{d:?}");
    }

    #[test]
    fn zero_reps_is_empty() {
        assert!(measure(1, 0, || ()).is_empty());
    }
}
