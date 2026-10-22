//! Calendar dates for due dates (proleptic Gregorian), std only: day numbers from Howard Hinnant's
//! `days_from_civil` / `civil_from_days`, so adding days and weekdays need no date crate.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

impl Date {
    /// A `YYYY-MM-DD` calendar date (see `args::iso_date`).
    pub fn parse(s: &str) -> Option<Self> {
        crate::args::iso_date(s).ok()?;
        Some(Self {
            y: s[0..4].parse().ok()?,
            m: s[5..7].parse().ok()?,
            d: s[8..10].parse().ok()?,
        })
    }

    /// Days since 1970-01-01.
    pub fn days(self) -> i64 {
        let y = i64::from(self.y) - i64::from(self.m <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400; // [0, 399]
        let mp = (i64::from(self.m) + 9) % 12; // March = 0
        let doy = (153 * mp + 2) / 5 + i64::from(self.d) - 1; // [0, 365]
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Self {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
        Self { y, m, d }
    }

    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.days() + n)
    }

    /// 0 = Monday ... 6 = Sunday.
    pub fn weekday(self) -> u32 {
        (self.days() + 3).rem_euclid(7) as u32 // 1970-01-01 was a Thursday
    }

    /// The first date strictly after `self` that falls on `weekday` (so "friday" said on a
    /// Friday means next week's).
    pub fn next_weekday(self, weekday: u32) -> Self {
        let ahead = (i64::from(weekday) - i64::from(self.weekday())).rem_euclid(7);
        self.add_days(if ahead == 0 { 7 } else { ahead })
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn parses_and_prints_calendar_dates_only() {
        assert_eq!(
            d("2026-10-22"),
            Date {
                y: 2026,
                m: 10,
                d: 22
            }
        );
        assert_eq!(d("2026-10-22").to_string(), "2026-10-22");
        assert!(Date::parse("2026-02-30").is_none() && Date::parse("kal").is_none());
    }

    #[test]
    fn day_numbers_round_trip_across_months_years_and_leap_days() {
        assert_eq!(d("1970-01-01").days(), 0);
        assert_eq!(d("2000-03-01").days() - d("2000-02-28").days(), 2); // 2000 is a leap year
        assert_eq!(d("1900-03-01").days() - d("1900-02-28").days(), 1); // 1900 is not
        for s in [
            "0001-01-01",
            "1969-12-31",
            "2024-02-29",
            "2026-10-22",
            "9999-12-31",
        ] {
            assert_eq!(Date::from_days(d(s).days()), d(s), "{s}");
        }
        assert_eq!(d("2026-12-31").add_days(1), d("2027-01-01"));
        assert_eq!(d("2026-03-01").add_days(-1), d("2026-02-28"));
    }

    #[test]
    fn weekdays_and_the_next_one() {
        assert_eq!(d("2026-10-22").weekday(), 3); // a Thursday
        assert_eq!(d("1970-01-01").weekday(), 3); // also a Thursday
        assert_eq!(d("2026-10-22").next_weekday(4), d("2026-10-23")); // friday
        assert_eq!(d("2026-10-22").next_weekday(0), d("2026-10-26")); // monday
        assert_eq!(d("2026-10-22").next_weekday(3), d("2026-10-29")); // thursday: next week's
    }
}
