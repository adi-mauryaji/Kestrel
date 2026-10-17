//! Peak resident memory (Ch. 36: `VmHWM`, read at the end of a run, per process).
//!
//! Linux only. Mapped model weights count once their pages are touched, so report RSS
//! together with the page-cache state. The kernel's "kB" means KiB (1024 bytes).

/// Reads the `VmHWM:` line of a `/proc/<pid>/status` text, in KiB.
pub fn parse_vmhwm_kib(status: &str) -> Option<u64> {
    parse_status_kib(status, "VmHWM:")
}

/// This process's peak RSS in KiB. `None` off Linux or when `/proc` can't be read.
pub fn peak_rss_kib() -> Option<u64> {
    parse_vmhwm_kib(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// Any `<Key>: <n> kB` line of a `/proc/<pid>/status` text, in KiB (`key` includes the colon).
pub fn parse_status_kib(status: &str, key: &str) -> Option<u64> {
    let mut parts = status
        .lines()
        .find(|l| l.split_whitespace().next() == Some(key))?
        .split_whitespace()
        .skip(1);
    let kib = parts.next()?.parse().ok()?;
    (parts.next() == Some("kB")).then_some(kib)
}

/// Memory split at one moment (PREREG D12): `RssAnon` = heap and other anonymous pages (model
/// weights loaded without mmap live here); `RssFile` = file-backed pages (mmap'd weights).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RssBreakdown {
    pub peak_kib: u64,
    pub anon_kib: u64,
    pub file_kib: u64,
}

pub fn parse_rss_breakdown(status: &str) -> Option<RssBreakdown> {
    Some(RssBreakdown {
        peak_kib: parse_status_kib(status, "VmHWM:")?,
        anon_kib: parse_status_kib(status, "RssAnon:")?,
        file_kib: parse_status_kib(status, "RssFile:")?,
    })
}

/// This process's peak RSS and its current anonymous / file-backed split.
pub fn rss_breakdown() -> Option<RssBreakdown> {
    parse_rss_breakdown(&std::fs::read_to_string("/proc/self/status").ok()?)
}

pub fn kib_to_mib(kib: u64) -> f64 {
    kib as f64 / 1024.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\tkestrel\nVmPeak:\t  912000 kB\nVmSize:\t  900000 kB\n\
                          VmHWM:\t  123456 kB\nVmRSS:\t  100000 kB\nThreads:\t12\n";

    #[test]
    fn parses_vmhwm_not_its_neighbours() {
        assert_eq!(parse_vmhwm_kib(STATUS), Some(123456));
    }

    #[test]
    fn missing_or_malformed_line_is_none() {
        assert_eq!(parse_vmhwm_kib(""), None);
        assert_eq!(parse_vmhwm_kib("VmRSS:\t 100 kB\n"), None);
        assert_eq!(parse_vmhwm_kib("VmHWM:\t abc kB\n"), None);
        assert_eq!(parse_vmhwm_kib("VmHWM:\t 12 MB\n"), None, "unexpected unit");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_own_peak_rss() {
        assert!(peak_rss_kib().unwrap() > 0);
    }

    const STATUS_SPLIT: &str = "VmHWM:\t  2000 kB\nVmRSS:\t  1500 kB\nRssAnon:\t  1200 kB\n\
                                RssFile:\t   290 kB\nRssShmem:\t  10 kB\n";

    #[test]
    fn parses_any_status_field_by_key() {
        assert_eq!(parse_status_kib(STATUS_SPLIT, "RssAnon:"), Some(1200));
        assert_eq!(parse_status_kib(STATUS_SPLIT, "RssFile:"), Some(290));
        assert_eq!(parse_status_kib(STATUS_SPLIT, "RssShmem:"), Some(10));
        assert_eq!(parse_status_kib(STATUS_SPLIT, "VmSwap:"), None);
        // "Rss" alone must not match "RssAnon:".
        assert_eq!(parse_status_kib(STATUS_SPLIT, "Rss:"), None);
    }

    #[test]
    fn breakdown_needs_all_three_fields() {
        assert_eq!(
            parse_rss_breakdown(STATUS_SPLIT),
            Some(RssBreakdown {
                peak_kib: 2000,
                anon_kib: 1200,
                file_kib: 290
            })
        );
        assert_eq!(parse_rss_breakdown(STATUS), None); // no RssAnon/RssFile lines
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_own_breakdown() {
        let b = rss_breakdown().unwrap();
        assert!(b.peak_kib >= b.anon_kib && b.anon_kib > 0);
    }

    #[test]
    fn kib_to_mib_is_binary() {
        assert_eq!(kib_to_mib(1536), 1.5);
    }
}
