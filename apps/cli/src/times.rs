//! Times of day in a request (ADR-034, Stage 2 feature 4), in English and Romanized Hinglish:
//! "at 5 pm", "5:30pm", "17:30", "6 o'clock", "shaam 6 baje", "subah saadhe 9 baje", "dedh baje".
//! App-level like the task commands: the phrase is taken out of a task request before its template
//! runs, so the research templates and datasets never see it. A bare "at 5" is not a time (it
//! stays in the title), nor is "5.30" without am/pm or baje.
//!
//! Times are minutes after local midnight. A time said without am or pm ("6 baje", "6 o'clock",
//! "5:30") is resolved when it is attached to a task: the next time the clock shows it, or, on
//! another day, the daytime reading (7–11 am, 12 noon to 6 pm).

use kestrel_tools::date::Date;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said {
    /// Minutes after midnight, am or pm known.
    At(u32),
    /// Minutes on a 12-hour dial (0..720, 12 o'clock = 0), am or pm not said.
    Either(u32),
}

/// Words before a time that go with it.
const AT: &[&str] = &["at", "by", "around"];
const BAJE: &[&str] = &["baje", "bje"];

/// The first time phrase in `text`: the text without it (and without an "at" / "by" before it),
/// and the time.
pub fn take(text: &str) -> Option<(String, Said)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let k: Vec<String> = words.iter().map(|w| key(w)).collect();
    let (mut start, end, said) = (0..k.len()).find_map(|i| {
        hinglish(&k, i)
            .or_else(|| english(&k, i))
            .map(|(end, s)| (i, end, s))
    })?;
    if start > 0 && AT.contains(&k[start - 1].as_str()) {
        start -= 1;
    }
    let rest: Vec<&str> = words[..start]
        .iter()
        .chain(&words[end..])
        .copied()
        .collect();
    Some((rest.join(" "), said))
}

/// The date and minute a time means for a task due `due` (None: today, or tomorrow once the time
/// has passed), at minute `now` of `today`.
pub fn resolve(said: Said, due: Option<Date>, today: Date, now: u32) -> (Date, u32) {
    // The times of day it could ring, earlier first.
    let ring = match said {
        Said::At(m) => [m, m],
        Said::Either(d) => [d, d + 720],
    };
    let next = ring.into_iter().find(|&m| m >= now);
    match due {
        None => match next {
            Some(m) => (today, m),
            None => (Date::from_days(today.days() + 1), ring[0]),
        },
        // Today, already past: the later reading, which rings at once.
        Some(d) if d == today => (d, next.unwrap_or(ring[1])),
        Some(d) => match said {
            Said::At(m) => (d, m),
            Said::Either(dial) if (7 * 60..12 * 60).contains(&dial) => (d, dial),
            Said::Either(dial) => (d, dial + 720),
        },
    }
}

/// A word as compared: lowercase, without the punctuation around it ("p.m." is "p.m").
fn key(w: &str) -> String {
    w.trim_matches(|c: char| matches!(c, ',' | ';' | '!' | '?' | '"' | '(' | ')'))
        .trim_end_matches('.')
        .to_lowercase()
}

/// "5", "5:30", "5.30", "17:30": the hour, the minutes if given, and how they were written.
struct Number {
    h: u32,
    m: Option<u32>,
    colon: bool,
    two_digit_hour: bool,
}

fn number(s: &str) -> Option<Number> {
    let digits = |t: &str, len: std::ops::RangeInclusive<usize>| {
        len.contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit())
    };
    let (hs, ms) = match s.find([':', '.']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    if !digits(hs, 1..=2) || ms.is_some_and(|m| !digits(m, 2..=2)) {
        return None;
    }
    let m = match ms {
        Some(m) => Some(m.parse().ok().filter(|m| *m < 60)?),
        None => None,
    };
    Some(Number {
        h: hs.parse().ok()?,
        m,
        colon: s.contains(':'),
        two_digit_hour: hs.len() == 2,
    })
}

/// Minutes on the 12-hour dial (12 o'clock = 0); None unless the hour is 1-12.
fn dial(h: u32, m: u32) -> Option<u32> {
    (1..=12).contains(&h).then_some((h % 12) * 60 + m)
}

fn pm(w: &str) -> Option<bool> {
    match w {
        "am" | "a.m" => Some(false),
        "pm" | "p.m" => Some(true),
        _ => None,
    }
}

/// The part of the day: the dial's reading in it.
fn period(w: &str) -> Option<fn(u32) -> u32> {
    Some(match w {
        "subah" | "subha" | "savere" | "sawere" => |d| d,
        // 12 to 6:59 in the afternoon; "dopahar 11 baje" stays 11.
        "dopahar" | "dopehar" | "dupahar" => |d| if d < 7 * 60 { d + 720 } else { d },
        "shaam" | "sham" => |d| d + 720,
        // 12 to 4:59 is after midnight.
        "raat" => |d| if d < 5 * 60 { d } else { d + 720 },
        _ => return None,
    })
}

/// "saadhe 5" = 5:30, "sawa 5" = 5:15, "paune 5" = 4:45.
fn fraction(w: &str) -> Option<i64> {
    match w {
        "saadhe" | "sadhe" | "saade" | "sade" => Some(30),
        "sawa" | "sava" => Some(15),
        "paune" | "pone" => Some(-15),
        _ => None,
    }
}

/// `[subah|dopahar|shaam|raat [ko]] [saadhe|sawa|paune] H|H:MM|dedh|dhai baje|bje [tak]` at `i`:
/// where it ends and the time (Either without a part of the day).
fn hinglish(k: &[String], i: usize) -> Option<(usize, Said)> {
    let word = |j: usize| k.get(j).map(String::as_str);
    let mut j = i;
    let part = word(j).and_then(period);
    if part.is_some() {
        j += 1;
        if word(j) == Some("ko") {
            j += 1;
        }
    }
    let frac = word(j).and_then(fraction);
    if frac.is_some() {
        j += 1;
    }
    let d = match word(j)? {
        "dedh" | "derh" if frac.is_none() => 90,
        "dhai" | "dhaai" | "adhai" if frac.is_none() => 150,
        w => {
            let n = number(w)?;
            if frac.is_some() && n.m.is_some() {
                return None;
            }
            dial(n.h, n.m.unwrap_or(0))?
        }
    };
    if !word(j + 1).is_some_and(|w| BAJE.contains(&w)) {
        return None;
    }
    j += 2;
    if word(j) == Some("tak") {
        j += 1;
    }
    let d = (i64::from(d) + frac.unwrap_or(0)).rem_euclid(720) as u32;
    Some((j, part.map_or(Said::Either(d), |p| Said::At(p(d)))))
}

/// `H[:MM] am|pm`, `H[:MM]pm`, `H o'clock`, `HH:MM` at `i`: where it ends and the time.
fn english(k: &[String], i: usize) -> Option<(usize, Said)> {
    let w = k.get(i)?;
    for suffix in ["am", "pm", "a.m", "p.m"] {
        if let Some(n) = w.strip_suffix(suffix).filter(|n| !n.is_empty()) {
            let n = number(n)?;
            let d = dial(n.h, n.m.unwrap_or(0))?;
            return Some((i + 1, Said::At(if pm(suffix)? { d + 720 } else { d })));
        }
    }
    let n = number(w)?;
    let next = k.get(i + 1).map(String::as_str);
    if let Some(p) = next.and_then(pm) {
        let d = dial(n.h, n.m.unwrap_or(0))?;
        return Some((i + 2, Said::At(if p { d + 720 } else { d })));
    }
    if matches!(next, Some("o'clock" | "oclock")) && n.m.is_none() {
        return Some((i + 2, Said::Either(dial(n.h, 0)?)));
    }
    // A clock time with a colon: 24-hour when it can only be ("17:30", "09:15", "0:30").
    let m = n.m.filter(|_| n.colon && n.h <= 23)?;
    Some((
        i + 1,
        if n.two_digit_hour || n.h == 0 || n.h > 12 {
            Said::At(n.h * 60 + m)
        } else {
            Said::Either(dial(n.h, m)?)
        },
    ))
}

/// 1050 -> "17:30".
pub fn hhmm(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// "17:30" -> 1050 (the memory file's form).
pub fn minutes(hhmm: &str) -> Option<u32> {
    let two = |s: &str| {
        (s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
    };
    let (h, m) = hhmm.split_once(':')?;
    let (h, m) = (two(h)?, two(m)?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32, m: u32) -> Said {
        Said::At(h * 60 + m)
    }

    #[test]
    fn english_times_come_out_of_the_text() {
        let cases = [
            (
                "remind me to call asha at 5 pm",
                "remind me to call asha",
                at(17, 0),
            ),
            (
                "remind me to call asha at 5:30pm tomorrow",
                "remind me to call asha tomorrow",
                at(17, 30),
            ),
            ("pay rent by 11 am", "pay rent", at(11, 0)),
            (
                "call at 5 p.m. about the flat",
                "call about the flat",
                at(17, 0),
            ),
            ("submit the form at 4.45 PM", "submit the form", at(16, 45)),
            ("standup at 09:15", "standup", at(9, 15)),
            ("call ravi 17:30", "call ravi", at(17, 30)),
            ("wake up at 12 am", "wake up", at(0, 0)),
            ("lunch at 12 pm", "lunch", at(12, 0)),
        ];
        for (text, rest, said) in cases {
            assert_eq!(take(text), Some((rest.to_string(), said)), "{text}");
        }
    }

    #[test]
    fn hinglish_times_come_out_of_the_text() {
        let cases = [
            (
                "kal shaam 6 baje asha ko call karna yaad dilana",
                "kal asha ko call karna yaad dilana",
                at(18, 0),
            ),
            (
                "subah saadhe 9 baje gym jaana hai",
                "gym jaana hai",
                at(9, 30),
            ),
            ("shaam ko paune 7 baje dawai", "dawai", at(18, 45)),
            ("raat 11 baje dawai lena", "dawai lena", at(23, 0)),
            ("raat 2 baje alarm", "alarm", at(2, 0)),
            ("raat 12 baje", "", at(0, 0)),
            ("dopahar dedh baje lunch", "lunch", at(13, 30)),
            ("dopahar 12 baje", "", at(12, 0)),
            ("report shaam 5 baje tak bhejna", "report bhejna", at(17, 0)),
            (
                "sawa 8 bje subah wali meeting",
                "subah wali meeting",
                Said::Either(8 * 60 + 15),
            ),
        ];
        for (text, rest, said) in cases {
            assert_eq!(take(text), Some((rest.to_string(), said)), "{text}");
        }
    }

    #[test]
    fn a_time_without_am_or_pm_is_either() {
        let cases = [
            (
                "6 baje maa ko call karna",
                "maa ko call karna",
                Said::Either(360),
            ),
            ("dhai baje", "", Said::Either(150)),
            ("call asha at 6 o'clock", "call asha", Said::Either(360)),
            ("meeting at 5:30", "meeting", Said::Either(330)),
            ("12 baje", "", Said::Either(0)),
        ];
        for (text, rest, said) in cases {
            assert_eq!(take(text), Some((rest.to_string(), said)), "{text}");
        }
    }

    #[test]
    fn numbers_that_are_not_times_stay() {
        for text in [
            "call asha at 6",
            "buy 5 apples",
            "rename task 3 to call ravi",
            "the meeting costs 5.30",
            "13 pm",
            "25:00",
            "5:75",
            "0 baje",
            "13 baje",
            "search my notes for budget",
            "",
        ] {
            assert_eq!(take(text), None, "{text}");
        }
    }

    #[test]
    fn an_undated_time_is_today_until_it_has_passed() {
        let today = Date::parse("2026-10-25").unwrap();
        let tomorrow = Date::parse("2026-10-26").unwrap();
        assert_eq!(resolve(at(17, 30), None, today, 600), (today, 1050));
        assert_eq!(
            resolve(at(17, 30), None, today, 1050),
            (today, 1050),
            "this minute counts"
        );
        assert_eq!(resolve(at(17, 30), None, today, 1080), (tomorrow, 1050));
        // "6 baje": the next time the clock shows 6.
        assert_eq!(resolve(Said::Either(360), None, today, 600), (today, 1080));
        assert_eq!(resolve(Said::Either(360), None, today, 300), (today, 360));
        assert_eq!(
            resolve(Said::Either(360), None, today, 1140),
            (tomorrow, 360)
        );
    }

    #[test]
    fn a_dated_time_keeps_its_date() {
        let today = Date::parse("2026-10-25").unwrap();
        let friday = Date::parse("2026-10-30").unwrap();
        // Today, already past: kept, so it rings at once.
        assert_eq!(resolve(at(9, 0), Some(today), today, 600), (today, 540));
        assert_eq!(
            resolve(Said::Either(360), Some(today), today, 1200),
            (today, 1080)
        );
        // Another day: am/pm as said, else the daytime reading.
        assert_eq!(resolve(at(7, 0), Some(friday), today, 600), (friday, 420));
        assert_eq!(
            resolve(Said::Either(360), Some(friday), today, 600),
            (friday, 1080)
        );
        assert_eq!(
            resolve(Said::Either(540), Some(friday), today, 600),
            (friday, 540)
        );
        assert_eq!(
            resolve(Said::Either(0), Some(friday), today, 600),
            (friday, 720)
        );
    }

    #[test]
    fn minutes_print_as_a_24_hour_clock_and_read_back() {
        assert_eq!(hhmm(1050), "17:30");
        assert_eq!(hhmm(5), "00:05");
        assert_eq!(minutes("17:30"), Some(1050));
        assert_eq!(minutes("00:05"), Some(5));
        for bad in ["24:00", "9:00", "17:60", ""] {
            assert_eq!(minutes(bad), None, "{bad}");
        }
    }
}
