//! Template plans for single-intent requests (Ch. 14.2, Ch. 8.2; ADR-023, ADR-024): a request the
//! router accepted as one intent gets its plan from deterministic code, no model. The plan is JSON
//! and goes through `plan::parse` like a model's, so there is one validator.
//!
//! Arguments are extracted by rule, for English and Romanized Hinglish alike:
//! - a due date: today / tonight / aaj, tomorrow / kal, the day after tomorrow / parso, weekday
//!   names (English and Hindi: somvar ...), `YYYY-MM-DD`, with "by / on / for" or "ko / tak";
//!   a weekday means the next one strictly after today.
//! - a task title: the request minus its command words and date phrase, in the user's words
//!   (ADR-024); an oblique Hinglish infinitive before "ka task / ka reminder" becomes plain
//!   ("lene ka task" -> "lena").
//! - a search query or memory terms: the content words left after removing command and question
//!   words (one stopword list per use), so new phrasings work without new patterns.
//! - notes to summarize: "last three / latest" -> how many (default 5), the word before "notes"
//!   -> the tag, "about X / X ke baare mein" -> a search, "document" -> read_document.
//! - a document: its name matched against the notes' names without the date prefix.
//!
//! A required slot that is missing is a `Clarify`: ask, never invent (Ch. 16).

use kestrel_core::Intent;
use serde_json::{Value, json};

use crate::date::Date;

#[derive(Debug, Clone, PartialEq)]
pub enum Proposal {
    /// A plan in the wire format of `plan::parse`.
    Plan(Value),
    /// A required slot is missing: ask for it.
    Clarify(&'static str),
    /// Not a template's job (several steps or nothing matched): Tier 4 plans it (9d).
    NeedsModel,
    /// Chitchat or out of scope: no tool runs.
    NoAction,
}

pub struct Context<'a> {
    pub today: Date,
    /// Note and document names under the sandbox roots (relative, e.g. "docs/project-plan.md").
    pub documents: &'a [String],
}

pub fn propose(intent: Intent, text: &str, ctx: &Context) -> Proposal {
    let words = words(text);
    match intent {
        Intent::CreateTask => create_task(words, ctx.today),
        Intent::SearchNotes => match content(&words, SEARCH_STOP) {
            q if q.is_empty() => Proposal::Clarify("query"),
            q => one("s", "search_notes", json!({"query": q})),
        },
        Intent::RetrieveMemory => match content(&words, MEMORY_STOP) {
            t if t.is_empty() => Proposal::Clarify("terms"),
            t => one("m", "retrieve_memory", json!({"terms": t})),
        },
        Intent::ReadDocument => match document(&words, READ_STOP, ctx.documents) {
            Some(path) => one("r", "read_document", json!({"path": path})),
            None => Proposal::Clarify("document"),
        },
        Intent::DeleteTask => match words.iter().find_map(|w| w.parse::<u64>().ok()) {
            Some(id) => one("d", "delete_task", json!({"id": id})),
            None => Proposal::Clarify("task id"),
        },
        Intent::SummarizeNotes => summarize(&words, ctx.documents),
        Intent::SetPreference => match preference(&words) {
            Some((key, value)) => one(
                "p",
                "update_preference",
                json!({"key": key, "value": value}),
            ),
            None => Proposal::Clarify("preference"),
        },
        Intent::Chitchat | Intent::Unsupported => Proposal::NoAction,
    }
}

/// The normalized request (as `Request::new` makes it) split into words, punctuation around
/// each word removed.
fn words(text: &str) -> Vec<String> {
    let trim = |c: char| matches!(c, ',' | ';' | ':' | '"' | '(' | ')' | '!' | '?' | '.');
    kestrel_core::normalize::normalize(text)
        .split_whitespace()
        .map(|w| w.trim_matches(trim).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

fn one(id: &str, op: &str, args: Value) -> Proposal {
    Proposal::Plan(json!({"nodes": [{"id": id, "op": op, "args": args}]}))
}

fn content(words: &[String], stop: &[&str]) -> String {
    words
        .iter()
        .filter(|w| !stop.contains(&w.as_str()))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn starts(words: &[String], at: usize, pat: &[&str]) -> bool {
    words.len() >= at + pat.len()
        && words[at..at + pat.len()]
            .iter()
            .zip(pat)
            .all(|(w, p)| w == p)
}

// --- due dates ---------------------------------------------------------------------------------

const WEEKDAYS: [(&str, u32); 16] = [
    ("monday", 0),
    ("tuesday", 1),
    ("wednesday", 2),
    ("thursday", 3),
    ("friday", 4),
    ("saturday", 5),
    ("sunday", 6),
    ("somvar", 0),
    ("mangalvar", 1),
    ("budhvar", 2),
    ("guruvar", 3),
    ("brihaspativar", 3),
    ("shukravar", 4),
    ("shanivar", 5),
    ("ravivar", 6),
    ("itvaar", 6),
];
const BEFORE_DATE: [&str; 6] = ["by", "on", "for", "before", "till", "until"];
const AFTER_DATE: [&str; 3] = ["ko", "tak", "se"];

/// The first date phrase in `words`, removed from them.
fn take_due(words: &mut Vec<String>, today: Date) -> Option<Date> {
    for i in 0..words.len() {
        let found: Option<(usize, Date)> = if starts(words, i, &["the", "day", "after", "tomorrow"])
        {
            Some((4, today.add_days(2)))
        } else if starts(words, i, &["day", "after", "tomorrow"]) {
            Some((3, today.add_days(2)))
        } else if starts(words, i, &["aaj", "ke", "liye"]) {
            Some((3, today))
        } else {
            let w = words[i].as_str();
            let next = words.get(i + 1).map(String::as_str);
            let day = |name: &str| WEEKDAYS.iter().find(|(n, _)| *n == name).map(|(_, d)| *d);
            match w {
                "today" | "tonight" | "aaj" => Some((1, today)),
                "tomorrow" | "kal" => Some((1, today.add_days(1))),
                "parso" | "parson" => Some((1, today.add_days(2))),
                "next" | "this" => next.and_then(day).map(|d| (2, today.next_weekday(d))),
                _ => day(w)
                    .map(|d| (1, today.next_weekday(d)))
                    .or_else(|| Date::parse(w).map(|d| (1, d))),
            }
        };
        if let Some((len, date)) = found {
            let mut start = i;
            let mut end = i + len;
            if start > 0 && BEFORE_DATE.contains(&words[start - 1].as_str()) {
                start -= 1;
            }
            if words
                .get(end)
                .is_some_and(|w| AFTER_DATE.contains(&w.as_str()))
            {
                end += 1;
            }
            words.drain(start..end);
            return Some(date);
        }
    }
    None
}

// --- task titles (ADR-024) ---------------------------------------------------------------------

const EN_PREFIXES: [&[&str]; 19] = [
    &["remind", "me", "to"],
    &["add", "a", "task", "to"],
    &["create", "a", "task", "to"],
    &["make", "a", "task", "to"],
    &["add", "a", "reminder", "to"],
    &["set", "a", "reminder", "to"],
    &["create", "a", "reminder", "to"],
    &["add", "a", "todo", "to"],
    &["remind", "me"],
    &["add", "a", "task"],
    &["create", "a", "task"],
    &["add", "a", "reminder"],
    &["set", "a", "reminder"],
    &["new", "task"],
    &["add", "task"],
    &["todo"],
    &["put"],
    &["add"],
    &["create"],
];
const EN_SUFFIXES: [&[&str]; 8] = [
    &["on", "my", "todo", "list"],
    &["to", "my", "todo", "list"],
    &["on", "my", "to-do", "list"],
    &["on", "my", "list"],
    &["to", "my", "list"],
    &["to", "my", "tasks"],
    &["on", "my", "tasks"],
    &["to", "the", "list"],
];
const HI_PREFIXES: [&[&str]; 3] = [
    &["todo", "list", "mein"],
    &["task", "list", "mein"],
    &["list", "mein"],
];
/// Command words after the title. With `oblique`, the title's last word is an oblique infinitive
/// ("lene ka task") that becomes plain ("lena").
const HI_SUFFIXES: [(&[&str], bool); 14] = [
    (&["ka", "task", "add", "kar", "do"], true),
    (&["ka", "task", "add", "karo"], true),
    (&["ka", "task", "bana", "do"], true),
    (&["ka", "task", "banao"], true),
    (&["ka", "reminder", "laga", "do"], true),
    (&["ka", "reminder", "lagao"], true),
    (&["ka", "reminder", "set", "karo"], true),
    (&["yaad", "dila", "do"], false),
    (&["yaad", "dilana"], false),
    (&["yaad", "dilao"], false),
    (&["remind", "kar", "do"], false),
    (&["remind", "karna"], false),
    (&["daal", "do"], false),
    (&["daalo"], false),
];

fn create_task(mut words: Vec<String>, today: Date) -> Proposal {
    let due = take_due(&mut words, today);
    if let Some(p) = EN_PREFIXES.iter().find(|p| starts(&words, 0, p)) {
        words.drain(..p.len());
    }
    if let Some(p) = EN_SUFFIXES
        .iter()
        .find(|p| words.len() >= p.len() && starts(&words, words.len() - p.len(), p))
    {
        words.truncate(words.len() - p.len());
    }
    if words.first().is_some_and(|w| w == "to") {
        words.remove(0);
    }
    if let Some(p) = HI_PREFIXES.iter().find(|p| starts(&words, 0, p)) {
        words.drain(..p.len());
    }
    let hi = HI_SUFFIXES
        .iter()
        .find(|(p, _)| words.len() >= p.len() && starts(&words, words.len() - p.len(), p));
    if let Some((p, oblique)) = hi {
        words.truncate(words.len() - p.len());
        if *oblique
            && let Some(last) = words.last_mut()
            && last.len() > 2
            && last.ends_with("ne")
        {
            last.truncate(last.len() - 1);
            last.push('a');
        }
    }
    if words.is_empty() {
        return Proposal::Clarify("title");
    }
    let mut args = json!({"title": words.join(" ")});
    if let Some(d) = due {
        args["due"] = json!(d.to_string());
    }
    one("t", "create_task", args)
}

// --- notes, documents, preferences ---------------------------------------------------------------

const SEARCH_STOP: &[&str] = &[
    "find",
    "search",
    "show",
    "look",
    "up",
    "for",
    "about",
    "on",
    "in",
    "any",
    "my",
    "me",
    "i",
    "the",
    "a",
    "an",
    "notes",
    "note",
    "have",
    "has",
    "do",
    "did",
    "does",
    "what",
    "where",
    "which",
    "mentioned",
    "mention",
    "mentioning",
    "wrote",
    "write",
    "written",
    "all",
    "everything",
    "related",
    "to",
    "list",
    "please",
    "pull",
    "get",
    "fetch",
    "tagged",
    "cover",
    "covers",
    "talk",
    "talks",
    "dhundo",
    "dhundho",
    "dhoondo",
    "dhoondho",
    "dikhao",
    "nikalo",
    "laao",
    "karo",
    "kar",
    "kiya",
    "kiye",
    "tha",
    "thi",
    "likha",
    "jisme",
    "jismein",
    "maine",
    "mere",
    "mera",
    "meri",
    "ke",
    "ki",
    "ka",
    "ko",
    "pe",
    "par",
    "mein",
    "baare",
    "koi",
    "kya",
    "hai",
    "hain",
    "woh",
    "wo",
    "jo",
    "se",
    "wale",
    "wala",
    "wali",
    "kahan",
    "kahin",
    "sakte",
    "ho",
];
const MEMORY_STOP: &[&str] = &[
    "what", "whats", "what's", "do", "does", "did", "you", "your", "remember", "recall", "know",
    "about", "when", "is", "are", "was", "who", "whom", "tell", "me", "my", "the", "a", "an", "i",
    "due", "of", "kab", "kaun", "kya", "yaad", "hai", "hain", "tha", "ke", "ki", "ka", "baare",
    "mein", "mujhe", "batao", "please",
];
const READ_STOP: &[&str] = &[
    "open", "show", "read", "view", "display", "load", "bring", "pull", "up", "see", "let", "me",
    "the", "my", "a", "an", "document", "doc", "file", "note", "notes", "please", "kholo", "khol",
    "dikhao", "padho", "padh", "sunao", "do", "karo", "mera", "meri", "mere", "wala", "wali",
    "wale", "ka", "ki", "ke", "called", "named", "naam", "to", "out", "i", "want",
];
/// READ_STOP plus the words that ask for a summary.
const SUMMARY_DOC_STOP: &[&str] = &[
    "open",
    "show",
    "read",
    "the",
    "my",
    "a",
    "an",
    "document",
    "doc",
    "file",
    "please",
    "mera",
    "meri",
    "wala",
    "wali",
    "ka",
    "ki",
    "ke",
    "summarize",
    "summarise",
    "summary",
    "give",
    "me",
    "of",
    "uska",
    "iska",
    "karo",
    "do",
];
const COUNTS: [(&str, u32); 23] = [
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("ek", 1),
    ("do", 2),
    ("teen", 3),
    ("char", 4),
    ("chaar", 4),
    ("paanch", 5),
    ("panch", 5),
    ("chhe", 6),
    ("chhah", 6),
    ("saat", 7),
    ("aath", 8),
    ("nau", 9),
    ("das", 10),
];
const TAG_STOP: &[&str] = &[
    "my", "mera", "meri", "mere", "latest", "last", "recent", "the", "all", "a", "an", "of",
    "these", "those", "saare", "sab", "purane", "old",
];
/// "my X notes" without a count (dataset card; Kestrel-Tasks-v0 sum-03).
pub const DEFAULT_RECENT: u32 = 5;

fn count(w: &str) -> Option<u32> {
    w.parse()
        .ok()
        .or_else(|| COUNTS.iter().find(|(n, _)| *n == w).map(|(_, c)| *c))
}

/// The name of a note or document without its folder, extension and date prefix, words spaced.
fn slug(name: &str) -> String {
    let stem = name.rsplit('/').next().unwrap_or(name);
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
    let stem = match stem.get(..11) {
        Some(p) if Date::parse(&p[..10]).is_some() && p.ends_with('-') => &stem[11..],
        _ => stem,
    };
    stem.replace(['-', '_'], " ")
}

/// A typed path as is (the sandbox decides at run time), or the one name that matches.
fn document(words: &[String], stop: &[&str], documents: &[String]) -> Option<String> {
    let phrase = content(words, stop);
    if phrase.is_empty() {
        return None;
    }
    if phrase.contains('/') || phrase.ends_with(".md") || phrase.ends_with(".txt") {
        return Some(phrase);
    }
    if let Some(d) = documents.iter().find(|d| slug(d) == phrase) {
        return Some(d.clone());
    }
    let want: Vec<&str> = phrase.split(' ').collect();
    let mut hits = documents.iter().filter(|d| {
        let s = slug(d);
        let have: Vec<&str> = s.split(' ').collect();
        want.iter().all(|w| have.contains(w))
    });
    match (hits.next(), hits.next()) {
        (Some(d), None) => Some(d.clone()),
        _ => None,
    }
}

fn summarize(words: &[String], documents: &[String]) -> Proposal {
    let then_summarize = |id: &str, op: &str, args: Value| {
        Proposal::Plan(json!({"nodes": [
            {"id": id, "op": op, "args": args},
            {"id": "s", "op": "summarize", "deps": [id]}]}))
    };
    let pos = |w: &str| words.iter().position(|x| x == w);
    if let Some(i) = pos("about") {
        let q = content(&words[i + 1..], &["the", "my", "a", "an"]);
        return if q.is_empty() {
            Proposal::Clarify("query")
        } else {
            then_summarize("q", "search_notes", json!({"query": q}))
        };
    }
    if let Some(i) = (1..words.len()).find(|&i| words[i] == "baare" && words[i - 1] == "ke") {
        let q = content(&words[..i - 1], SEARCH_STOP);
        return if q.is_empty() {
            Proposal::Clarify("query")
        } else {
            then_summarize("q", "search_notes", json!({"query": q}))
        };
    }
    if words
        .iter()
        .any(|w| matches!(w.as_str(), "document" | "doc" | "file"))
    {
        return match document(words, SUMMARY_DOC_STOP, documents) {
            Some(path) => then_summarize("r", "read_document", json!({"path": path})),
            None => Proposal::Clarify("document"),
        };
    }
    let Some(noun) = words.iter().position(|w| w == "notes" || w == "note") else {
        return Proposal::NeedsModel;
    };
    let recency = words
        .iter()
        .position(|w| matches!(w.as_str(), "last" | "latest" | "recent"));
    let n = match recency
        .and_then(|i| words.get(i + 1))
        .and_then(|w| count(w))
    {
        Some(n) => n,
        None if recency.is_some() && words[noun] == "note" => 1,
        None => DEFAULT_RECENT,
    };
    let mut j = noun;
    while j > 0 && matches!(words[j - 1].as_str(), "wale" | "wala" | "wali") {
        j -= 1;
    }
    let tag = (j > 0)
        .then(|| words[j - 1].as_str())
        .filter(|w| !TAG_STOP.contains(w) && count(w).is_none());
    let mut args = json!({"n": n});
    if let Some(t) = tag {
        args["tag"] = json!(t);
    }
    then_summarize("f", "fetch_recent_notes", args)
}

fn preference(words: &[String]) -> Option<(&'static str, &'static str)> {
    let any = |set: &[&str]| words.iter().any(|w| set.contains(&w.as_str()));
    if any(&["summary", "summaries"]) {
        if any(&["bullet", "bullets", "points", "point"]) {
            return Some(("summary_style", "bullets"));
        }
        if any(&["paragraph", "paragraphs"]) {
            return Some(("summary_style", "paragraph"));
        }
    }
    if any(&["hinglish"]) {
        return Some(("language", "hinglish"));
    }
    if any(&["english"]) {
        return Some(("language", "english"));
    }
    if any(&[
        "short", "shorter", "brief", "concise", "chhote", "chhota", "chhoti",
    ]) {
        return Some(("answer_length", "short"));
    }
    if any(&["detailed", "long", "longer", "lambe", "lamba", "detail"]) {
        return Some(("answer_length", "detailed"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCS: [&str; 4] = [
        "docs/project-plan.md",
        "docs/q4-budget.md",
        "2026-10-18-groceries.md",
        "meetings/2026-10-20-standup.md",
    ];

    fn go(intent: Intent, text: &str) -> Proposal {
        let docs: Vec<String> = DOCS.iter().map(|s| s.to_string()).collect();
        propose(
            intent,
            text,
            &Context {
                today: Date::parse("2026-10-22").unwrap(),
                documents: &docs,
            },
        )
    }

    fn plan(intent: Intent, text: &str) -> Value {
        match go(intent, text) {
            Proposal::Plan(p) => {
                crate::plan::parse(&p.to_string())
                    .unwrap_or_else(|e| panic!("{text}: invalid template plan: {e}"));
                p
            }
            other => panic!("{text}: expected a plan, got {other:?}"),
        }
    }

    fn task(text: &str) -> (String, Option<String>) {
        let p = plan(Intent::CreateTask, text);
        let a = &p["nodes"][0]["args"];
        (
            a["title"].as_str().unwrap().to_string(),
            a["due"].as_str().map(Into::into),
        )
    }

    fn t(title: &str, due: Option<&str>) -> (String, Option<String>) {
        (title.into(), due.map(Into::into))
    }

    #[test]
    fn english_task_titles_and_due_dates() {
        assert_eq!(
            task("remind me to call asha tomorrow"),
            t("call asha", Some("2026-10-23"))
        );
        assert_eq!(
            task("remind me the day after tomorrow to call the bank"),
            t("call the bank", Some("2026-10-24"))
        );
        assert_eq!(
            task("set a reminder to pay rent on friday"),
            t("pay rent", Some("2026-10-23"))
        );
        assert_eq!(
            task("add a task to buy a birthday gift for maa"),
            t("buy a birthday gift for maa", None)
        );
        assert_eq!(
            task("create a task to file taxes by 2026-11-30"),
            t("file taxes", Some("2026-11-30"))
        );
        assert_eq!(
            task("put call mom on my todo list for today"),
            t("call mom", Some("2026-10-22"))
        );
        assert_eq!(
            task("add a reminder to renew the lease next monday"),
            t("renew the lease", Some("2026-10-26"))
        );
        assert_eq!(
            task("remind me tonight to lock the car"),
            t("lock the car", Some("2026-10-22"))
        );
    }

    #[test]
    fn hinglish_task_titles_keep_the_users_words() {
        assert_eq!(
            task("kal asha ko call karna yaad dilana"),
            t("asha ko call karna", Some("2026-10-23"))
        );
        assert_eq!(
            task("parso rent bharna yaad dila do"),
            t("rent bharna", Some("2026-10-24"))
        );
        assert_eq!(
            task("maa ke liye gift lene ka task add karo"),
            t("maa ke liye gift lena", None)
        );
        assert_eq!(
            task("friday tak report submit karne ka task bana do"),
            t("report submit karna", Some("2026-10-23"))
        );
        assert_eq!(
            task("somvar ko doctor ka appointment book karne ka reminder lagao"),
            t("doctor ka appointment book karna", Some("2026-10-26"))
        );
        assert_eq!(
            task("aaj ke liye todo list mein milk lena daal do"),
            t("milk lena", Some("2026-10-22"))
        );
        assert_eq!(
            task("2026-12-01 ko passport renew karna yaad dilana"),
            t("passport renew karna", Some("2026-12-01"))
        );
    }

    #[test]
    fn a_task_without_a_title_asks_for_one() {
        for text in [
            "remind me",
            "yaad dilana",
            "remind me tomorrow",
            "kal yaad dilana",
            "add a task",
        ] {
            assert_eq!(
                go(Intent::CreateTask, text),
                Proposal::Clarify("title"),
                "{text}"
            );
        }
    }

    fn query(text: &str) -> String {
        plan(Intent::SearchNotes, text)["nodes"][0]["args"]["query"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn search_queries_are_the_content_words() {
        assert_eq!(query("find my notes about the budget"), "budget");
        assert_eq!(query("do i have any notes on the pune trip"), "pune trip");
        assert_eq!(query("show my notes on cricket"), "cricket");
        assert_eq!(query("where did i write about the visa"), "visa");
        assert_eq!(query("notes mein kharcha search karo"), "kharcha");
        assert_eq!(query("cricket ke notes dhundo"), "cricket");
        assert_eq!(
            query("woh note dhundo jisme maine visa mention kiya tha"),
            "visa"
        );
        assert_eq!(
            go(Intent::SearchNotes, "find my notes"),
            Proposal::Clarify("query")
        );
    }

    fn nodes(p: &Value) -> Vec<(String, Value)> {
        p["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| (n["op"].as_str().unwrap().into(), n["args"].clone()))
            .collect()
    }

    #[test]
    fn summaries_fetch_find_or_read_their_notes() {
        let p = plan(
            Intent::SummarizeNotes,
            "summarize my last three meeting notes",
        );
        assert_eq!(
            nodes(&p)[0],
            (
                "fetch_recent_notes".into(),
                json!({"n": 3, "tag": "meeting"})
            )
        );
        assert_eq!(nodes(&p)[1].0, "summarize");
        assert_eq!(p["nodes"][1]["deps"], json!([p["nodes"][0]["id"]]));
        let p = plan(
            Intent::SummarizeNotes,
            "meri last do client notes ka summary do",
        );
        assert_eq!(nodes(&p)[0].1, json!({"n": 2, "tag": "client"}));
        let p = plan(Intent::SummarizeNotes, "mera latest note summarize karo");
        assert_eq!(nodes(&p)[0].1, json!({"n": 1}));
        let p = plan(Intent::SummarizeNotes, "summarize my cricket wale notes");
        assert_eq!(nodes(&p)[0].1, json!({"n": 5, "tag": "cricket"}));
        let p = plan(
            Intent::SummarizeNotes,
            "summarize what i wrote about the visa",
        );
        assert_eq!(
            nodes(&p)[0],
            ("search_notes".into(), json!({"query": "visa"}))
        );
        let p = plan(
            Intent::SummarizeNotes,
            "visa ke baare mein jo likha tha uska summary do",
        );
        assert_eq!(
            nodes(&p)[0],
            ("search_notes".into(), json!({"query": "visa"}))
        );
        let p = plan(Intent::SummarizeNotes, "summarize the q4 budget document");
        assert_eq!(
            nodes(&p)[0],
            ("read_document".into(), json!({"path": "docs/q4-budget.md"}))
        );
    }

    #[test]
    fn documents_are_matched_by_name_or_taken_as_typed_paths() {
        let path = |text| {
            plan(Intent::ReadDocument, text)["nodes"][0]["args"]["path"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(path("open the project plan"), "docs/project-plan.md");
        assert_eq!(
            path("mera groceries wala note padho"),
            "2026-10-18-groceries.md"
        );
        assert_eq!(path("q4 budget kholo"), "docs/q4-budget.md");
        assert_eq!(path("open /etc/hosts"), "/etc/hosts"); // the sandbox decides at run time
        assert_eq!(
            go(Intent::ReadDocument, "open the tax file"),
            Proposal::Clarify("document")
        );
        assert_eq!(
            go(Intent::ReadDocument, "open"),
            Proposal::Clarify("document")
        );
    }

    #[test]
    fn deletes_need_a_task_number() {
        let id = |text| {
            plan(Intent::DeleteTask, text)["nodes"][0]["args"]["id"]
                .as_u64()
                .unwrap()
        };
        assert_eq!(id("delete task 4"), 4);
        assert_eq!(id("i'm done with task 12, remove it"), 12);
        assert_eq!(id("task 7 hata do"), 7);
        assert_eq!(
            go(Intent::DeleteTask, "delete my last task"),
            Proposal::Clarify("task id")
        );
    }

    #[test]
    fn preferences_map_to_known_keys() {
        let pref = |text| {
            let p = plan(Intent::SetPreference, text);
            let a = &p["nodes"][0]["args"];
            (
                a["key"].as_str().unwrap().to_string(),
                a["value"].as_str().unwrap().to_string(),
            )
        };
        let kv = |k: &str, v: &str| (k.to_string(), v.to_string());
        assert_eq!(
            pref("keep your answers short"),
            kv("answer_length", "short")
        );
        assert_eq!(
            pref("i want detailed replies"),
            kv("answer_length", "detailed")
        );
        assert_eq!(pref("jawab chhote rakho"), kv("answer_length", "short"));
        assert_eq!(pref("talk to me in english"), kv("language", "english"));
        assert_eq!(
            pref("ab se hinglish mein baat karo"),
            kv("language", "hinglish")
        );
        assert_eq!(
            pref("summaries as bullet points please"),
            kv("summary_style", "bullets")
        );
        assert_eq!(
            go(Intent::SetPreference, "i like blue"),
            Proposal::Clarify("preference")
        );
    }

    #[test]
    fn memory_terms_are_the_content_words() {
        let terms = |text| {
            plan(Intent::RetrieveMemory, text)["nodes"][0]["args"]["terms"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(terms("what do you remember about ravi"), "ravi");
        assert_eq!(terms("who is my manager"), "manager");
        assert_eq!(terms("ravi ke baare mein kya yaad hai"), "ravi");
        assert_eq!(terms("rent kab due hai"), "rent");
        assert_eq!(
            go(Intent::RetrieveMemory, "what do you remember"),
            Proposal::Clarify("terms")
        );
    }

    #[test]
    fn chitchat_and_out_of_scope_run_no_tool() {
        assert_eq!(go(Intent::Chitchat, "hi there"), Proposal::NoAction);
        assert_eq!(
            go(Intent::Unsupported, "book me a flight"),
            Proposal::NoAction
        );
    }
}
