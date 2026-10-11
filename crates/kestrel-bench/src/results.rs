//! Result rows written by benchmarks (Ch. 37.1 scenarios, Ch. 40 tables).

use crate::rss::kib_to_mib;
use crate::stats::LatencyStats;

/// One row per system × scenario run. RSS is in MiB, like E0's `peak_rss_mib`.
/// An empty field means "not measured" (or undefined, like the SD of one sample).
pub const LATENCY_SUMMARY_HEADER: [&str; 9] = [
    "system",
    "scenario",
    "n",
    "mean_ms",
    "sd_ms",
    "p50_ms",
    "p95_ms",
    "max_ms",
    "peak_rss_mib",
];

pub struct LatencySummary<'a> {
    /// E.g. "R3" (Ch. 19) or a tier name.
    pub system: &'a str,
    /// A Ch. 37.1 scenario, e.g. "warm_inference".
    pub scenario: &'a str,
    pub stats: &'a LatencyStats,
    pub peak_rss_kib: Option<u64>,
}

impl LatencySummary<'_> {
    /// Fields in `LATENCY_SUMMARY_HEADER` order; the array length makes a mismatch a compile error.
    pub fn fields(&self) -> [String; 9] {
        let s = self.stats;
        [
            self.system.to_owned(),
            self.scenario.to_owned(),
            s.n.to_string(),
            s.mean_ms.to_string(),
            opt(s.sd_ms),
            s.p50_ms.to_string(),
            s.p95_ms.to_string(),
            s.max_ms.to_string(),
            opt(self.peak_rss_kib.map(kib_to_mib)),
        ]
    }
}

fn opt(v: Option<f64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CsvWriter, summarize};
    use std::time::Duration;

    fn row(summary: &LatencySummary) -> String {
        let mut w = CsvWriter::new(Vec::new(), &LATENCY_SUMMARY_HEADER).unwrap();
        w.write_row(&summary.fields()).unwrap();
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        out.lines().nth(1).unwrap().to_owned()
    }

    #[test]
    fn writes_a_full_row() {
        let ms = [10, 20, 30].map(Duration::from_millis);
        let stats = summarize(&ms).unwrap();
        let s = LatencySummary {
            system: "R1",
            scenario: "warm_inference",
            stats: &stats,
            peak_rss_kib: Some(2048),
        };
        assert_eq!(row(&s), "R1,warm_inference,3,20,10,20,30,30,2");
    }

    #[test]
    fn unmeasured_and_undefined_values_are_empty() {
        let stats = summarize(&[Duration::from_millis(5)]).unwrap();
        let s = LatencySummary {
            system: "R0",
            scenario: "cold_start",
            stats: &stats,
            peak_rss_kib: None,
        };
        assert_eq!(row(&s), "R0,cold_start,1,5,,5,5,5,");
    }
}
