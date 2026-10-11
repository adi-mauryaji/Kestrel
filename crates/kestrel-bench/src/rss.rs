//! Peak resident memory (Ch. 36: `VmHWM`, read at the end of a run, per process).
//!
//! Linux only. Mapped model weights count once their pages are touched, so report RSS
//! together with the page-cache state. The kernel's "kB" means KiB (1024 bytes).

/// Reads the `VmHWM:` line of a `/proc/<pid>/status` text, in KiB.
pub fn parse_vmhwm_kib(status: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let mut parts = line.split_whitespace().skip(1);
    let kib = parts.next()?.parse().ok()?;
    (parts.next() == Some("kB")).then_some(kib)
}

/// This process's peak RSS in KiB. `None` off Linux or when `/proc` can't be read.
pub fn peak_rss_kib() -> Option<u64> {
    parse_vmhwm_kib(&std::fs::read_to_string("/proc/self/status").ok()?)
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

    #[test]
    fn kib_to_mib_is_binary() {
        assert_eq!(kib_to_mib(1536), 1.5);
    }
}
