//! Kestrel benchmarking: latency stats, timing, peak RSS and CSV result writers
//! (blueprint Ch. 36, 37, 40, A.4). Energy readers come later: RAPL is root-only here.

pub mod csv;
pub mod results;
pub mod rss;
pub mod stats;
pub mod timing;

pub use csv::CsvWriter;
pub use results::{LATENCY_SUMMARY_HEADER, LatencySummary};
pub use rss::{kib_to_mib, peak_rss_kib};
pub use stats::{LatencyStats, summarize};
pub use timing::measure;
