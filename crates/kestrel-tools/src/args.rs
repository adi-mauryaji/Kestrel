//! Argument checks shared by tool calls and plan nodes (Ch. 15.2: lengths, control characters,
//! date format, numeric clamps). Model output is untrusted input; every string passes here.
//!
//! Change from A.13: a date must be a real calendar date, not only `dddd-dd-dd`
//! (A.13's check accepts `2026-13-45`).

use kestrel_core::KestrelError;

/// Trimmed text of 1..=`max` characters with no control characters.
pub fn clean_text(s: &str, max: usize, what: &str) -> Result<String, KestrelError> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > max || t.chars().any(char::is_control) {
        return Err(KestrelError::Invalid(format!(
            "{what}: empty, longer than {max} characters, or has control characters"
        )));
    }
    Ok(t.to_string())
}

/// A `YYYY-MM-DD` calendar date (proleptic Gregorian, years 1..=9999).
pub fn iso_date(s: &str) -> Result<String, KestrelError> {
    let bad = || KestrelError::Invalid(format!("date {s:?} is not a calendar date YYYY-MM-DD"));
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        b[r].iter().try_fold(0u32, |n, c| {
            c.is_ascii_digit().then(|| n * 10 + u32::from(c - b'0'))
        })
    };
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return Err(bad());
    }
    let (y, m, d) = (digits(0..4), digits(5..7), digits(8..10));
    let (Some(y), Some(m), Some(d)) = (y, m, d) else {
        return Err(bad());
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(bad()),
    };
    if y == 0 || d == 0 || d > days {
        return Err(bad());
    }
    Ok(s.to_string())
}

/// `n`, or `default` when absent, clamped to `lo..=hi`.
pub fn clamp(n: Option<u32>, default: u32, lo: u32, hi: u32) -> u32 {
    n.unwrap_or(default).clamp(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_trimmed_and_bounded() {
        assert_eq!(clean_text("  budget  ", 200, "query").unwrap(), "budget");
        assert!(clean_text("   ", 200, "query").is_err());
        assert!(clean_text(&"x".repeat(201), 200, "query").is_err());
        assert_eq!(
            clean_text(&"é".repeat(200), 200, "query")
                .unwrap()
                .chars()
                .count(),
            200
        ); // chars, not bytes
        let err = clean_text("a\u{7}b", 200, "title").unwrap_err().to_string();
        assert!(err.contains("title"), "{err}");
        assert!(clean_text("line\nbreak", 200, "title").is_err());
    }

    #[test]
    fn hinglish_and_unicode_text_is_allowed() {
        assert_eq!(
            clean_text("kal asha ko call karna", 200, "title").unwrap(),
            "kal asha ko call karna"
        );
        assert!(clean_text("बजट", 200, "query").is_ok());
    }

    #[test]
    fn dates_must_exist_on_the_calendar() {
        for ok in [
            "2026-10-22",
            "2024-02-29",
            "2000-02-29",
            "0001-01-01",
            "9999-12-31",
        ] {
            assert_eq!(iso_date(ok).unwrap(), ok);
        }
        for bad in [
            "2026-13-01",
            "2026-00-10",
            "2026-02-29",
            "1900-02-29",
            "2026-04-31",
            "2026-10-00",
            "tomorrow",
            "2026-1-05",
            "2026/10/22",
            "0000-01-01",
            " 2026-10-22",
            "२०२६-१०-२२",
        ] {
            assert!(iso_date(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn limits_default_and_clamp() {
        assert_eq!(clamp(None, 10, 1, 50), 10);
        assert_eq!(clamp(Some(9999), 10, 1, 50), 50);
        assert_eq!(clamp(Some(0), 10, 1, 50), 1);
        assert_eq!(clamp(Some(7), 10, 1, 50), 7);
    }
}
