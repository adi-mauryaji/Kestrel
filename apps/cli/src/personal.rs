//! Memory in `kestrel ask` (Week 10; Ch. 8 steps 3 and 13, Ch. 8.3, Ch. 20). No model is needed:
//! everything here is the memory file, the correction detector and the template planner.
//!
//! Before a request is routed: a correction (Ch. 20) is handled instead of routed. A durable one
//! asks "Save: ...?" and is stored only on a yes (provenance `correction`, a `confirm` event); a
//! one-off applies to this turn and stores nothing. Any other request gets its memory context:
//! the approved preferences and the best matching memories, with provenance (Ch. 18.2).
//! After routing: a request accepted as `set_preference` ("I prefer short answers") is stored as the
//! user's own statement, the only durable memory a request writes (Ch. 8 step 13).
//! If the memory file cannot be opened, `ask` says so once and answers without personalization
//! (Ch. 8.3). The context is capped by words, a stand-in for the SLM's tokens in this demo.

use std::path::Path;

use kestrel_core::{Intent, Request};
use kestrel_learning::alias;
use kestrel_learning::correction::{self, Action};
use kestrel_memory::{Draft, Kind, Memory, Source};
use kestrel_router::Outcome;
use kestrel_tools::date::Date;
use kestrel_tools::templates::{self, Proposal};

/// Memories shown before routing, and the context budget in words.
pub const CONTEXT_K: usize = 5;
pub const CONTEXT_WORDS: usize = 120;

pub enum Store {
    On(Memory),
    /// Why there is no memory, said once.
    Off(String),
}

/// What happens to a request before routing.
#[derive(Debug, PartialEq)]
pub enum Before {
    /// A correction: handled here, not routed. The lines say what was done.
    Correction(Vec<String>),
    /// Route `text` (the request with the user's aliases resolved, ADR-028); these lines (the
    /// memory context, or a notice) come first.
    Route { lines: Vec<String>, text: String },
}

pub struct Personal {
    store: Store,
    told: bool,
}

impl Personal {
    /// The memory file at `path` (its directory created 0700, the file 0600), or none for
    /// `--no-memory`. An error opening it is kept as the notice, not raised (Ch. 8.3).
    pub fn open(path: Option<&Path>) -> Self {
        let store = match path {
            None => Store::Off("memory off (--no-memory)".into()),
            Some(p) => match crate::memory::open(p) {
                Ok(m) => Store::On(m),
                Err(e) => Store::Off(format!(
                    "memory unavailable ({e}); answering without personalization"
                )),
            },
        };
        Self { store, told: false }
    }

    /// Handles a correction, or returns the memory context to show before routing.
    pub fn before(
        &mut self,
        req: &Request,
        now: i64,
        confirm: &mut dyn FnMut(&str) -> bool,
    ) -> Before {
        // A taught alias first: "no, by short notes I mean ..." is teaching, not a style correction.
        if let Some(a) = alias::detect(&req.text) {
            let Store::On(m) = &mut self.store else {
                return Before::Correction(vec![format!(
                    "memory unavailable: \"{}\" = \"{}\" is not kept",
                    a.phrase, a.target
                )]);
            };
            if !confirm(&alias::question(&a)) {
                return Before::Correction(vec!["not saved".into()]);
            }
            return Before::Correction(vec![match alias::save(m, &a, now) {
                Ok(_) => format!(
                    "saved: when you say \"{}\" you mean \"{}\"",
                    a.phrase, a.target
                ),
                Err(e) => format!("not saved: {e}"),
            }]);
        }
        match correction::detect(&req.text) {
            Action::NotACorrection => {}
            Action::ApplyOnce(c) => {
                return Before::Correction(vec![format!(
                    "this turn only: {} = {}",
                    c.key, c.value
                )]);
            }
            Action::StoreAfterConfirm(c) => {
                let Store::On(m) = &mut self.store else {
                    return Before::Correction(vec![format!(
                        "memory unavailable: {} = {} applies to this turn only",
                        c.key, c.value
                    )]);
                };
                if !confirm(&correction::question(&c)) {
                    return Before::Correction(vec!["not saved".into()]);
                }
                let saved = match &c.exception {
                    Some(e) => format!("{} = {} (unless {e})", c.key, c.value),
                    None => format!("{} = {}", c.key, c.value),
                };
                return Before::Correction(vec![match correction::save(m, &c, now) {
                    Ok(_) => format!("saved: {saved}"),
                    Err(e) => format!("not saved: {e}"),
                }]);
            }
        }
        let mut lines = Vec::new();
        let mut text = req.text.clone();
        match &self.store {
            Store::Off(why) if !self.told => {
                self.told = true;
                lines.push(why.clone());
            }
            Store::Off(_) => {}
            Store::On(m) => {
                let words = |s: &str| s.split_whitespace().count();
                let found = m
                    .context(&req.text, now, CONTEXT_K, CONTEXT_WORDS, &words)
                    .and_then(|ctx| Ok((ctx, alias::aliases(m, now)?)));
                match found {
                    Ok((ctx, aliases)) => {
                        // Aliases are applied, not shown as context lines.
                        let shown = ctx.text.lines().filter(|l| !l.contains("] alias_"));
                        lines.extend(shown.map(|l| format!("memory: {l}")));
                        let resolved = alias::resolve(&req.text, &aliases);
                        if resolved != req.text {
                            lines.push(format!("with your aliases: {resolved}"));
                            text = resolved;
                        }
                    }
                    Err(e) => lines.push(format!(
                        "memory unavailable ({e}); this request goes without it"
                    )),
                }
            }
        }
        Before::Route { lines, text }
    }

    /// Stores the preference of a request accepted as `set_preference` (the template planner's key
    /// and value); nothing else is remembered.
    pub fn after(&mut self, req: &Request, outcome: &Outcome, now: i64) -> Vec<String> {
        let (Outcome::Accepted(p), Store::On(m)) = (outcome, &mut self.store) else {
            return vec![];
        };
        if p.intent != Intent::SetPreference {
            return vec![];
        }
        let ctx = templates::Context {
            today: Date::from_days(now.div_euclid(86_400)),
            documents: &[],
        };
        let Proposal::Plan(plan) = templates::propose(Intent::SetPreference, &req.text, &ctx)
        else {
            return vec![];
        };
        let args = &plan["nodes"][0]["args"];
        let (Some(key), Some(value)) = (args["key"].as_str(), args["value"].as_str()) else {
            return vec![];
        };
        let d = Draft::new(Kind::Preference, key, value).importance(4);
        vec![match m.propose(&d, Source::UserExplicit, now) {
            Ok(_) => format!("saved: {key} = {value}"),
            Err(e) => format!("not saved: {e}"),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::{Prediction, Tier};
    use kestrel_memory::Filter;

    const NOW: i64 = 1_792_713_600; // 2026-10-23T00:00:00Z

    fn db(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("kestrel_personal_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("memory.db")
    }

    fn items(p: &Personal) -> Vec<(String, String, Source)> {
        match &p.store {
            Store::On(m) => m
                .items(Filter::All)
                .unwrap()
                .into_iter()
                .map(|i| (i.key, i.value, i.source))
                .collect(),
            Store::Off(_) => vec![],
        }
    }

    fn route(lines: Vec<&str>, text: &str) -> Before {
        Before::Route {
            lines: lines.into_iter().map(String::from).collect(),
            text: text.into(),
        }
    }

    fn accepted(intent: Intent, confidence: f32) -> Outcome {
        Outcome::Accepted(Prediction {
            intent,
            confidence,
            tier: Tier::Rules,
        })
    }

    #[test]
    fn a_durable_correction_is_saved_only_after_a_yes() {
        let mut p = Personal::open(Some(&db("yes")));
        let req = Request::new(1, "No, I prefer concise answers unless I ask for details.");
        let mut asked = Vec::new();
        let mut no = |q: &str| {
            asked.push(q.to_string());
            false
        };
        assert_eq!(
            p.before(&req, NOW, &mut no),
            Before::Correction(vec!["not saved".into()])
        );
        assert_eq!(
            asked,
            ["Save: answer_length = short (unless i ask for details)?"]
        );
        assert!(items(&p).is_empty());
        let out = p.before(&req, NOW, &mut |_| true);
        assert_eq!(
            out,
            Before::Correction(vec![
                "saved: answer_length = short (unless i ask for details)".into()
            ])
        );
        assert_eq!(
            items(&p),
            [("answer_length".into(), "short".into(), Source::Correction)]
        );
    }

    #[test]
    fn a_one_off_correction_applies_to_this_turn_and_stores_nothing() {
        let mut p = Personal::open(Some(&db("once")));
        let out = p.before(
            &Request::new(1, "No, just this time keep it short"),
            NOW,
            &mut |_| panic!("a one-off is never asked"),
        );
        assert_eq!(
            out,
            Before::Correction(vec!["this turn only: answer_length = short".into()])
        );
        assert!(items(&p).is_empty());
    }

    #[test]
    fn a_stated_preference_is_stored_and_shapes_the_next_context() {
        let mut p = Personal::open(Some(&db("pref")));
        let req = Request::new(1, "I prefer short answers");
        assert_eq!(
            p.before(&req, NOW, &mut |_| true),
            route(vec![], "i prefer short answers")
        );
        assert_eq!(
            p.after(&req, &accepted(Intent::SetPreference, 0.99), NOW),
            ["saved: answer_length = short"]
        );
        assert_eq!(
            items(&p),
            [("answer_length".into(), "short".into(), Source::UserExplicit)]
        );
        let next = Request::new(2, "summarize my last three meeting notes");
        assert_eq!(
            p.before(&next, NOW + 60, &mut |_| true),
            route(
                vec!["memory: [pref|user_explicit|2026-10-23] answer_length: short"],
                "summarize my last three meeting notes"
            )
        );
    }

    #[test]
    fn only_an_accepted_set_preference_writes_memory() {
        let mut p = Personal::open(Some(&db("other")));
        let req = Request::new(1, "I prefer short answers");
        let abstained = Outcome::Abstained {
            best: None,
            reason: kestrel_router::AbstainReason::LowConfidence,
        };
        assert!(p.after(&req, &abstained, NOW).is_empty());
        assert!(
            p.after(&req, &accepted(Intent::SearchNotes, 0.99), NOW)
                .is_empty()
        );
        assert!(items(&p).is_empty());
    }

    #[test]
    fn memory_unavailable_means_one_notice_and_no_personalization() {
        // Ch. 8.3 fault injection: the memory file's directory is a regular file.
        let blocker = db("blocked").with_file_name("not-a-dir");
        std::fs::write(&blocker, b"x").unwrap();
        let mut p = Personal::open(Some(&blocker.join("memory.db")));
        let req = Request::new(1, "summarize my notes");
        let Before::Route { lines: first, .. } = p.before(&req, NOW, &mut |_| true) else {
            panic!("routed")
        };
        assert_eq!(first.len(), 1);
        assert!(first[0].starts_with("memory unavailable"), "{first:?}");
        assert_eq!(
            p.before(&req, NOW, &mut |_| true),
            route(vec![], "summarize my notes"),
            "said once"
        );
        let fix = Request::new(2, "No, I prefer concise answers");
        assert_eq!(
            p.before(&fix, NOW, &mut |_| panic!("nothing to save into")),
            Before::Correction(vec![
                "memory unavailable: answer_length = short applies to this turn only".into()
            ])
        );
        let pref = Request::new(3, "I prefer short answers");
        assert!(
            p.after(&pref, &accepted(Intent::SetPreference, 0.99), NOW)
                .is_empty()
        );
    }

    #[test]
    fn memory_can_be_switched_off() {
        let mut p = Personal::open(None);
        let Before::Route { lines, .. } = p.before(&Request::new(1, "hi"), NOW, &mut |_| true)
        else {
            panic!("routed")
        };
        assert_eq!(lines, ["memory off (--no-memory)"]);
    }

    #[test]
    fn a_taught_alias_is_saved_after_a_yes_and_resolves_later_requests() {
        let mut p = Personal::open(Some(&db("alias")));
        let teach = Request::new(1, "When I say standup doc, I mean team sync");
        let mut asked = Vec::new();
        let out = p.before(&teach, NOW, &mut |q| {
            asked.push(q.to_string());
            true
        });
        assert_eq!(
            asked,
            ["Save: when you say \"standup doc\" you mean \"team sync\"?"]
        );
        assert_eq!(
            out,
            Before::Correction(vec![
                "saved: when you say \"standup doc\" you mean \"team sync\"".into()
            ])
        );
        let next = Request::new(2, "open my standup doc");
        assert_eq!(
            p.before(&next, NOW + 60, &mut |_| true),
            route(
                vec!["with your aliases: open my team sync"],
                "open my team sync"
            )
        );
        let no = Request::new(3, "by the gang I mean riya and kabir");
        assert_eq!(
            p.before(&no, NOW, &mut |_| false),
            Before::Correction(vec!["not saved".into()])
        );
    }

    #[test]
    fn without_memory_an_alias_is_not_kept() {
        let mut p = Personal::open(None);
        let out = p.before(
            &Request::new(1, "by the gang I mean riya and kabir"),
            NOW,
            &mut |_| panic!("nothing to save into"),
        );
        assert_eq!(
            out,
            Before::Correction(vec![
                "memory unavailable: \"the gang\" = \"riya and kabir\" is not kept".into()
            ])
        );
    }
}
