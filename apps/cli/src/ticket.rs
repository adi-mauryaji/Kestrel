//! A ticket "prepare + file" (v4 E, blueprint §5): "file this ticket ~/Downloads/irctc.pdf". Rules read the
//! journey from the PDF's text; the person sees what was read and says yes; then the PDF is copied into
//! the notes folder and a reminder is set for before the departure. Nothing here uses the model or the
//! internet, so a wrong date cannot be invented, and a page or a PDF cannot ask for it: only the typed
//! request can.

use std::path::{Path, PathBuf};

use kestrel_memory::Memory;
use kestrel_tools::date::Date;

/// The reminder comes this long before the departure, in minutes.
const BEFORE: u32 = 3 * 60;
/// A reminder before this minute of the day would wake them: it comes the evening before.
const NOT_BEFORE: u32 = 7 * 60;
const EVENING_BEFORE: &str = "21:00";
const THAT_MORNING: &str = "07:00";
/// Words that make a date a booking's, not the journey's.
const NOT_THE_JOURNEY: [&str; 9] = [
    "booked",
    "booking date",
    "issue",
    "transaction",
    "payment",
    "generated",
    "printed",
    "invoice",
    "ticket date",
];
/// Words that name the journey's date or departure.
const THE_JOURNEY: [&str; 9] = [
    "departure",
    "departs",
    "dep:",
    "dep ",
    "journey",
    "doj",
    "travel date",
    "start date",
    "flight date",
];
/// Words that name a departure's time.
const THE_DEPARTURE: [&str; 6] = [
    "departure",
    "departs",
    "dep:",
    "dep ",
    "boarding",
    "reporting",
];
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// The path in a request to file a ticket, as typed.
pub fn parse(text: &str) -> Option<String> {
    let said = text.trim().trim_end_matches(['.', '!', '?']).trim();
    let lower = said.to_lowercase();
    if lower.len() != said.len() {
        return None;
    }
    let (start, end) = pdf_path(&lower)?;
    let around = format!("{} {}", &lower[..start], &lower[end..]);
    let words: Vec<&str> = around.split(|c: char| !c.is_alphanumeric()).collect();
    let has = |c: &[&str]| words.iter().any(|w| c.contains(w));
    if !has(&["ticket", "booking"]) || !has(&["file", "save", "rakh", "add"]) {
        return None;
    }
    Some(said[start..end].to_string())
}

/// Where a path to a PDF is in `lower`: from its "/" or "~/" to its ".pdf".
fn pdf_path(lower: &str) -> Option<(usize, usize)> {
    let starts = lower.char_indices().find(|(at, c)| {
        let begins = *at == 0 || matches!(lower.as_bytes()[at - 1], b' ' | b'\'' | b'"');
        begins && (*c == '/' || lower[*at..].starts_with("~/"))
    });
    let (start, _) = starts?;
    let end = start + lower[start..].find(".pdf")? + ".pdf".len();
    Some((start, end))
}

/// A journey as the rules read it from a ticket's text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Booking {
    /// "train", "flight", "bus" or "journey".
    pub kind: &'static str,
    pub number: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub pnr: Option<String>,
    /// "YYYY-MM-DD".
    pub date: Option<String>,
    /// "HH:MM".
    pub time: Option<String>,
}

fn iso(y: i32, m: u32, d: u32) -> Option<String> {
    let text = format!("{y:04}-{m:02}-{d:02}");
    let date = Date::parse(&text)?;
    // A date that does not exist comes back as another.
    (date.to_string() == text).then_some(text)
}

fn month_of(word: &str) -> Option<u32> {
    let low = word.to_lowercase();
    let at = MONTHS.iter().position(|m| low.starts_with(m))?;
    (word.chars().all(char::is_alphabetic)).then_some(at as u32 + 1)
}

/// The year in a date: four digits, or two after 2000.
fn year_of(word: &str) -> Option<i32> {
    let n: i32 = word.parse().ok()?;
    match word.len() {
        4 => Some(n),
        2 => Some(2000 + n),
        _ => None,
    }
}

/// A date written as one word: 15-Nov-2026, 15/11/2026, 15.11.2026, 2026-11-15.
fn date_in_word(word: &str) -> Option<String> {
    let parts: Vec<&str> = word.split(['-', '/', '.']).collect();
    let [a, b, c] = parts.as_slice() else {
        return None;
    };
    if a.len() == 4 {
        return iso(a.parse().ok()?, b.parse().ok()?, c.parse().ok()?);
    }
    let day: u32 = a.parse().ok()?;
    let month = month_of(b).or_else(|| b.parse().ok())?;
    iso(year_of(c)?, month, day)
}

/// The first date in `line`, in any form a ticket uses.
fn date_in(line: &str) -> Option<String> {
    let trim = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let words: Vec<String> = line.split_whitespace().map(trim).collect();
    for (i, word) in words.iter().enumerate() {
        if let Some(found) = date_in_word(word) {
            return Some(found);
        }
        let (Some(b), Some(c)) = (words.get(i + 1), words.get(i + 2)) else {
            continue;
        };
        // 15 Nov 2026, Nov 15 2026.
        let found = match (word.parse::<u32>(), month_of(word)) {
            (Ok(day), _) => month_of(b).and_then(|m| iso(year_of(c)?, m, day)),
            (_, Some(m)) => b.parse().ok().and_then(|day| iso(year_of(c)?, m, day)),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The first time in `line`: 06:10, 6:05 pm, 18:45 hrs, 0605 hrs.
fn time_in(line: &str) -> Option<String> {
    let low = line.to_lowercase();
    let words: Vec<&str> = low.split_whitespace().collect();
    for (i, raw) in words.iter().enumerate() {
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != ':');
        let next = words.get(i + 1).copied().unwrap_or("");
        let (am, pm) = (
            next.starts_with("am") || word.ends_with("am"),
            next.starts_with("pm") || word.ends_with("pm"),
        );
        let digits = word.trim_end_matches(|c: char| c.is_alphabetic());
        let (h, m) = match digits.split_once(':') {
            Some((h, m)) => (h.to_string(), m.get(..2).unwrap_or(m).to_string()),
            // 0605 hrs: only with its word, or any number would do.
            None if digits.len() == 4 && next.starts_with("hrs") => {
                (digits[..2].to_string(), digits[2..].to_string())
            }
            None => continue,
        };
        let (Ok(mut h), Ok(m)) = (h.parse::<u32>(), m.parse::<u32>()) else {
            continue;
        };
        if pm && h < 12 {
            h += 12;
        }
        if am && h == 12 {
            h = 0;
        }
        if h < 24 && m < 60 {
            return Some(format!("{h:02}:{m:02}"));
        }
    }
    None
}

/// The part of a line before its arrival: a ticket often writes both on one line.
fn before_arrival(line: &str) -> &str {
    let low = line.to_ascii_lowercase();
    match low.find("arriv") {
        Some(at) if line.is_char_boundary(at) => &line[..at],
        _ => line,
    }
}

/// The text after `label`, in `line`, cut where the next label starts.
fn after_label(line: &str, label: &str) -> Option<String> {
    let low = line.to_ascii_lowercase();
    let at = low.find(label)?;
    let rest = line.get(at + label.len()..)?;
    let lower = rest.to_ascii_lowercase();
    let ends = ["  ", " to:", " to ", " date", " dep", " class", " quota"];
    let end = ends
        .iter()
        .filter_map(|e| lower.find(e))
        .min()
        .unwrap_or(rest.len());
    let value = rest.get(..end).unwrap_or(rest).trim();
    (!value.is_empty()).then(|| value.chars().take(MOST_PLACE).collect())
}

/// The longest name of a place kept, in characters.
const MOST_PLACE: usize = 40;

/// "DEL → BOM", "DEL - BOM": a route in codes.
fn route_in(line: &str) -> Option<(String, String)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let code = |w: &str| w.len() == 3 && w.chars().all(|c| c.is_ascii_uppercase());
    match words.as_slice() {
        [a, sign, b, ..]
            if code(a) && code(b) && matches!(*sign, "→" | "->" | "-" | "to" | "–") =>
        {
            Some((a.to_string(), b.to_string()))
        }
        _ => None,
    }
}

/// "NEW DELHI (NDLS) LUCKNOW NR (LKO)": the stations on a line of a table, first to last. A line with a
/// label on it is not one: "Class: CHAIR CAR (CC)".
fn stations_in(line: &str) -> Option<(String, String)> {
    if line.contains(':') {
        return None;
    }
    let found: Vec<String> = line
        .split_inclusive(')')
        .filter_map(|piece| {
            let (name, code) = piece.split_once('(')?;
            let code = code.strip_suffix(')')?;
            let is_code =
                (2..=5).contains(&code.len()) && code.chars().all(|c| c.is_ascii_uppercase());
            (is_code && !name.trim().is_empty()).then(|| format!("{} ({code})", name.trim()))
        })
        .collect();
    match found.as_slice() {
        [first, .., last] => Some((first.clone(), last.clone())),
        _ => None,
    }
}

/// The train's or flight's number on a line that names it.
fn number_in(line: &str, kind: &str) -> Option<String> {
    let low = line.to_ascii_lowercase();
    let at = low.find(kind)?;
    let rest = line.get(at + kind.len()..)?;
    let tokens: Vec<&str> = rest
        .split(|c: char| c.is_whitespace() || matches!(c, ':' | '/'))
        .filter(|t| !t.is_empty())
        .collect();
    if kind == "train" {
        return tokens
            .iter()
            .find(|t| t.len() == 5 && t.chars().all(|c| c.is_ascii_digit()))
            .map(|t| t.to_string());
    }
    let carrier = |t: &str| {
        t.len() == 2
            && t.chars().all(|c| c.is_ascii_alphanumeric())
            && t.chars().any(|c| c.is_ascii_uppercase())
    };
    let flight = |t: &str| (1..=4).contains(&t.len()) && t.chars().all(|c| c.is_ascii_digit());
    for (i, t) in tokens.iter().enumerate() {
        if carrier(t) && tokens.get(i + 1).is_some_and(|n| flight(n)) {
            return Some(format!("{t} {}", tokens[i + 1]));
        }
        if let Some((c, n)) = t.split_once('-')
            && carrier(c)
            && flight(n)
        {
            return Some(format!("{c} {n}"));
        }
        if t.len() > 2 && t.is_char_boundary(2) && carrier(&t[..2]) && flight(&t[2..]) {
            return Some(format!("{} {}", &t[..2], &t[2..]));
        }
    }
    None
}

/// The PNR on a line that names it.
fn pnr_in(line: &str) -> Option<String> {
    let low = line.to_ascii_lowercase();
    let at = low.find("pnr")?;
    let rest = line.get(at + 3..)?;
    rest.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .find(|t| {
            (5..=10).contains(&t.len())
                && t.chars().any(|c| c.is_ascii_digit())
                && !t.chars().any(|c| c.is_ascii_lowercase())
        })
        .map(String::from)
}

/// What a ticket's text says of the journey.
pub fn read_booking(text: &str) -> Booking {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let low = text.to_ascii_lowercase();
    let says = |w: &[&str]| w.iter().any(|x| low.contains(x));
    let kind = if says(&["irctc", "train"]) {
        "train"
    } else if says(&["flight", "airline", "boarding pass", "indigo"]) {
        "flight"
    } else if says(&["bus", "redbus"]) {
        "bus"
    } else {
        "journey"
    };
    let not_the_journey = |line: &str| {
        let l = line.to_ascii_lowercase();
        NOT_THE_JOURNEY.iter().any(|w| l.contains(w))
    };
    let names = |line: &str, words: &[&str]| {
        let l = format!("{} ", line.to_ascii_lowercase());
        words.iter().any(|w| l.contains(w))
    };
    let date = (lines.iter())
        .filter(|l| !not_the_journey(l) && names(l, &THE_JOURNEY))
        .find_map(|l| date_in(before_arrival(l)))
        .or_else(|| {
            (lines.iter())
                .filter(|l| !not_the_journey(l) && !l.to_ascii_lowercase().starts_with("arriv"))
                .find_map(|l| date_in(before_arrival(l)))
        });
    let time = (lines.iter())
        .filter(|l| !not_the_journey(l) && names(l, &THE_DEPARTURE))
        .find_map(|l| time_in(before_arrival(l)));
    let labelled = |label: &str| lines.iter().find_map(|l| after_label(l, label));
    let route = (lines.iter())
        .find_map(|l| route_in(l))
        .or_else(|| lines.iter().find_map(|l| stations_in(l)));
    Booking {
        kind,
        number: match kind {
            "train" | "flight" => lines.iter().find_map(|l| number_in(l, kind)),
            _ => None,
        },
        from: labelled("from:").or_else(|| route.as_ref().map(|r| r.0.clone())),
        to: labelled("to:").or_else(|| route.as_ref().map(|r| r.1.clone())),
        pnr: lines.iter().find_map(|l| pnr_in(l)),
        date,
        time,
    }
}

/// "Train 12004 NDLS → LKO".
fn journey_name(b: &Booking) -> String {
    let mut word = String::new();
    let mut letters = b.kind.chars();
    if let Some(first) = letters.next() {
        word.extend(first.to_uppercase());
        word.push_str(letters.as_str());
    }
    let mut name = word;
    if let Some(n) = &b.number {
        name.push(' ');
        name.push_str(n);
    }
    if let (Some(from), Some(to)) = (&b.from, &b.to) {
        name.push_str(&format!(" {from} → {to}"));
    }
    name
}

/// "15 Nov 2026".
fn long(d: Date, with_year: bool) -> String {
    let month = MONTHS[d.m as usize - 1];
    let mut letters = month.chars();
    let first = letters
        .next()
        .map_or(String::new(), |c| c.to_uppercase().collect());
    if with_year {
        format!("{} {first}{} {}", d.d, letters.as_str(), d.y)
    } else {
        format!("{} {first}{}", d.d, letters.as_str())
    }
}

/// When the reminder comes and what it says; None without a date, or once its time has passed.
pub fn reminder(booking: &Booking, today: Date, minute: u32) -> Option<(Date, String, String)> {
    let date = Date::parse(booking.date.as_deref()?)?;
    let name = journey_name(booking);
    let Some(time) = booking.time.as_deref() else {
        return (date > today || (date == today && minute < crate::times::minutes(THAT_MORNING)?))
            .then(|| (date, THAT_MORNING.to_string(), format!("{name} today")));
    };
    let leaves = crate::times::minutes(time)?;
    let title = format!("{name} at {time} on {}", long(date, false));
    let (mut day, mut at) = if leaves >= BEFORE {
        (date, leaves - BEFORE)
    } else {
        (date.add_days(-1), leaves + 24 * 60 - BEFORE)
    };
    let mut hhmm = format!("{:02}:{:02}", at / 60, at % 60);
    if at < NOT_BEFORE {
        day = day.add_days(-1);
        hhmm = EVENING_BEFORE.to_string();
        at = crate::times::minutes(EVENING_BEFORE)?;
    }
    (day > today || (day == today && at > minute)).then_some((day, hhmm, title))
}

/// `text` as letters, digits and single dashes, lowercase.
fn slug(text: &str) -> String {
    let low = text.to_lowercase();
    let parts: Vec<&str> = low
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    parts.join("-")
}

/// The file's name under the notes folder's tickets folder.
pub fn file_name(booking: &Booking, original: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let stem = match (&booking.date, &booking.number) {
        (Some(date), Some(number)) => format!("{date}-{}-{}", booking.kind, slug(number)),
        (Some(date), None) => format!("{date}-{}", booking.kind),
        _ => {
            let name = Path::new(original)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let cleaned = slug(&name);
            if cleaned.is_empty() {
                "ticket".to_string()
            } else {
                cleaned
            }
        }
    };
    let first = format!("{stem}.pdf");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{stem}-{n}.pdf"))
        .find(|name| !taken(name))
        .unwrap_or(first)
}

/// The path as typed, made whole.
fn whole_path(typed: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let file = match typed.strip_prefix("~/") {
        Some(rest) => home
            .ok_or("no home folder here: type the full path")?
            .join(rest),
        None => PathBuf::from(typed),
    };
    if !file.is_absolute() {
        return Err("The ticket's full path, please: ~/Downloads/ticket.pdf".into());
    }
    Ok(file)
}

/// Files the ticket at `path` after a yes; what to say.
#[allow(clippy::too_many_arguments)]
pub fn file(
    path: &str,
    home: Option<&Path>,
    notes: &Path,
    memory: &mut Memory,
    today: Date,
    minute: u32,
    now: i64,
    read: &dyn Fn(&Path) -> Result<String, String>,
    confirm: &mut dyn FnMut(&str) -> bool,
) -> Vec<String> {
    let source = match whole_path(path, home) {
        Ok(p) => p,
        Err(why) => return vec![why],
    };
    if !source.is_file() {
        return vec![format!("There is no file at {path}.")];
    }
    let booking = read(&source)
        .map(|text| read_booking(&text))
        .unwrap_or(Booking {
            kind: "journey",
            ..Booking::default()
        });
    let folder = notes.join(TICKETS);
    let original = source
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let name = file_name(&booking, &original, &|n| folder.join(n).exists());
    let due = reminder(&booking, today, minute);
    let readable = booking.date.is_some();
    let question = if readable {
        let date = booking
            .date
            .as_deref()
            .and_then(Date::parse)
            .map_or(String::new(), |d| long(d, true));
        let mut lines = vec![format!("File this ticket? {}", journey_name(&booking))];
        let leaves = booking
            .time
            .as_deref()
            .map_or(String::new(), |t| format!(" at {t}"));
        lines.push(format!("Leaves {date}{leaves}"));
        if let Some(pnr) = &booking.pnr {
            lines.push(format!("PNR {pnr}"));
        }
        lines.push(format!("As {TICKETS}/{name} in your notes"));
        lines.push(match &due {
            Some((day, at, _)) => format!("Reminder {} at {at}", long(*day, true)),
            None => "No reminder: that time has passed".to_string(),
        });
        lines.join("\n")
    } else {
        format!(
            "I could not read a journey from this PDF. File it anyway as {TICKETS}/{name} in your notes, with no reminder?"
        )
    };
    if !confirm(&question) {
        return vec!["Not filed.".into()];
    }
    if let Err(e) =
        std::fs::create_dir_all(&folder).and_then(|()| std::fs::copy(&source, folder.join(&name)))
    {
        return vec![format!("I could not file it: {e}")];
    }
    let filed = format!("Filed as {TICKETS}/{name} in your notes.");
    let Some((day, at, title)) = due else {
        return vec![format!("{filed} There is no reminder.")];
    };
    let set = memory
        .create_task(&title, Some(&day.to_string()), now)
        .and_then(|id| memory.set_task_time(id, Some(&at)));
    match set {
        Ok(_) => vec![
            filed,
            format!("I'll remind you {} at {at}.", long(day, true)),
        ],
        Err(e) => vec![filed, format!("But I could not set the reminder: {e}")],
    }
}

/// The folder tickets go in, under the notes folder.
const TICKETS: &str = "tickets";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_to_file_a_ticket_is_told_from_the_rest() {
        for (text, want) in [
            (
                "file this ticket ~/Downloads/irctc.pdf",
                "~/Downloads/irctc.pdf",
            ),
            ("file my ticket /home/me/a b.pdf", "/home/me/a b.pdf"),
            ("file ticket '/home/me/x.pdf'", "/home/me/x.pdf"),
            ("File this ticket \"/home/me/X.PDF\".", "/home/me/X.PDF"),
            ("ye ticket file karo ~/Downloads/x.pdf", "~/Downloads/x.pdf"),
            ("ticket file kar do ~/x.pdf", "~/x.pdf"),
            ("~/x.pdf ticket file karo", "~/x.pdf"),
            ("save this booking ~/x.pdf", "~/x.pdf"),
            ("mera ticket rakh lo ~/x.pdf", "~/x.pdf"),
            ("add this ticket to my notes ~/x.pdf", "~/x.pdf"),
        ] {
            assert_eq!(parse(text).as_deref(), Some(want), "{text}");
        }
        for text in [
            "file this ticket",
            "what is my ticket",
            "file ~/x.pdf",
            "file this ticket ~/x.txt",
            "summarize ticket ~/x.pdf",
            "hello",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    const TRAIN: &str = "IRCTC Electronic Reservation Slip\n\
PNR No: 2134567890   Train No./Name: 12004/LKO SHATABDI\n\
Class: CHAIR CAR (CC)  Quota: GENERAL\n\
From: NEW DELHI (NDLS)  To: LUCKNOW NR (LKO)\n\
Boarding At: NEW DELHI (NDLS)\n\
Date of Journey: 15-Nov-2026\n\
Departure* 06:10 15-Nov-2026   Arrival* 12:40 15-Nov-2026\n\
Booked on: 01-Nov-2026 21:14:22 HRS\n\
Transaction ID: 100001234567\n";

    const FLIGHT: &str = "IndiGo E-Ticket / Itinerary\n\
Booking Reference (PNR): X7K2LM\n\
Flight 6E 2341\n\
DEL → BOM\n\
Departure: 15 Nov 2026, 18:45 hrs\n\
Arrival: 15 Nov 2026, 21:00 hrs\n\
Issued on 02 Nov 2026\n";

    const BUS: &str = "RedBus ticket\n\
Ticket No: TS123456\n\
From: Kanpur To: Lucknow\n\
Date of Journey: 16/11/2026\n\
Boarding time: 9:30 PM\n";

    fn some(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn a_train_ticket_is_read_with_the_journey_s_date_not_the_booking_s() {
        let b = read_booking(TRAIN);
        assert_eq!(b.kind, "train");
        assert_eq!(b.number, some("12004"));
        assert_eq!(b.from, some("NEW DELHI (NDLS)"));
        assert_eq!(b.to, some("LUCKNOW NR (LKO)"));
        assert_eq!(b.pnr, some("2134567890"));
        assert_eq!(b.date, some("2026-11-15"));
        assert_eq!(b.time, some("06:10"), "the departure, not the arrival");
    }

    #[test]
    fn a_ticket_laid_out_as_a_table_is_read_as_the_pdf_gives_it() {
        // experiments/v4/tickets/train.html, as `kestrel pdf-text` gives it.
        let text = "Electronic Reservation Slip (ERS) - Made up for a test, not a real booking\n\n\
IRCTC e-ticketing style.  This is a synthetic ticket.\n\n\
Booked From Boarding At To\n\n\
NEW DELHI (NDLS) NEW DELHI (NDLS) LUCKNOW NR (LKO)\n\n\
Departure* 06:10 15-Nov-2026 Arrival* 12:40 15-Nov-2026\n\n\
PNR No: 2134567890 Train No./Name: 12004/LKO SHA TABDI Class: CHAIR CAR (CC)\n\n\
Quota: GENERAL Distance: 512 KM Booking Date: 01-Nov-2026 21:14:22 HRS\n\n\
Transaction ID: 100001234567";
        let b = read_booking(text);
        assert_eq!(b.kind, "train");
        assert_eq!(b.number, some("12004"));
        assert_eq!(b.from, some("NEW DELHI (NDLS)"));
        assert_eq!(b.to, some("LUCKNOW NR (LKO)"));
        assert_eq!(b.pnr, some("2134567890"));
        assert_eq!(b.date, some("2026-11-15"));
        assert_eq!(b.time, some("06:10"));
    }

    #[test]
    fn a_flight_ticket_is_read_with_its_route() {
        let b = read_booking(FLIGHT);
        assert_eq!(b.kind, "flight");
        assert_eq!(b.number, some("6E 2341"));
        assert_eq!((b.from, b.to), (some("DEL"), some("BOM")));
        assert_eq!(b.pnr, some("X7K2LM"));
        assert_eq!(b.date, some("2026-11-15"));
        assert_eq!(b.time, some("18:45"));
    }

    #[test]
    fn a_bus_ticket_is_read_with_a_twelve_hour_time() {
        let b = read_booking(BUS);
        assert_eq!(b.kind, "bus");
        assert_eq!((b.from, b.to), (some("Kanpur"), some("Lucknow")));
        assert_eq!(b.date, some("2026-11-16"));
        assert_eq!(b.time, some("21:30"));
    }

    #[test]
    fn dates_and_times_come_in_the_forms_tickets_use() {
        for (line, want) in [
            ("Departure 15-Nov-2026", "2026-11-15"),
            ("Departure 15 November 2026", "2026-11-15"),
            ("Departure Nov 15, 2026", "2026-11-15"),
            ("Departure 15/11/2026", "2026-11-15"),
            ("Departure 15-11-2026", "2026-11-15"),
            ("Departure 15.11.2026", "2026-11-15"),
            ("Departure 2026-11-15", "2026-11-15"),
            ("Departure 15-Nov-26", "2026-11-15"),
        ] {
            assert_eq!(read_booking(line).date.as_deref(), Some(want), "{line}");
        }
        for (line, want) in [
            ("Departure 06:10", "06:10"),
            ("Departure 6:05 pm", "18:05"),
            ("Departure 12:30 AM", "00:30"),
            ("Departure 18:45 hrs", "18:45"),
            ("Departure 0605 hrs", "06:05"),
        ] {
            assert_eq!(read_booking(line).time.as_deref(), Some(want), "{line}");
        }
        // A time is not taken for a date, nor a date that cannot be one.
        assert_eq!(read_booking("Departure 06:10 11:30").date, None);
        assert_eq!(read_booking("Departure 45-Nov-2026").date, None);
        assert_eq!(read_booking("Departure 31/02/2026").date, None);
    }

    #[test]
    fn a_page_with_only_the_booking_s_date_has_no_journey_date() {
        let b =
            read_booking("Booked on: 01-Nov-2026\nTransaction date 01-Nov-2026\nIssued 01/11/2026");
        assert_eq!(b.date, None);
        assert_eq!(b.kind, "journey");
    }

    fn today() -> Date {
        Date::parse("2026-11-04").unwrap()
    }

    fn booking(date: &str, time: Option<&str>) -> Booking {
        Booking {
            kind: "train",
            number: some("12004"),
            from: some("NDLS"),
            to: some("LKO"),
            pnr: some("2134567890"),
            date: some(date),
            time: time.map(String::from),
        }
    }

    #[test]
    fn the_reminder_comes_three_hours_before_but_not_in_the_night() {
        let at = |date: &str, time: &str| {
            let (d, at, _) = reminder(&booking(date, Some(time)), today(), 600).unwrap();
            (d.to_string(), at)
        };
        assert_eq!(
            at("2026-11-15", "18:45"),
            ("2026-11-15".to_string(), "15:45".to_string())
        );
        assert_eq!(
            at("2026-11-15", "10:00"),
            ("2026-11-15".to_string(), "07:00".to_string())
        );
        // Before 07:00 it would wake them: the evening before instead.
        assert_eq!(
            at("2026-11-15", "06:10"),
            ("2026-11-14".to_string(), "21:00".to_string())
        );
        assert_eq!(
            at("2026-11-15", "09:59"),
            ("2026-11-14".to_string(), "21:00".to_string())
        );
        // Across midnight, and late enough to stay.
        assert_eq!(
            at("2026-11-15", "01:30"),
            ("2026-11-14".to_string(), "22:30".to_string())
        );
        let (_, _, title) = reminder(&booking("2026-11-15", Some("18:45")), today(), 600).unwrap();
        assert_eq!(title, "Train 12004 NDLS → LKO at 18:45 on 15 Nov");
        assert!(
            !title.contains("2134567890"),
            "the PNR stays out of a notification"
        );
    }

    #[test]
    fn no_time_means_that_morning_and_no_date_or_a_past_journey_no_reminder() {
        let (date, at, title) = reminder(&booking("2026-11-15", None), today(), 600).unwrap();
        assert_eq!(
            (date.to_string().as_str(), at.as_str()),
            ("2026-11-15", "07:00")
        );
        assert_eq!(title, "Train 12004 NDLS → LKO today");
        assert_eq!(
            reminder(
                &Booking {
                    date: None,
                    ..booking("x", None)
                },
                today(),
                600
            ),
            None
        );
        assert_eq!(
            reminder(&booking("2026-11-01", Some("10:00")), today(), 600),
            None
        );
        assert_eq!(
            reminder(&booking("2026-11-04", Some("11:00")), today(), 10 * 60),
            None,
            "its 3 hours began"
        );
        assert!(reminder(&booking("2026-11-04", Some("14:00")), today(), 10 * 60).is_some());
    }

    #[test]
    fn a_filed_name_says_the_journey_and_never_overwrites() {
        let b = booking("2026-11-15", Some("06:10"));
        assert_eq!(
            file_name(&b, "irctc.pdf", &|_| false),
            "2026-11-15-train-12004.pdf"
        );
        assert_eq!(
            file_name(&b, "irctc.pdf", &|n| n == "2026-11-15-train-12004.pdf"),
            "2026-11-15-train-12004-2.pdf"
        );
        let none = Booking {
            kind: "journey",
            ..Booking::default()
        };
        assert_eq!(
            file_name(&none, "My Ticket (1).PDF", &|_| false),
            "my-ticket-1.pdf"
        );
        let odd = Booking {
            number: some("../../evil"),
            ..b
        };
        let name = file_name(&odd, "x.pdf", &|_| false);
        assert!(!name.contains('/') && !name.contains(".."), "{name}");
    }

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_ticket_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("notes")).unwrap();
        d
    }

    fn pdf(d: &Path) -> String {
        let p = d.join("irctc.pdf");
        std::fs::write(&p, b"%PDF-1.4 the ticket").unwrap();
        p.to_string_lossy().into_owned()
    }

    fn go(
        d: &Path,
        path: &str,
        m: &mut Memory,
        text: Result<&'static str, &'static str>,
        confirm: &mut dyn FnMut(&str) -> bool,
    ) -> Vec<String> {
        let read = move |_: &Path| text.map(String::from).map_err(String::from);
        file(
            path,
            Some(d),
            &d.join("notes"),
            m,
            today(),
            600,
            5,
            &read,
            confirm,
        )
    }

    #[test]
    fn a_yes_after_what_was_read_files_it_and_sets_the_reminder() {
        let d = folder("yes");
        let mut m = Memory::in_memory().unwrap();
        let path = pdf(&d);
        let mut shown = String::new();
        let said = go(&d, &path, &mut m, Ok(TRAIN), &mut |q| {
            shown = q.to_string();
            true
        });
        for part in [
            "Train 12004",
            "NEW DELHI (NDLS)",
            "LUCKNOW NR (LKO)",
            "15 Nov 2026",
            "06:10",
            "2134567890",
            "tickets/2026-11-15-train-12004.pdf",
            "14 Nov 2026 at 21:00",
        ] {
            assert!(shown.contains(part), "{part}: {shown}");
        }
        let filed = d.join("notes/tickets/2026-11-15-train-12004.pdf");
        assert_eq!(std::fs::read(&filed).unwrap(), b"%PDF-1.4 the ticket");
        assert!(Path::new(&path).exists(), "the original stays");
        let tasks = m.tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].title,
            "Train 12004 NEW DELHI (NDLS) → LUCKNOW NR (LKO) at 06:10 on 15 Nov"
        );
        assert_eq!(
            tasks[0].due.as_deref(),
            Some("2026-11-14"),
            "{:?}",
            tasks[0]
        );
        assert_eq!(tasks[0].due_time.as_deref(), Some("21:00"));
        assert!(said.iter().any(|l| l.contains("Filed")), "{said:?}");
        assert!(said.iter().any(|l| l.contains("remind you")), "{said:?}");
    }

    #[test]
    fn a_no_changes_nothing() {
        let d = folder("no");
        let mut m = Memory::in_memory().unwrap();
        let path = pdf(&d);
        let said = go(&d, &path, &mut m, Ok(TRAIN), &mut |_| false);
        assert_eq!(said, ["Not filed."]);
        assert!(!d.join("notes/tickets").exists());
        assert!(m.tasks().unwrap().is_empty());
    }

    #[test]
    fn a_pdf_with_no_journey_can_be_filed_with_no_reminder_and_says_so() {
        let d = folder("scan");
        let mut m = Memory::in_memory().unwrap();
        let path = pdf(&d);
        for text in [Ok("Nothing useful here."), Err("the PDF could not be read")] {
            let mut shown = String::new();
            let said = go(&d, &path, &mut m, text, &mut |q| {
                shown = q.to_string();
                false
            });
            assert!(shown.contains("could not read a journey"), "{shown}");
            assert_eq!(said, ["Not filed."]);
        }
        let said = go(&d, &path, &mut m, Ok("Nothing useful here."), &mut |_| true);
        assert!(
            said[0].contains("Filed") && said[0].contains("no reminder"),
            "{said:?}"
        );
        assert!(d.join("notes/tickets/irctc.pdf").exists());
        assert!(m.tasks().unwrap().is_empty());
    }

    #[test]
    fn a_past_journey_is_filed_with_no_reminder() {
        let d = folder("past");
        let mut m = Memory::in_memory().unwrap();
        let path = pdf(&d);
        let said = go(
            &d,
            &path,
            &mut m,
            Ok("Train 12004\nDeparture 06:10 01-Nov-2026"),
            &mut |_| true,
        );
        assert!(said.join(" ").contains("no reminder"), "{said:?}");
        assert!(m.tasks().unwrap().is_empty());
    }

    #[test]
    fn a_bad_path_asks_nothing() {
        let d = folder("bad");
        let mut m = Memory::in_memory().unwrap();
        let mut never = |q: &str| -> bool { panic!("asked: {q}") };
        for (path, want) in [("~/nowhere.pdf", "no file"), ("relative.pdf", "full path")] {
            let said = go(&d, path, &mut m, Ok(TRAIN), &mut never);
            assert!(said[0].contains(want), "{want}: {said:?}");
        }
        std::fs::create_dir_all(d.join("dir.pdf")).unwrap();
        let said = go(
            &d,
            &d.join("dir.pdf").to_string_lossy(),
            &mut m,
            Ok(TRAIN),
            &mut never,
        );
        assert!(said[0].contains("no file"), "{said:?}");
    }

    #[test]
    fn a_home_path_is_expanded() {
        let d = folder("home");
        let mut m = Memory::in_memory().unwrap();
        pdf(&d);
        let said = go(&d, "~/irctc.pdf", &mut m, Ok(TRAIN), &mut |_| true);
        assert!(said.iter().any(|l| l.contains("Filed")), "{said:?}");
    }
}
