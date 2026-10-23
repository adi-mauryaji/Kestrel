//! Context construction (Ch. 18.2): approved preferences always (Ch. 8 step 3), then the
//! retrieved items, one line each with its provenance (`[pref|user_explicit|2026-09-30]
//! answer_length: concise`) so the model and the user can see why it is there. The token budget is
//! counted with the caller's counter (the target model's tokenizer). Over budget, summaries go
//! before raw items. A pending item never appears, even if a caller passes one.

use crate::retrieval::Scored;
use crate::store::{Item, Kind};

/// The rendered context and what went into it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Context {
    pub text: String,
    pub ids: Vec<i64>,
    /// Items left out: over budget, or not approved.
    pub dropped: usize,
}

/// One context line: `[kind|source|updated date] key: value (exception: ...)`.
pub fn line(item: &Item) -> String {
    let kind = match item.kind {
        Kind::Preference => "pref",
        Kind::Fact => "fact",
        Kind::Summary => "summary",
        Kind::TaskHistory => "task",
    };
    let mut s = format!(
        "[{kind}|{}|{}] {}: {}",
        item.source.as_str(),
        utc_date(item.updated_at),
        item.key,
        item.value
    );
    if let Some(e) = &item.exception {
        s.push_str(&format!(" (exception: {e})"));
    }
    s
}

/// Preferences, then retrieved summaries, then the other retrieved items (each group in the
/// order given), every line that still fits `budget` as counted by `count` (+1 for its newline).
pub fn build(
    prefs: Vec<Item>,
    retrieved: Vec<Scored>,
    budget: usize,
    count: &dyn Fn(&str) -> usize,
) -> Context {
    let (summaries, rest): (Vec<Item>, Vec<Item>) = retrieved
        .into_iter()
        .map(|s| s.item)
        .partition(|i| i.kind == Kind::Summary);
    let mut c = Context::default();
    let mut lines = Vec::new();
    let mut used = 0;
    for item in prefs.into_iter().chain(summaries).chain(rest) {
        let l = line(&item);
        let cost = count(&l) + 1;
        if !item.approved || used + cost > budget {
            c.dropped += 1;
            continue;
        }
        used += cost;
        c.ids.push(item.id);
        lines.push(l);
    }
    c.text = lines.join("\n");
    c
}

/// The UTC calendar date of a Unix time, `YYYY-MM-DD` (Hinnant's `civil_from_days`, as in
/// kestrel-tools' date.rs; tools depends on memory, so it cannot be shared from there).
pub fn utc_date(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Kind, Source};

    const SEPT_30: i64 = 1_790_726_400; // 2026-09-30T00:00:00Z

    fn item(id: i64, kind: Kind, key: &str, value: &str) -> Item {
        Item {
            id,
            kind,
            key: key.into(),
            value: value.into(),
            exception: None,
            source: Source::UserExplicit,
            importance: 3,
            approved: true,
            created_at: 0,
            updated_at: SEPT_30,
            expires_at: None,
        }
    }

    fn scored(item: Item, score: f64) -> Scored {
        Scored {
            item,
            score,
            matched: 1,
        }
    }

    fn words(s: &str) -> usize {
        s.split_whitespace().count()
    }

    #[test]
    fn dates_are_utc_calendar_days() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
        assert_eq!(utc_date(-1), "1969-12-31");
        assert_eq!(utc_date(SEPT_30 + 86_399), "2026-09-30");
    }

    #[test]
    fn a_line_shows_kind_source_date_and_any_exception() {
        let mut it = item(1, Kind::Preference, "answer_length", "concise");
        it.source = Source::Correction;
        it.exception = Some("i ask for details".into());
        assert_eq!(
            line(&it),
            "[pref|correction|2026-09-30] answer_length: concise (exception: i ask for details)"
        );
        let task = item(2, Kind::TaskHistory, "task_7", "call asha");
        assert_eq!(
            line(&task),
            "[task|user_explicit|2026-09-30] task_7: call asha"
        );
    }

    #[test]
    fn preferences_first_then_summaries_and_the_budget_holds() {
        let prefs = vec![item(1, Kind::Preference, "answer_length", "concise")];
        let hits = || {
            vec![
                scored(
                    item(2, Kind::Fact, "advisor", "dr rao reviews the budget"),
                    0.9,
                ),
                scored(
                    item(3, Kind::Summary, "week_42", "budget moved to friday"),
                    0.5,
                ),
                scored(item(4, Kind::Fact, "budget", "q4 budget is 2 lakh"), 0.4),
            ]
        };
        let all = build(prefs.clone(), hits(), 1000, &words);
        assert_eq!(all.ids, [1, 3, 2, 4]);
        assert_eq!(all.dropped, 0);
        assert_eq!(all.text.lines().count(), 4);
        // Each line costs its words + 1 (the newline): 4 + 7 fit in 11, nothing else does.
        let tight = build(prefs, hits(), 11, &words);
        assert_eq!(tight.ids, [1, 3]);
        assert_eq!(tight.dropped, 2);
    }

    #[test]
    fn pending_items_never_enter_the_context() {
        let mut pending = item(5, Kind::Fact, "guess", "budget due monday");
        pending.approved = false;
        pending.source = Source::ModelInferred;
        let c = build(vec![], vec![scored(pending, 1.0)], 1000, &words);
        assert_eq!(
            c,
            Context {
                text: String::new(),
                ids: vec![],
                dropped: 1
            }
        );
    }
}
