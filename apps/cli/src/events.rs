//! What the Kestrel app (apps/desktop, v4 A3) hears without asking: reminders as they ring; expiries
//! and downloads later (blueprint §4.10–4.11). `GET /api/events?after=N` answers at once with the
//! events numbered above N, and the app asks every few seconds. Not a long poll: the service answers
//! one request at a time, so a held request would hold up the page and the socket.
//!
//! An event is kept `KEEP_S` (at most `MAX_KEPT` of them): an app that starts a little after the
//! service still hears what rang at its start, and one opened much later does not hear it again.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// How long an event is kept for the app, in seconds.
pub const KEEP_S: i64 = 600;
/// The most events kept.
pub const MAX_KEPT: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub seq: u64,
    pub kind: &'static str,
    pub title: String,
    pub body: String,
    /// Unix seconds.
    pub at: i64,
}

#[derive(Debug, Default)]
pub struct Events {
    last: u64,
    kept: VecDeque<Event>,
}

/// The service's events, shared by the reminder thread and the page.
pub type Shared = Arc<Mutex<Events>>;

impl Events {
    /// Adds an event that happened at `at`; its number.
    pub fn push(&mut self, kind: &'static str, title: &str, body: &str, at: i64) -> u64 {
        self.last += 1;
        self.kept.push_back(Event {
            seq: self.last,
            kind,
            title: title.into(),
            body: body.into(),
            at,
        });
        if self.kept.len() > MAX_KEPT {
            self.kept.pop_front();
        }
        self.last
    }

    /// What `GET /api/events?after=N` answers at `now`: the kept events numbered above `after`,
    /// and the last number given. An `after` above that number was asked before a restart:
    /// everything kept.
    pub fn after(&self, after: u64, now: i64) -> Value {
        let after = if after > self.last { 0 } else { after };
        let events: Vec<Value> = self
            .kept
            .iter()
            .filter(|e| e.seq > after && now - e.at <= KEEP_S)
            .map(|e| {
                json!({"seq": e.seq, "kind": e.kind, "title": e.title, "body": e.body, "at": e.at})
            })
            .collect();
        json!({"events": events, "last": self.last})
    }
}

/// The N of `after=N` in a query string; 0 without one, None when N is not a number.
pub fn after_in(query: &str) -> Option<u64> {
    match query.split('&').find_map(|p| p.strip_prefix("after=")) {
        Some(n) => n.parse().ok(),
        None => Some(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seqs(v: &Value) -> Vec<u64> {
        v["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["seq"].as_u64().unwrap())
            .collect()
    }

    #[test]
    fn the_app_hears_each_event_once_by_asking_after_the_last() {
        let mut e = Events::default();
        assert_eq!(e.after(0, 1000), json!({"events": [], "last": 0}));
        assert_eq!(
            e.push("reminder", "Kestrel reminder", "09:00 Call mom", 1000),
            1
        );
        let first = e.after(0, 1005);
        assert_eq!(
            first,
            json!({"events": [{"seq": 1, "kind": "reminder", "title": "Kestrel reminder",
                               "body": "09:00 Call mom", "at": 1000}], "last": 1})
        );
        assert_eq!(e.push("reminder", "Kestrel reminder", "09:30 Gym", 1800), 2);
        assert_eq!(seqs(&e.after(1, 1805)), [2]);
        assert_eq!(e.after(2, 1810), json!({"events": [], "last": 2}));
    }

    #[test]
    fn an_event_is_kept_ten_minutes_and_at_most_max_kept() {
        let mut e = Events::default();
        e.push("reminder", "t", "b", 1000);
        assert_eq!(seqs(&e.after(0, 1000 + KEEP_S)), [1]);
        assert_eq!(seqs(&e.after(0, 1000 + KEEP_S + 1)), Vec::<u64>::new());
        for i in 0..MAX_KEPT as i64 + 5 {
            e.push("reminder", "t", "b", 2000 + i);
        }
        let all = seqs(&e.after(0, 2000));
        assert_eq!(all.len(), MAX_KEPT);
        assert_eq!(all[0], 7);
        assert_eq!(e.after(0, 2000)["last"], MAX_KEPT as u64 + 6);
    }

    #[test]
    fn an_app_that_asks_after_a_number_never_given_hears_everything_kept() {
        // The service restarted under the app: its numbers began again.
        let mut e = Events::default();
        e.push("reminder", "t", "b", 1000);
        assert_eq!(seqs(&e.after(57, 1001)), [1]);
    }

    #[test]
    fn after_is_read_from_the_query() {
        assert_eq!(after_in(""), Some(0));
        assert_eq!(after_in("after=12"), Some(12));
        assert_eq!(after_in("x=1&after=3"), Some(3));
        assert_eq!(after_in("after=-1"), None);
        assert_eq!(after_in("after="), None);
        assert_eq!(after_in("after=ten"), None);
    }
}
