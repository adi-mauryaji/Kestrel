//! The morning brief and the weather reminder (v4 D): the day's tasks, documents about to expire, and,
//! with the internet on, the weather at the person's place and the headlines of their feeds. On asking
//! ("morning brief") and, once a day at the time they chose, as a notification. It is off until a time
//! is chosen, and what it asks of the internet goes to the log like any request.

use std::error::Error;
use std::path::Path;

use kestrel_memory::Memory;
use kestrel_tools::date::Date;
use serde_json::Value;

use crate::remind::Notify;
use crate::send::Link;

/// A brief missed by more than this, in minutes, waits for tomorrow: it is a morning's, not a day's.
pub const WINDOW: u32 = 240;
/// Rain this likely, in percent, calls for a reminder.
const RAIN_LIKELY: i64 = 60;
const VERY_HOT: f64 = 40.0;
const VERY_COLD: f64 = 5.0;
/// A document this close to expiry is in the brief.
const DOCUMENT_DAYS: i64 = 30;
/// Headlines in a brief, from each feed.
const HEADLINES: usize = 3;
/// How often the thread looks at the clock.
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(30);
/// What the words of a request for the brief may be.
const ONLY_THE_BRIEF: [&str; 11] = [
    "morning", "daily", "brief", "briefing", "today's", "todays", "aaj", "ka", "my", "me", "do",
];

/// Does `text` ask for the brief?
pub fn parse(text: &str) -> bool {
    let said = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim()
        .to_lowercase();
    let words: Vec<&str> = said.split_whitespace().collect();
    words.len() >= 2
        && words.iter().any(|w| *w == "brief" || *w == "briefing")
        && words.iter().all(|w| ONLY_THE_BRIEF.contains(w))
}

/// What today's forecast calls for, if anything: the weather reminder.
pub fn alert(forecast: &Value) -> Option<String> {
    let today = &forecast["today"];
    if matches!(today["code"].as_i64(), Some(95 | 96 | 99)) {
        return Some("Thunderstorms today: stay indoors if you can.".into());
    }
    if let Some(rain) = today["rain"].as_i64().filter(|r| *r >= RAIN_LIKELY) {
        return Some(format!("Rain likely today ({rain}%): take an umbrella."));
    }
    if let Some(max) = today["max"].as_f64().filter(|t| *t >= VERY_HOT) {
        return Some(format!(
            "Very hot today ({}°C): drink water and avoid the noon sun.",
            max.round()
        ));
    }
    if let Some(min) = today["min"].as_f64().filter(|t| *t <= VERY_COLD) {
        return Some(format!("Very cold today ({}°C): dress warm.", min.round()));
    }
    None
}

/// Is the brief due at `minute`, set for `at`, if it has not been given today?
pub fn due(minute: u32, at: u32, given_today: bool) -> bool {
    !given_today && minute >= at && minute < at + WINDOW
}

/// The brief: its lines, and the weather reminder if the day calls for one.
pub struct Brief {
    pub lines: Vec<String>,
    pub alert: Option<String>,
}

/// A task as the brief says it.
fn said(t: &kestrel_memory::TaskRow) -> String {
    match &t.due_time {
        Some(at) => format!("{} at {at}", t.title),
        None => t.title.clone(),
    }
}

/// Puts the brief together. With the internet off, or no place and no feed, it asks nothing.
pub fn compose(
    memory: &mut Memory,
    link: &dyn Link,
    data: Option<&Path>,
    today: Date,
    now: i64,
) -> Result<Brief, Box<dyn Error>> {
    let settings = data
        .and_then(|d| crate::settings::load(&crate::settings::path(d)).ok())
        .unwrap_or_default();
    let tasks = memory.tasks()?;
    let on = |pick: &dyn Fn(Date) -> bool| -> Vec<String> {
        let mut found: Vec<&kestrel_memory::TaskRow> = (tasks.iter())
            .filter(|t| t.due.as_deref().and_then(Date::parse).is_some_and(pick))
            .collect();
        found.sort_by_key(|t| (t.due_time.is_some(), t.due_time.clone(), t.id));
        found.iter().map(|t| said(t)).collect()
    };
    let due_today = on(&|d| d == today);
    let overdue = on(&|d| d < today);
    let mut lines = Vec::new();
    if !due_today.is_empty() {
        lines.push(format!("Today: {}.", due_today.join("; ")));
    }
    if !overdue.is_empty() {
        lines.push(format!("Overdue: {}.", overdue.join("; ")));
    }
    for r in memory.documents()? {
        let Some(d) = r.expires_on.as_deref().and_then(Date::parse) else {
            continue;
        };
        let left = d.days() - today.days();
        match left {
            ..0 => lines.push(format!("Your {} has expired.", r.kind)),
            0 => lines.push(format!("Your {} expires today.", r.kind)),
            1 => lines.push(format!("Your {} expires tomorrow.", r.kind)),
            2..=DOCUMENT_DAYS => lines.push(format!("Your {} expires in {left} days.", r.kind)),
            _ => {}
        }
    }
    if lines.is_empty() {
        lines.push("Nothing is due today.".to_string());
    }
    let mut alert_now = None;
    if settings.internet {
        if let Some(place) = &settings.place {
            match crate::online::forecast(link, memory, now, place) {
                Ok(f) => {
                    lines.push(crate::online::weather_line(
                        &f,
                        crate::online::Day::Today,
                        &place.name,
                    ));
                    alert_now = alert(&f);
                }
                Err(_) => lines.push("The weather could not be read.".to_string()),
            }
        }
        lines.extend(crate::online::news_up_to(
            link,
            memory,
            now,
            &settings.feeds,
            HEADLINES,
        ));
    }
    Ok(Brief {
        lines,
        alert: alert_now,
    })
}

/// The service's check: gives the brief once a day from its time on, and the weather reminder with it.
/// Marked before anything is asked, so a failed notification does not ask the internet each minute.
pub fn ring(
    memory: &mut Memory,
    link: &dyn Link,
    data: Option<&Path>,
    today: Date,
    minute: u32,
    now: i64,
    send: &mut Notify,
) -> Result<bool, Box<dyn Error>> {
    let settings = data
        .and_then(|d| crate::settings::load(&crate::settings::path(d)).ok())
        .unwrap_or_default();
    let Some(at) = settings.brief_at.as_deref().and_then(crate::times::minutes) else {
        return Ok(false);
    };
    let day = today.to_string();
    if !due(minute, at, memory.brief_given(&day)?) {
        return Ok(false);
    }
    memory.mark_brief(&day, now)?;
    let brief = compose(memory, link, data, today, now)?;
    let title = match &settings.name {
        Some(name) => format!("Good morning, {name}"),
        None => "Good morning".to_string(),
    };
    send(&title, &brief.lines.join("\n"))?;
    memory.count_use(now, "brief_rang");
    if let Some(reminder) = &brief.alert {
        send("The weather today", reminder)?;
        memory.count_use(now, "weather_reminder_rang");
    }
    Ok(true)
}

/// The service's brief thread: its own connection and way out, so a slow network holds up no reminder.
pub fn ring_forever(
    db: &Path,
    events: &crate::events::Shared,
    desktop: bool,
    data: Option<std::path::PathBuf>,
) {
    let mut memory = match crate::memory::open(db) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("the morning brief is off: {e}");
            return;
        }
    };
    let net = match crate::net::Net::beside_this_program() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("the morning brief is off: {e}");
            return;
        }
    };
    let offset = crate::clock::utc_offset();
    loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let today = crate::clock::local_date(now, offset);
        let minute = crate::clock::local_minute(now, offset);
        let mut to = |title: &str, body: &str| {
            let mut show = crate::remind::send;
            crate::remind::deliver(events, desktop.then_some(&mut show), now, title, body)
        };
        if let Err(e) = ring(
            &mut memory,
            &net,
            data.as_deref(),
            today,
            minute,
            now,
            &mut to,
        ) {
            eprintln!("the morning brief could not be shown: {e}");
        }
        std::thread::sleep(CHECK_EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::Place;
    use crate::online::tests::Answers;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn asking_for_the_brief_is_told_from_the_rest() {
        for text in [
            "morning brief",
            "Morning briefing?",
            "today's brief",
            "daily brief",
            "aaj ka brief",
            "brief me",
            "brief do",
            "my brief",
        ] {
            assert!(parse(text), "{text}");
        }
        for text in [
            "write a brief about the budget",
            "the brief was late",
            "morning",
            "good morning",
            "brief",
            "add task read the brief",
        ] {
            assert!(!parse(text), "{text}");
        }
    }

    fn day(code: i64, max: f64, min: f64, rain: i64) -> Value {
        json!({"today": {"code": code, "max": max, "min": min, "rain": rain}})
    }

    #[test]
    fn the_weather_reminder_comes_only_when_the_day_calls_for_one() {
        assert_eq!(alert(&day(0, 30.0, 18.0, 0)), None);
        assert_eq!(alert(&day(3, 30.0, 18.0, 59)), None);
        assert_eq!(
            alert(&day(63, 30.0, 18.0, 80)).unwrap(),
            "Rain likely today (80%): take an umbrella."
        );
        assert_eq!(
            alert(&day(95, 30.0, 18.0, 90)).unwrap(),
            "Thunderstorms today: stay indoors if you can."
        );
        assert_eq!(
            alert(&day(0, 41.4, 28.0, 0)).unwrap(),
            "Very hot today (41°C): drink water and avoid the noon sun."
        );
        assert_eq!(
            alert(&day(0, 15.0, 3.2, 0)).unwrap(),
            "Very cold today (3°C): dress warm."
        );
        assert_eq!(alert(&json!({})), None);
    }

    #[test]
    fn the_brief_is_due_once_from_its_time_for_a_morning() {
        const EIGHT: u32 = 8 * 60;
        assert!(!due(EIGHT - 1, EIGHT, false));
        assert!(due(EIGHT, EIGHT, false));
        assert!(due(EIGHT + WINDOW - 1, EIGHT, false));
        assert!(!due(EIGHT + WINDOW, EIGHT, false), "missed: tomorrow");
        assert!(!due(EIGHT + 5, EIGHT, true), "given");
    }

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_brief_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn settings(d: &Path, change: impl Fn(&mut crate::settings::Settings)) {
        let mut s = crate::settings::load(&crate::settings::path(d)).unwrap();
        change(&mut s);
        crate::settings::save(&crate::settings::path(d), &s).unwrap();
    }

    fn today() -> Date {
        Date::parse("2026-11-04").unwrap()
    }

    fn memory() -> Memory {
        let mut m = Memory::in_memory().unwrap();
        m.create_task("call Asha", Some("2026-11-04"), 1).unwrap();
        let later = m
            .create_task("pay the bill", Some("2026-11-04"), 1)
            .unwrap();
        m.set_task_time(later, Some("17:00")).unwrap();
        m.create_task("old thing", Some("2026-11-01"), 1).unwrap();
        m.create_task("next week", Some("2026-11-11"), 1).unwrap();
        m.create_task("no date", None, 1).unwrap();
        m
    }

    fn documents(m: &mut Memory, found: &[(&str, &str, &str)]) {
        let rows: Vec<kestrel_memory::documents::FoundDocument> = found
            .iter()
            .map(|(path, kind, _)| kestrel_memory::documents::FoundDocument {
                path: path.to_string(),
                kind: kind.to_string(),
                sha256: "h".into(),
                found_on: None,
            })
            .collect();
        m.sync_documents(&rows).unwrap();
        for (path, _, date) in found {
            m.answer_document(path, Some(date)).unwrap();
        }
    }

    fn delhi() -> Place {
        Place {
            name: "Delhi".into(),
            region: "India".into(),
            lat: "28.65".into(),
            lon: "77.23".into(),
        }
    }

    #[test]
    fn with_the_internet_off_the_brief_is_tasks_and_documents_and_asks_nothing() {
        let d = folder("off");
        settings(&d, |s| {
            s.place = Some(delhi());
            s.feeds = vec!["https://a.example/rss".into()];
        });
        let (mut m, link) = (memory(), Answers::default());
        documents(
            &mut m,
            &[
                ("pan.pdf", "PAN card", "2026-11-16"),
                ("old.pdf", "policy", "2026-10-30"),
                ("far.pdf", "passport", "2030-01-01"),
            ],
        );
        let b = compose(&mut m, &link, Some(&d), today(), 5).unwrap();
        let text = b.lines.join("\n");
        assert!(
            text.contains("Today: call Asha; pay the bill at 17:00."),
            "{text}"
        );
        assert!(text.contains("Overdue: old thing."), "{text}");
        assert!(
            !text.contains("next week") && !text.contains("no date"),
            "{text}"
        );
        assert!(text.contains("Your PAN card expires in 12 days."), "{text}");
        assert!(text.contains("Your policy has expired."), "{text}");
        assert!(!text.contains("passport"), "{text}");
        assert!(!text.contains("Delhi"), "{text}");
        assert_eq!(b.alert, None);
        assert!(link.asked.borrow().is_empty());
        assert!(m.outbound(9).unwrap().is_empty());
    }

    #[test]
    fn a_quiet_day_says_so() {
        let d = folder("quiet");
        let (mut m, link) = (Memory::in_memory().unwrap(), Answers::default());
        let b = compose(&mut m, &link, Some(&d), today(), 5).unwrap();
        assert_eq!(b.lines, ["Nothing is due today."]);
    }

    #[test]
    fn with_the_internet_on_it_adds_the_weather_and_headlines_each_in_the_log() {
        let d = folder("on");
        settings(&d, |s| {
            s.internet = true;
            s.place = Some(delhi());
            s.feeds = vec!["https://a.example/rss".into()];
        });
        let (mut m, mut link) = (memory(), Answers::default());
        let items: Vec<Value> = (0..9)
            .map(|i| json!({"title": format!("Headline {i}")}))
            .collect();
        link.feeds = vec![(
            "https://a.example/rss",
            Ok(json!({"title": "Feed", "items": items})),
        )];
        let b = compose(&mut m, &link, Some(&d), today(), 5).unwrap();
        let text = b.lines.join("\n");
        assert!(text.contains("Delhi today: rain"), "{text}");
        assert!(
            text.contains("Headline 2") && !text.contains("Headline 3"),
            "three in a brief: {text}"
        );
        assert!(text.contains("(from a.example)"), "{text}");
        assert_eq!(
            b.alert.as_deref(),
            Some("Rain likely today (80%): take an umbrella.")
        );
        let log = m.outbound(9).unwrap();
        assert_eq!(
            log.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(),
            ["rss", "weather"]
        );
    }

    #[test]
    fn it_rings_once_a_day_from_its_time_and_the_weather_reminder_with_it() {
        let d = folder("ring");
        settings(&d, |s| {
            s.internet = true;
            s.place = Some(delhi());
            s.brief_at = Some("08:00".into());
            s.name = Some("Meera".into());
        });
        let (mut m, link) = (memory(), Answers::default());
        let mut shown: Vec<(String, String)> = Vec::new();
        let mut send = |t: &str, b: &str| -> Result<(), Box<dyn Error>> {
            shown.push((t.to_string(), b.to_string()));
            Ok(())
        };
        assert!(!ring(&mut m, &link, Some(&d), today(), 7 * 60, 5, &mut send).unwrap());
        assert!(ring(&mut m, &link, Some(&d), today(), 8 * 60, 5, &mut send).unwrap());
        assert!(!ring(&mut m, &link, Some(&d), today(), 8 * 60 + 1, 5, &mut send).unwrap());
        let tomorrow = Date::parse("2026-11-05").unwrap();
        assert!(ring(&mut m, &link, Some(&d), tomorrow, 8 * 60, 5, &mut send).unwrap());
        assert_eq!(
            shown.len(),
            4,
            "the brief and the reminder, each day: {shown:?}"
        );
        assert_eq!(shown[0].0, "Good morning, Meera");
        assert!(shown[0].1.contains("call Asha"), "{shown:?}");
        assert_eq!(shown[1].0, "The weather today");
        assert!(shown[1].1.contains("umbrella"), "{shown:?}");
        assert_eq!(m.outbound(9).unwrap().len(), 2, "one forecast a day");
        let rows = m.usage().unwrap();
        let n = |what: &str| rows.iter().find(|r| r.what == what).map(|r| r.n);
        assert_eq!(n("brief_rang"), Some(2));
        assert_eq!(n("weather_reminder_rang"), Some(2));
    }

    #[test]
    fn with_no_time_chosen_it_never_rings_and_a_failed_notification_asks_no_more() {
        let d = folder("no_time");
        settings(&d, |s| {
            s.internet = true;
            s.place = Some(delhi());
        });
        let (mut m, link) = (memory(), Answers::default());
        let mut never = |_: &str, _: &str| -> Result<(), Box<dyn Error>> { panic!("rang") };
        assert!(!ring(&mut m, &link, Some(&d), today(), 8 * 60, 5, &mut never).unwrap());
        assert!(link.asked.borrow().is_empty());
        settings(&d, |s| s.brief_at = Some("08:00".into()));
        let mut fails =
            |_: &str, _: &str| -> Result<(), Box<dyn Error>> { Err("no notifier".into()) };
        assert!(ring(&mut m, &link, Some(&d), today(), 8 * 60, 5, &mut fails).is_err());
        assert!(!ring(&mut m, &link, Some(&d), today(), 8 * 60 + 1, 5, &mut fails).unwrap());
        assert_eq!(link.asked.borrow().len(), 1, "asked once");
    }
}
