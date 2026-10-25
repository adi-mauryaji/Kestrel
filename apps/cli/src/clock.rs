//! The local calendar date for the app (ADR-034). Unix time divided by 86 400 is the UTC date, a day
//! behind in India from 00:00 to 05:30, so "kal" and "due today" would be off by one. The standard
//! library has no time zones, so the UTC offset comes from `date +%z` (the system's own zone, DST
//! included), read once; without it the app falls back to UTC.

use kestrel_tools::date::Date;

/// "+0530" -> 19 800 s, "-0800" -> -28 800 s.
pub fn parse_offset(s: &str) -> Option<i64> {
    let s = s.trim_end();
    let (sign, digits) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (h, m): (i64, i64) = (digits[..2].parse().ok()?, digits[2..].parse().ok()?);
    (h <= 14 && m < 60).then_some(sign * (h * 3600 + m * 60))
}

/// The system's UTC offset now, in seconds (0 if `date` is not available).
pub fn utc_offset() -> i64 {
    std::process::Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .and_then(|o| parse_offset(&String::from_utf8_lossy(&o.stdout)))
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
    fn offsets_parse_as_seconds_east_of_utc() {
        assert_eq!(parse_offset("+0530"), Some(19_800));
        assert_eq!(parse_offset("-0800"), Some(-28_800));
        assert_eq!(parse_offset("+0000"), Some(0));
        assert_eq!(parse_offset("+0545\n"), Some(20_700)); // Nepal; `date` ends with a newline
        for bad in ["", "0530", "+05:30", "+5", "+05300", "IST", "+0560"] {
            assert_eq!(parse_offset(bad), None, "{bad}");
        }
    }

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
        // `date` exists on every Linux this runs on; India here, but any zone is fine.
        let off = utc_offset();
        assert!((-12 * 3600..=14 * 3600).contains(&off), "{off}");
    }
}
