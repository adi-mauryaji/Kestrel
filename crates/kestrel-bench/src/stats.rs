//! Latency summaries (blueprint Ch. 36, A.4).
//!
//! Differences from the A.4 reference, each covered by a test:
//! - the nearest-rank index is integer arithmetic. A.4 computes `ceil(p / 100 * n)` in floats,
//!   so p7 of 100 samples picks the 8th (0.07 * 100.0 = 7.000000000000001). p50 and p95 happen
//!   to be safe; any other percentile isn't;
//! - samples are sorted as `Duration`s (a total order) instead of `partial_cmp().unwrap()` on floats;
//! - sample SD and CV are included for the repeatability check (Ch. 37.4, Week-2 "T").

use std::time::Duration;

/// Summary of one run's latencies, in milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct LatencyStats {
    pub n: usize,
    pub mean_ms: f64,
    /// Sample standard deviation (n − 1). `None` for a single sample.
    pub sd_ms: Option<f64>,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

impl LatencyStats {
    /// Coefficient of variation, `sd / mean`. `None` for a single sample or a zero mean.
    pub fn cv(&self) -> Option<f64> {
        let sd = self.sd_ms?;
        (self.mean_ms > 0.0).then(|| sd / self.mean_ms)
    }
}

/// Nearest-rank percentile: the smallest sample such that at least `p`% of samples are <= it.
/// `sorted` must be non-empty and ascending; `p` is a whole percent in 0..=100.
fn percentile_sorted(sorted: &[Duration], p: usize) -> Duration {
    debug_assert!(!sorted.is_empty() && p <= 100);
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

/// Exact for whole milliseconds: integer nanoseconds divided once.
fn ms(d: Duration) -> f64 {
    d.as_nanos() as f64 / 1e6
}

/// `None` when there are no samples.
pub fn summarize(samples: &[Duration]) -> Option<LatencyStats> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    let mean_ms = sorted.iter().map(|&d| ms(d)).sum::<f64>() / n as f64;
    let sd_ms = (n > 1).then(|| {
        let squares: f64 = sorted.iter().map(|&d| (ms(d) - mean_ms).powi(2)).sum();
        (squares / (n - 1) as f64).sqrt()
    });
    Some(LatencyStats {
        n,
        mean_ms,
        sd_ms,
        p50_ms: ms(percentile_sorted(&sorted, 50)),
        p95_ms: ms(percentile_sorted(&sorted, 95)),
        max_ms: ms(sorted[n - 1]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn millis(v: &[u64]) -> Vec<Duration> {
        v.iter().copied().map(Duration::from_millis).collect()
    }

    #[test]
    fn percentiles_nearest_rank() {
        let s: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        let st = summarize(&s).unwrap();
        assert_eq!(
            (st.n, st.p50_ms, st.p95_ms, st.max_ms),
            (100, 50.0, 95.0, 100.0)
        );
        assert!((st.mean_ms - 50.5).abs() < 1e-9);
    }

    /// A.4 computes the rank as ceil(0.07 * 100.0) = ceil(7.000000000000001) = 8.
    #[test]
    fn percentile_rank_has_no_float_rounding_error() {
        let s: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(percentile_sorted(&s, 7), Duration::from_millis(7));
    }

    #[test]
    fn small_samples_pick_a_sample_not_an_interpolation() {
        let st = summarize(&millis(&[10, 20, 30])).unwrap();
        assert_eq!((st.p50_ms, st.p95_ms), (20.0, 30.0));
    }

    #[test]
    fn input_order_does_not_matter() {
        let up = summarize(&millis(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10])).unwrap();
        let down = summarize(&millis(&[10, 9, 8, 7, 6, 5, 4, 3, 2, 1])).unwrap();
        assert_eq!(up, down);
        assert_eq!((up.p50_ms, up.p95_ms), (5.0, 10.0));
    }

    #[test]
    fn sample_sd_and_cv() {
        // Squared deviations from the mean 5 sum to 32, so the sample variance is 32/7.
        let st = summarize(&millis(&[2, 4, 4, 4, 5, 5, 7, 9])).unwrap();
        let sd = (32.0_f64 / 7.0).sqrt();
        assert!((st.sd_ms.unwrap() - sd).abs() < 1e-12);
        assert!((st.cv().unwrap() - sd / 5.0).abs() < 1e-12);
    }

    #[test]
    fn single_sample_has_no_spread() {
        let st = summarize(&millis(&[5])).unwrap();
        assert_eq!((st.sd_ms, st.cv()), (None, None));
        assert_eq!((st.p50_ms, st.p95_ms, st.max_ms), (5.0, 5.0, 5.0));
    }

    #[test]
    fn empty_is_none() {
        assert!(summarize(&[]).is_none());
    }
}
