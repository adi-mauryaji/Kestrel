//! The local calendar date for the app (ADR-034). Unix time divided by 86 400 is the UTC date, a day
//! behind in India from 00:00 to 05:30, so "kal" and "due today" would be off by one. The standard
//! library has no time zones, so the UTC offset comes from the system itself (kestrel-platform:
//! `date +%z` on Linux and macOS, PowerShell on Windows), DST included; without it the app falls
//! back to UTC.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use kestrel_platform::Os;
use kestrel_tools::date::Date;

/// How long Windows keeps an offset it read: asking starts PowerShell (about half a second), and
/// reminders ask every half minute.
const KEEP_ON_WINDOWS: Duration = Duration::from_secs(600);

/// The system's UTC offset now, in seconds (0 if the system cannot tell).
pub fn utc_offset() -> i64 {
    let os = Os::current();
    if os != Os::Windows {
        return read_offset(os);
    }
    static KEPT: Mutex<Option<(Instant, i64)>> = Mutex::new(None);
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    match *kept {
        Some((at, offset)) if at.elapsed() < KEEP_ON_WINDOWS => offset,
        _ => {
            let offset = read_offset(os);
            *kept = Some((Instant::now(), offset));
            offset
        }
    }
}

fn read_offset(os: Os) -> i64 {
    kestrel_platform::utc_offset_command(os)
        .output()
        .ok()
        .and_then(|o| kestrel_platform::parse_utc_offset(os, &String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(0)
}

/// The calendar date at Unix time `now` in a zone `offset` seconds east of UTC.
pub fn local_date(now: i64, offset: i64) -> Date {
    Date::from_days((now + offset).div_euclid(86_400))
}

/// Minutes after local midnight at Unix time `now`.
pub fn local_minute(now: i64, offset: i64) -> u32 {
    ((now + offset).rem_euclid(86_400) / 60) as u32
}

/// Today's local date.
pub fn today() -> Date {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    local_date(now, utc_offset())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_midnight_in_india_it_is_already_the_next_day() {
        // 2026-10-24 20:00 UTC = 2026-10-25 01:30 IST.
        let now = Date::parse("2026-10-24").unwrap().days() * 86_400 + 20 * 3600;
        assert_eq!(local_date(now, 0).to_string(), "2026-10-24");
        assert_eq!(local_date(now, 19_800).to_string(), "2026-10-25");
        assert_eq!(local_date(now, -28_800).to_string(), "2026-10-24"); // 12:00 in California
        assert_eq!(local_minute(now, 19_800), 90);
        assert_eq!(local_minute(now, -28_800), 12 * 60);
    }

    #[test]
    fn this_machine_has_a_readable_offset() {
        // `date` (PowerShell on Windows) exists wherever this runs; any zone is fine.
        let off = utc_offset();
        assert!((-12 * 3600..=14 * 3600).contains(&off), "{off}");
    }
}
