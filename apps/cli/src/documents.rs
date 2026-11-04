//! Documents (v4 B, blueprint §4.6): "mera PAN kahan hai", "passport kab expire hoga". The kind of a
//! file in the notes folder and its expiry date are found by rules; a date is kept only after the
//! user says yes, and then rings 30 and 7 days before. No document text or ID number is stored, and
//! what is asked here goes to rules, not to the model. Requests are told apart by rules until
//! Kinds-v3, whose data waits for the pilot's real requests.

use kestrel_core::KestrelError;
use kestrel_memory::{DocumentRow, FoundDocument, Memory};
use kestrel_tools::date::Date;
use kestrel_tools::tools::Note;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Aadhaar,
    Pan,
    Passport,
    Licence,
    Insurance,
    Marksheet,
    Bill,
}

impl Kind {
    /// As stored and as said: "your PAN card".
    pub fn word(self) -> &'static str {
        match self {
            Kind::Aadhaar => "Aadhaar card",
            Kind::Pan => "PAN card",
            Kind::Passport => "passport",
            Kind::Licence => "driving licence",
            Kind::Insurance => "insurance policy",
            Kind::Marksheet => "marksheet",
            Kind::Bill => "bill",
        }
    }
}

/// What a request asks about documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    List,
    Check,
    Where(Kind),
    Expiry(Kind),
}

/// What kind of document this file is: a PDF by its name or its text, any other note only by words
/// that a real document carries (a note saying "renew my passport" is not a passport).
pub fn kind_of(name: &str, text: &str) -> Option<Kind> {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).collect();
    if lower.ends_with(".pdf") {
        let by_name: [(Kind, &[&str]); 7] = [
            (Kind::Aadhaar, &["aadhaar", "aadhar", "adhar", "uidai"]),
            (Kind::Pan, &["pan", "epan", "pancard"]),
            (Kind::Passport, &["passport"]),
            (Kind::Licence, &["dl", "licence", "license", "driving"]),
            (Kind::Insurance, &["insurance", "policy"]),
            (Kind::Marksheet, &["marksheet", "marks"]),
            (Kind::Bill, &["bill", "invoice"]),
        ];
        let named = by_name
            .iter()
            .find(|(_, c)| words.iter().any(|w| c.contains(w)));
        if let Some((k, _)) = named {
            return Some(*k);
        }
    }
    let text = text.to_lowercase();
    let by_text: [(Kind, &[&str]); 7] = [
        (Kind::Aadhaar, &["unique identification authority", "uidai"]),
        (Kind::Pan, &["permanent account number"]),
        (Kind::Passport, &["passport no", "passport number"]),
        (
            Kind::Licence,
            &["driving licence", "driving license", "dl no"],
        ),
        (
            Kind::Insurance,
            &["policy no", "policy number", "sum insured"],
        ),
        (
            Kind::Marksheet,
            &["statement of marks", "marksheet", "mark sheet"],
        ),
        (
            Kind::Bill,
            &[
                "amount due",
                "amount payable",
                "electricity bill",
                "invoice no",
                "bill no",
            ],
        ),
    ];
    by_text
        .iter()
        .find(|(_, c)| c.iter().any(|w| text.contains(w)))
        .map(|(k, _)| *k)
}

/// A date in `s`: three numbers (day first, or a year first), or a day, a month's name and a year.
fn date_in(s: &str) -> Option<Date> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let month = |w: &str| {
        let w = w.to_lowercase();
        if w.len() < 3 || !w.is_ascii() {
            return None;
        }
        MONTHS
            .iter()
            .position(|m| w.starts_with(m))
            .map(|i| i as u32 + 1)
    };
    // "5th" is 5.
    let number = |w: &str| {
        let w = w.trim_end_matches(['s', 't', 'n', 'd', 'r', 'h']);
        if w.is_empty() || w.len() > 4 {
            return None;
        }
        w.parse::<u32>().ok()
    };
    let year = |w: &str| match (w.len(), number(w)?) {
        (4, y) => Some(y),
        (2, y) => Some(2000 + y),
        _ => None,
    };
    let tokens: Vec<&str> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    tokens.windows(3).find_map(|t| {
        let (y, m, d) = if let (Some(m), Some(d), Some(y)) = (month(t[0]), number(t[1]), year(t[2]))
        {
            (y, m, d)
        } else if let (Some(d), Some(m), Some(y)) = (number(t[0]), month(t[1]), year(t[2])) {
            (y, m, d)
        } else if t[0].len() == 4 {
            (number(t[0])?, number(t[1])?, number(t[2])?)
        } else {
            (year(t[2])?, number(t[1])?, number(t[0])?)
        };
        Date::parse(&format!("{y:04}-{m:02}-{d:02}"))
    })
}

/// The expiry date printed in a document: the first date after a label such as "Date of Expiry" or
/// "Valid till", on its line or the next. Dates are read day first, as in India. A date with no such
/// label (of issue, of birth) is not an expiry.
pub fn expiry_of(text: &str) -> Option<Date> {
    const LABELS: [&str; 14] = [
        "date of expiry",
        "expiry date",
        "expiry",
        "expires on",
        "expires",
        "valid till",
        "valid until",
        "valid upto",
        "valid up to",
        "validity",
        "policy end date",
        "end date",
        "exp date",
        "exp.",
    ];
    let lines: Vec<&str> = text.lines().collect();
    lines.iter().enumerate().find_map(|(i, line)| {
        let lower = line.to_lowercase();
        let after = LABELS
            .iter()
            .filter_map(|l| lower.find(l).map(|at| at + l.len()))
            .min()?;
        date_in(&lower[after..]).or_else(|| lines.get(i + 1).and_then(|next| date_in(next)))
    })
}

/// What `text` asks about documents, if it does.
pub fn parse(text: &str) -> Option<Asked> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let any = |c: &[&str]| words.iter().any(|w| c.contains(w));
    // Asking for something else that only mentions a document.
    let other = [
        "write",
        "likh",
        "likho",
        "likhna",
        "draft",
        "mail",
        "email",
        "translate",
        "anuvad",
        "summarize",
        "summary",
        "notes",
        "note",
        "what",
        "kaise",
        "how",
        "remind",
        "yaad",
        "explain",
        "why",
        "kyun",
    ];
    if words.len() > 9 || any(&other) {
        return None;
    }
    let show = [
        "kahan", "kaha", "where", "dikhao", "dikha", "show", "find", "nikalo", "open", "kholo",
    ];
    let expiry = [
        "expire", "expires", "expiry", "expired", "khatam", "valid", "validity", "renew",
    ];
    if any(&[
        "documents",
        "document",
        "kaagaz",
        "kagaz",
        "kaagzat",
        "kagzat",
        "papers",
    ]) {
        return if any(&["check", "scan", "jaanch", "jaancho"]) {
            Some(Asked::Check)
        } else if any(&show) || words.len() <= 3 {
            Some(Asked::List)
        } else {
            None
        };
    }
    let named: [(Kind, &[&str]); 7] = [
        (Kind::Aadhaar, &["aadhaar", "aadhar", "adhar"]),
        (Kind::Pan, &["pan", "pancard"]),
        (Kind::Passport, &["passport"]),
        (Kind::Licence, &["licence", "license", "dl"]),
        (Kind::Insurance, &["insurance", "policy"]),
        (Kind::Marksheet, &["marksheet"]),
        (Kind::Bill, &["bill", "invoice"]),
    ];
    let (kind, _) = named.iter().find(|(_, c)| any(c))?;
    if any(&expiry) {
        Some(Asked::Expiry(*kind))
    } else if any(&show) {
        Some(Asked::Where(*kind))
    } else {
        None
    }
}

/// "11 March 2029".
fn long(d: Date) -> String {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!("{} {} {}", d.d, NAMES[(d.m as usize - 1) % 12], d.y)
}

/// The documents among `notes`, for `Memory::sync_documents`.
pub fn found_in(notes: &[Note]) -> Vec<FoundDocument> {
    notes
        .iter()
        .filter_map(|n| {
            let kind = kind_of(&n.name, &n.text)?;
            Some(FoundDocument {
                path: n.name.clone(),
                kind: kind.word().into(),
                sha256: Sha256::digest(n.text.as_bytes())
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
                found_on: expiry_of(&n.text).map(|d| d.to_string()),
            })
        })
        .collect()
}

fn date(d: &Option<String>) -> Option<Date> {
    d.as_deref().and_then(Date::parse)
}

fn listed(r: &DocumentRow) -> String {
    let when = match (date(&r.expires_on), date(&r.found_on), r.answered) {
        (Some(d), ..) => format!(", expires {}", long(d)),
        (None, Some(d), false) => format!(", says it expires {} (not confirmed)", long(d)),
        _ => String::new(),
    };
    format!("{}: {}{when}", r.kind, r.path)
}

fn expiry_line(r: &DocumentRow, today: Date) -> String {
    match (date(&r.expires_on), date(&r.found_on), r.answered) {
        (Some(d), ..) => {
            let left = d.days() - today.days();
            let when = if left >= 0 {
                format!("expires on {}, in {left} days", long(d))
            } else {
                format!("expired on {}, {} days ago", long(d), -left)
            };
            format!("Your {} ({}) {when}.", r.kind, r.path)
        }
        (None, Some(d), false) => format!(
            "Your {} ({}) says it expires on {}. To be reminded, say: check my documents",
            r.kind,
            r.path,
            long(d)
        ),
        _ => format!("I found no expiry date in your {} ({}).", r.kind, r.path),
    }
}

/// Asks about each date not yet answered; a yes keeps it.
fn check(
    memory: &mut Memory,
    rows: &[DocumentRow],
    confirm: &mut dyn FnMut(&str) -> bool,
) -> Result<Vec<String>, KestrelError> {
    let mut said = Vec::new();
    for r in rows.iter().filter(|r| !r.answered) {
        let Some(d) = date(&r.found_on) else { continue };
        let q = format!(
            "Your {} ({}) says it expires on {}. Keep this date and remind you before it?",
            r.kind,
            r.path,
            long(d)
        );
        if confirm(&q) {
            memory.answer_document(&r.path, r.found_on.as_deref())?;
            said.push(format!(
                "I will remind you 30 days and 7 days before {} ({}).",
                long(d),
                r.kind
            ));
        } else {
            memory.answer_document(&r.path, None)?;
            said.push(format!("Not kept: the date in {}.", r.path));
        }
    }
    if said.is_empty() {
        let kept = rows.iter().filter(|r| r.expires_on.is_some()).count();
        said.push(format!(
            "Nothing new: {} documents, {kept} with an expiry date kept.",
            rows.len()
        ));
    }
    Ok(said)
}

/// Answers `asked` with the folder as it is now; `confirm` asks before a date is kept.
pub fn answer(
    asked: Asked,
    memory: &mut Memory,
    notes: &[Note],
    today: Date,
    confirm: &mut dyn FnMut(&str) -> bool,
) -> Result<Vec<String>, KestrelError> {
    memory.sync_documents(&found_in(notes))?;
    let rows = memory.documents()?;
    let none = |k: Kind| vec![format!("I found no {} in your notes folder.", k.word())];
    let of = |k: Kind| rows.iter().filter(move |r| r.kind == k.word());
    Ok(match asked {
        Asked::List if rows.is_empty() => vec![
            "No documents found in your notes folder. PDFs of your ID cards, policies and bills \
             show up here once they are in it."
                .to_string(),
        ],
        Asked::List => rows.iter().map(listed).collect(),
        Asked::Where(k) => {
            let found: Vec<&str> = of(k).map(|r| r.path.as_str()).collect();
            if found.is_empty() {
                none(k)
            } else {
                vec![format!("Your {}: {}", k.word(), found.join(", "))]
            }
        }
        Asked::Expiry(k) => {
            let found: Vec<String> = of(k).map(|r| expiry_line(r, today)).collect();
            if found.is_empty() { none(k) } else { found }
        }
        Asked::Check => check(memory, &rows, confirm)?,
    })
}

/// What `GET /api/documents` answers, with the folder as it is now.
pub fn page_json(
    memory: &mut Memory,
    notes: &[Note],
    today: Date,
) -> Result<serde_json::Value, KestrelError> {
    memory.sync_documents(&found_in(notes))?;
    let rows: Vec<serde_json::Value> = memory
        .documents()?
        .iter()
        .map(|r| {
            let shown = date(&r.expires_on).or_else(|| date(&r.found_on).filter(|_| !r.answered));
            serde_json::json!({
                "name": r.path,
                "kind": r.kind,
                "expires": shown.map(long),
                "confirmed": r.expires_on.is_some(),
                "days_left": date(&r.expires_on).map(|d| d.days() - today.days()),
            })
        })
        .collect();
    Ok(serde_json::json!({ "documents": rows }))
}

/// Rings the confirmed dates due a reminder, 30 and 7 days before (once past or close, one reminder,
/// not two); how many rang. A failed `send` stops here and rings again next time.
pub fn ring(
    memory: &mut Memory,
    today: Date,
    now: i64,
    send: &mut crate::remind::Notify,
) -> Result<usize, Box<dyn std::error::Error>> {
    let rows = memory.documents()?;
    let mut rung = 0;
    for r in &rows {
        let Some(d) = date(&r.expires_on) else {
            continue;
        };
        let left = d.days() - today.days();
        let week = left <= 7 && !r.reminded_7;
        if !week && (left > 30 || r.reminded_30) {
            continue;
        }
        let title = match left {
            ..0 => format!("Your {} has expired", r.kind),
            0 => format!("Your {} expires today", r.kind),
            1 => format!("Your {} expires tomorrow", r.kind),
            _ => format!("Your {} expires in {left} days", r.kind),
        };
        send(&title, &format!("{}: expiry date {}", r.path, long(d)))?;
        memory.count_use(now, "document_reminder_rang");
        memory.mark_document_reminded(&r.path, 30, now)?;
        if week {
            memory.mark_document_reminded(&r.path, 7, now)?;
        }
        rung += 1;
    }
    Ok(rung)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn note(name: &str, text: &str) -> Note {
        Note {
            name: name.into(),
            text: text.into(),
        }
    }

    #[test]
    fn the_kind_comes_from_a_pdf_s_name_or_words_a_real_document_carries() {
        for (name, text, kind) in [
            ("my passport.pdf", "", Some(Kind::Passport)),
            ("docs/Aadhar_card.PDF", "", Some(Kind::Aadhaar)),
            ("e-pan.pdf", "", Some(Kind::Pan)),
            ("DL scan.pdf", "", Some(Kind::Licence)),
            (
                "scan1.pdf",
                "INCOME TAX DEPARTMENT\nPermanent Account Number ABCDE1234F",
                Some(Kind::Pan),
            ),
            (
                "scan2.pdf",
                "Unique Identification Authority of India\n1234 5678 9012",
                Some(Kind::Aadhaar),
            ),
            (
                "scan3.pdf",
                "Passport No. Z1234567\nDate of Expiry 11/03/2029",
                Some(Kind::Passport),
            ),
            (
                "scan4.pdf",
                "Driving Licence\nDL No. UP1420190012345",
                Some(Kind::Licence),
            ),
            (
                "p.pdf",
                "Policy No. 123\nSum Insured 5,00,000",
                Some(Kind::Insurance),
            ),
            (
                "m.pdf",
                "Statement of Marks\nRoll No 12",
                Some(Kind::Marksheet),
            ),
            (
                "jan.pdf",
                "Electricity Bill\nAmount Due 1200",
                Some(Kind::Bill),
            ),
            ("todo.md", "must renew my passport soon, and the bill", None),
            ("passport-plan.md", "ideas for the trip", None),
            ("report.pdf", "Calibrated cascade routing on a laptop", None),
        ] {
            assert_eq!(kind_of(name, text), kind, "{name}");
        }
    }

    #[test]
    fn the_expiry_is_a_labelled_date_read_day_first() {
        for (text, want) in [
            (
                "Date of Issue 12/03/2019\nDate of Expiry / x 11/03/2029",
                Some("2029-03-11"),
            ),
            ("Valid till 31-Dec-2027.", Some("2027-12-31")),
            ("Expiry Date: 2030-01-15", Some("2030-01-15")),
            ("Policy End Date\n01 April 2027", Some("2027-04-01")),
            ("valid upto 31.12.2028", Some("2028-12-31")),
            ("Valid Till: March 5, 2031", Some("2031-03-05")),
            ("Expires on 05/06/30", Some("2030-06-05")),
            ("VALIDITY (TR): 09-08-2034", Some("2034-08-09")),
            ("Date of Issue 12/03/2019\nDate of Birth 01/01/1990", None),
            ("Date of Expiry 31/02/2029", None),
            ("Valid till further notice\n\n\n12/03/2019", None),
            ("no dates here", None),
        ] {
            assert_eq!(expiry_of(text), want.map(date), "{text}");
        }
    }

    #[test]
    fn requests_about_documents_are_told_from_the_rest() {
        for (text, want) in [
            ("mera PAN kahan hai", Some(Asked::Where(Kind::Pan))),
            ("where is my passport", Some(Asked::Where(Kind::Passport))),
            ("aadhaar card dikhao", Some(Asked::Where(Kind::Aadhaar))),
            ("mera aadhar kaha hai?", Some(Asked::Where(Kind::Aadhaar))),
            ("find my pan card", Some(Asked::Where(Kind::Pan))),
            ("bill kahan hai", Some(Asked::Where(Kind::Bill))),
            (
                "passport kab expire hoga",
                Some(Asked::Expiry(Kind::Passport)),
            ),
            (
                "when does my licence expire",
                Some(Asked::Expiry(Kind::Licence)),
            ),
            (
                "insurance kab khatam hogi",
                Some(Asked::Expiry(Kind::Insurance)),
            ),
            ("mere documents dikhao", Some(Asked::List)),
            ("show my documents", Some(Asked::List)),
            ("kaagaz kahan hai", Some(Asked::List)),
            ("check my documents", Some(Asked::Check)),
            ("documents check karo", Some(Asked::Check)),
            ("bill bharna yaad dila dena", None),
            ("remind me to pay the bill at 5 pm", None),
            ("write a mail about my passport expiry", None),
            ("what is a passport", None),
            ("pan card kaise banaye", None),
            ("find my notes about passport", None),
            ("translate where is my passport to hindi", None),
            ("mere tasks btao", None),
        ] {
            assert_eq!(parse(text), want, "{text}");
        }
    }

    fn folder() -> Vec<Note> {
        vec![
            note(
                "passport.txt",
                "Passport No. Z1234567\nDate of Expiry 11/03/2029",
            ),
            note("e-pan.txt", "Permanent Account Number ABCDE1234F"),
            note("todo.md", "renew my passport"),
        ]
    }

    #[test]
    fn a_date_is_kept_only_after_yes_and_each_is_asked_once() {
        let mut m = Memory::in_memory().unwrap();
        let today = date("2026-11-03");
        let mut asked = Vec::new();
        let lines = answer(Asked::Check, &mut m, &folder(), today, &mut |q| {
            asked.push(q.to_string());
            true
        })
        .unwrap();
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert!(
            asked[0].contains("passport.txt") && asked[0].contains("11 March 2029"),
            "{asked:?}"
        );
        assert!(lines.iter().any(|l| l.contains("30 days")), "{lines:?}");
        assert_eq!(
            m.documents()
                .unwrap()
                .iter()
                .filter(|d| d.expires_on.is_some())
                .count(),
            1
        );

        let lines = answer(Asked::Check, &mut m, &folder(), today, &mut |_| {
            panic!("asked again")
        })
        .unwrap();
        assert!(lines.iter().any(|l| l.contains("Nothing new")), "{lines:?}");

        let mut other = Memory::in_memory().unwrap();
        answer(Asked::Check, &mut other, &folder(), today, &mut |_| false).unwrap();
        assert!(
            other
                .documents()
                .unwrap()
                .iter()
                .all(|d| d.expires_on.is_none())
        );
        answer(Asked::Check, &mut other, &folder(), today, &mut |_| {
            panic!("asked again")
        })
        .unwrap();
    }

    #[test]
    fn where_and_when_say_what_the_folder_holds() {
        let mut m = Memory::in_memory().unwrap();
        let today = date("2026-11-03");
        let mut no = |_: &str| panic!("nothing to confirm here");
        let say = answer(Asked::Where(Kind::Pan), &mut m, &folder(), today, &mut no).unwrap();
        assert!(
            say[0].contains("PAN card") && say[0].contains("e-pan.txt"),
            "{say:?}"
        );
        let say = answer(
            Asked::Where(Kind::Aadhaar),
            &mut m,
            &folder(),
            today,
            &mut no,
        )
        .unwrap();
        assert!(say[0].contains("no Aadhaar card"), "{say:?}");
        // Printed in the file, not yet confirmed: it says so.
        let say = answer(
            Asked::Expiry(Kind::Passport),
            &mut m,
            &folder(),
            today,
            &mut no,
        )
        .unwrap();
        assert!(
            say[0].contains("11 March 2029") && say[0].contains("check my documents"),
            "{say:?}"
        );
        let say = answer(Asked::Expiry(Kind::Pan), &mut m, &folder(), today, &mut no).unwrap();
        assert!(say[0].contains("no expiry date"), "{say:?}");
        m.answer_document("passport.txt", Some("2029-03-11"))
            .unwrap();
        let say = answer(
            Asked::Expiry(Kind::Passport),
            &mut m,
            &folder(),
            today,
            &mut no,
        )
        .unwrap();
        assert!(
            say[0].contains("11 March 2029") && say[0].contains("days"),
            "{say:?}"
        );
        let say = answer(Asked::List, &mut m, &folder(), today, &mut no).unwrap();
        assert_eq!(
            say.iter().filter(|l| l.contains(".txt")).count(),
            2,
            "{say:?}"
        );
        let none = answer(
            Asked::List,
            &mut Memory::in_memory().unwrap(),
            &[],
            today,
            &mut no,
        )
        .unwrap();
        assert!(none[0].contains("No documents"), "{none:?}");
    }

    #[test]
    fn the_page_sees_each_document_and_whether_its_date_is_confirmed() {
        let mut m = Memory::in_memory().unwrap();
        let today = date("2029-03-01");
        let v = page_json(&mut m, &folder(), today).unwrap();
        let docs = v["documents"].as_array().unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[1]["name"], "passport.txt");
        assert_eq!(docs[1]["expires"], "11 March 2029");
        assert_eq!(docs[1]["confirmed"], false);
        assert_eq!(docs[0]["expires"], serde_json::Value::Null);
        m.answer_document("passport.txt", Some("2029-03-11"))
            .unwrap();
        let v = page_json(&mut m, &folder(), today).unwrap();
        assert_eq!(v["documents"][1]["confirmed"], true);
        assert_eq!(v["documents"][1]["days_left"], 10);
    }

    fn rung(m: &mut Memory, day: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        ring(m, date(day), 1, &mut |t, b| {
            out.push((t.to_string(), b.to_string()));
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn a_confirmed_date_rings_30_and_7_days_before_and_each_once() {
        let mut m = Memory::in_memory().unwrap();
        m.sync_documents(&found_in(&folder())).unwrap();
        assert!(
            rung(&mut m, "2029-03-10").is_empty(),
            "not confirmed: never rings"
        );
        m.answer_document("passport.txt", Some("2029-03-11"))
            .unwrap();
        assert!(rung(&mut m, "2029-02-08").is_empty(), "31 days before");
        let first = rung(&mut m, "2029-02-09");
        assert_eq!(first.len(), 1);
        assert!(
            first[0].0.contains("passport") && first[0].0.contains("30 days"),
            "{first:?}"
        );
        assert!(first[0].1.contains("passport.txt"), "{first:?}");
        assert!(rung(&mut m, "2029-02-10").is_empty(), "once");
        assert!(rung(&mut m, "2029-03-03").is_empty(), "8 days before");
        let second = rung(&mut m, "2029-03-04");
        assert!(second[0].0.contains("7 days"), "{second:?}");
        assert!(rung(&mut m, "2029-03-05").is_empty(), "once");
    }

    #[test]
    fn a_date_confirmed_late_rings_once_and_a_failed_send_rings_again() {
        let mut m = Memory::in_memory().unwrap();
        m.sync_documents(&found_in(&folder())).unwrap();
        m.answer_document("passport.txt", Some("2029-03-11"))
            .unwrap();
        let e = ring(&mut m, date("2029-03-08"), 1, &mut |_, _| {
            Err("no desktop".into())
        });
        assert!(e.is_err());
        let late = rung(&mut m, "2029-03-08");
        assert_eq!(late.len(), 1, "within 7 days: one reminder, not two");
        assert!(late[0].0.contains("3 days"), "{late:?}");
        assert!(rung(&mut m, "2029-03-09").is_empty());
        let rows = m.usage().unwrap();
        let rang = rows.iter().find(|r| r.what == "document_reminder_rang");
        assert_eq!(rang.map(|r| r.n), Some(1), "not the one that failed");
        let mut past = Memory::in_memory().unwrap();
        past.sync_documents(&found_in(&folder())).unwrap();
        past.answer_document("passport.txt", Some("2029-03-11"))
            .unwrap();
        let gone = rung(&mut past, "2029-04-01");
        assert!(gone[0].0.contains("expired"), "{gone:?}");
    }
}
