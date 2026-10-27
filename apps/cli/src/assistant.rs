//! One request through the assistant (ADR-034): what `kestrel ask` and `kestrel serve` share, so
//! the terminal and the always-on service answer alike. In order: ", also" / ", now" join two
//! requests and chat spellings are normalized (src/hinglish.rs); a taught alias or a correction is handled (src/personal.rs); a task command (list,
//! done, rename, reschedule) runs on the memory file (src/commands.rs); otherwise the cascade
//! routes the request, and with a notes folder the routed parts are planned and carried out
//! (src/act.rs). In the service, a request for a v3 skill (chat, email, rewrite, translate) is
//! written before the cascade (src/kinds.rs). The answer comes back as lines; the caller prints
//! them or sends them. `handle_streaming` also hands a written skill's text over as it comes.
//! With a meaning index (v3, ADR-039), a notes search the keywords miss also finds notes by meaning.
//! "add this to notes" (v3, src/notes_save.rs) comes first, on the text as typed.

use std::time::Instant;

use kestrel_core::{Intent, Request};
use kestrel_router::{Cascade, Outcome};

use crate::act::{Act, Prompt};
use crate::ask::{Log, render};
use crate::personal::{Before, Personal};
use crate::telemetry::Sink;

pub struct Assistant<'a> {
    cascade: Cascade<'a, Intent>,
    log: Log,
    personal: Personal,
    act: Option<Act>,
    sink: Sink,
    brief: bool,
    n: u64,
    kinds: Option<crate::kinds::Kinds<'a>>,
    meaning: Option<Box<dyn crate::meaning::Meaning + 'a>>,
}

impl<'a> Assistant<'a> {
    /// `log` is the one the cascade's recorders write to.
    pub fn new(
        cascade: Cascade<'a, Intent>,
        log: Log,
        personal: Personal,
        act: Option<Act>,
        sink: Sink,
        brief: bool,
    ) -> Self {
        Self {
            cascade,
            log,
            personal,
            act,
            sink,
            brief,
            n: 0,
            kinds: None,
            meaning: None,
        }
    }

    /// v3 (ADR-039): notes searches also find notes by meaning.
    pub fn with_meaning(mut self, meaning: Box<dyn crate::meaning::Meaning + 'a>) -> Self {
        self.meaning = Some(meaning);
        self
    }

    /// v3: requests for a skill (chat, email, rewrite, translate) are written before the cascade.
    pub fn with_kinds(mut self, kinds: crate::kinds::Kinds<'a>) -> Self {
        if let Some(act) = self.act.as_mut() {
            act.also_writes();
        }
        self.kinds = Some(kinds);
        self
    }

    pub fn handle(&mut self, text: &str, confirm: &mut dyn FnMut(&str) -> bool) -> Vec<String> {
        self.respond(text, confirm, None)
    }

    /// As `handle`, with a written skill's text handed to `part` while Gemma writes it; the lines
    /// are then what follows it (src/kinds.rs).
    pub fn handle_streaming(
        &mut self,
        text: &str,
        confirm: &mut dyn FnMut(&str) -> bool,
        part: &mut dyn FnMut(&str),
    ) -> Vec<String> {
        self.respond(text, confirm, Some(part))
    }

    fn respond(
        &mut self,
        text: &str,
        confirm: &mut dyn FnMut(&str) -> bool,
        part: Option<&mut dyn FnMut(&str)>,
    ) -> Vec<String> {
        self.n += 1;
        let n = self.n;
        let mut out = Vec::new();
        self.log.borrow_mut().clear();
        // ", also" / ", now" join two requests the way "and" does, and chat spellings ("krna",
        // "kl") become the ones the rules know (ADR-034).
        let typed = crate::hinglish::normalize(&crate::commands::join_words(text));
        let said = Request::new(n, &typed);
        let unix_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let now = i64::try_from(unix_s).unwrap_or(i64::MAX);
        // v3: "add this to notes" reads the text as typed, before anything takes it for a request.
        if let Some(act) = self.act.as_ref()
            && let Some(asked) = crate::notes_save::parse(text)
        {
            return act.save_note(asked, now, confirm);
        }
        // The request routed is the one with the user's aliases resolved.
        let req = match self.personal.before(&said, now, confirm) {
            Before::Correction(lines) => return lines,
            Before::Route { lines, text } => {
                // --brief leaves out the memory context; alias resolutions and notices stay.
                out.extend(
                    lines
                        .into_iter()
                        .filter(|l| !(self.brief && l.starts_with("memory: "))),
                );
                Request::new(n, &text)
            }
        };
        // Task commands (list, done, rename, reschedule) are the app's, before the cascade.
        if let Some(act) = self.act.as_mut()
            && let Some(cmd) = crate::commands::parse(&req.text, act.today(now))
        {
            out.extend(act.command(&cmd, now));
            return out;
        }
        // v3 (ADR-037): a request for a skill is written now, from the text as typed.
        let meaning = (self.meaning.as_deref_mut()).map(|m| m as &mut dyn crate::meaning::Meaning);
        if let Some(kinds) = self.kinds.as_ref()
            && let Some(lines) = kinds.handle(&req, text, confirm, part, meaning)
        {
            out.extend(lines);
            return out;
        }
        let t0 = Instant::now();
        let (outcome, trace) = self.cascade.route(&req);
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        if !self.brief {
            let accept = self.cascade.config().accept;
            let report = render(&req, &self.log.borrow(), &outcome, &trace, accept, ms);
            out.extend(report.lines().map(String::from));
        }
        // Acting, the plan's update_preference step stores a stated preference; otherwise this does.
        if self.act.is_none() {
            out.extend(self.personal.after(&req, &outcome, now));
        }
        if self.sink.is_on() {
            let peak = kestrel_bench::rss::peak_rss_kib();
            let rec =
                crate::telemetry::record(&self.log.borrow(), &outcome, &trace, ms, unix_s, peak);
            self.sink.write(&rec);
        }
        if let Some(act) = self.act.as_mut() {
            let routed = routed_parts(&self.cascade, &req, &outcome, n);
            let prompt = Prompt(std::cell::RefCell::new(confirm));
            let meaning = self
                .meaning
                .as_deref_mut()
                .map(|m| m as &mut dyn crate::meaning::Meaning);
            out.extend(act.carry_out_with(routed.as_deref(), n, now, &prompt, meaning));
        }
        out
    }
}

/// What `act` plans from: a request that joins several (the routing signal) is split and each part
/// routed (ADR-025); otherwise the routed request itself. None when the cascade, or a part, abstained.
pub fn routed_parts(
    cascade: &Cascade<'_, Intent>,
    req: &Request,
    outcome: &Outcome<Intent>,
    id: u64,
) -> Option<Vec<(Intent, String)>> {
    if kestrel_core::features::has_conjunction(&req.text) {
        let parts = kestrel_tools::compound::split(&req.text);
        if parts.len() > 1 {
            return parts
                .into_iter()
                .enumerate()
                .map(
                    |(k, part)| match cascade.route(&Request::new(id * 100 + k as u64, &part)).0 {
                        Outcome::Accepted(p) => Some((p.intent, part)),
                        Outcome::Abstained { .. } => None,
                    },
                )
                .collect();
        }
    }
    match outcome {
        Outcome::Accepted(p) => Some(vec![(p.intent, req.text.clone())]),
        Outcome::Abstained { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ask::Recorder;
    use crate::kinds::{Kind, Kinds, TAU, Writer};
    use kestrel_core::{KestrelError, Tier, TierModel};
    use kestrel_router::RouterConfig;

    /// Tier 3 that names an intent by the request's first word ("remind" -> create_task, ...).
    struct ByFirstWord;

    impl TierModel for ByFirstWord {
        fn tier(&self) -> Tier {
            Tier::Encoder
        }
        fn est_cost_ms(&self) -> u32 {
            1
        }
        fn classify(&self, req: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
            let intent = match req.text.split(' ').next() {
                Some("remind") => Intent::CreateTask,
                Some("search") => Intent::SearchNotes,
                _ => Intent::Chitchat,
            };
            Ok(vec![(intent, 0.99)])
        }
    }

    fn assistant(name: &str, brief: bool) -> (Assistant<'static>, std::path::PathBuf) {
        let d =
            std::env::temp_dir().join(format!("kestrel_assistant_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("notes")).unwrap();
        std::fs::write(
            d.join("notes/2026-10-24-standup.md"),
            "tags: meeting\nStandup.\n- [ ] send the report\n",
        )
        .unwrap();
        let log = Log::default();
        let tiers: Vec<Box<dyn TierModel>> =
            vec![Box::new(Recorder::new(Box::new(ByFirstWord), log.clone()))];
        let cfg = RouterConfig {
            accept: [1.0, 1.0, 0.9, 0.0],
            latency_budget_ms: 1000,
            multi_intent_to_generative: true,
        };
        let act = Act::open(&d.join("notes"), &d.join("memory.db")).unwrap();
        let a = Assistant::new(
            Cascade::new(tiers, cfg).unwrap(),
            log,
            Personal::open(None),
            Some(act),
            Sink::open(None),
            brief,
        );
        (a, d)
    }

    fn yes() -> impl FnMut(&str) -> bool {
        |_| true
    }

    /// The kind head by first word: "draft" is a sure email, "maybe" an unsure rewrite, "who" a
    /// sure notes question.
    struct KindByFirstWord;

    impl TierModel<Kind> for KindByFirstWord {
        fn tier(&self) -> Tier {
            Tier::Encoder
        }
        fn est_cost_ms(&self) -> u32 {
            1
        }
        fn classify(&self, req: &Request) -> Result<Vec<(Kind, f32)>, KestrelError> {
            Ok(match req.text.split(' ').next() {
                Some("draft") => vec![(Kind::Email, 0.9), (Kind::App, 0.1)],
                Some("maybe") => vec![(Kind::Rewrite, 0.5), (Kind::App, 0.4)],
                Some("who") => vec![(Kind::Notes, 0.9), (Kind::App, 0.1)],
                _ => vec![(Kind::App, 0.95)],
            })
        }
    }

    /// Writes "<kind> draft of: <request>".
    struct Drafts;

    impl Writer for Drafts {
        fn write(
            &self,
            kind: Kind,
            request: &str,
            _: &mut dyn FnMut(&str),
        ) -> Result<String, KestrelError> {
            Ok(format!("{} draft of: {request}", kind.label()))
        }

        fn answer(
            &self,
            _: &str,
            notes: &[(&str, &str)],
        ) -> Result<crate::notes_qa::NotesAnswer, KestrelError> {
            Ok(crate::notes_qa::NotesAnswer {
                text: format!("Asked about {} notes.", notes.len()),
                ungrounded: None,
            })
        }
    }

    #[test]
    fn a_skill_is_written_before_the_cascade_and_other_requests_take_the_old_path() {
        let (a, _) = assistant("kinds", true);
        let mut a = a.with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU));
        // The first request also carries the session's one memory notice.
        let out = a.handle("remind me to call asha tomorrow", &mut yes());
        assert!(
            out.iter()
                .any(|l| l.starts_with("created task #1: call asha")),
            "{out:?}"
        );
        assert_eq!(
            a.handle("Draft a mail to Asha: Leave on Friday!", &mut yes()),
            ["email draft of: Draft a mail to Asha: Leave on Friday!"]
        );
        // Unsure: a "no" keeps the old path, a "yes" writes.
        let out = a.handle("maybe shorten this: Hello There", &mut |_| false);
        assert!(!out.iter().any(|l| l.contains("draft of")), "{out:?}");
        assert_eq!(
            a.handle("maybe shorten this: Hello There", &mut yes()),
            ["rewrite draft of: maybe shorten this: Hello There"]
        );
    }

    #[test]
    fn a_streamed_skill_comes_in_parts_and_the_old_path_in_lines() {
        let (a, _) = assistant("stream", true);
        let mut a = a.with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU));
        let mut parts: Vec<String> = Vec::new();
        let out = a.handle_streaming("remind me to call asha tomorrow", &mut yes(), &mut |p| {
            parts.push(p.into())
        });
        assert!(parts.is_empty(), "{parts:?}");
        assert!(
            out.iter()
                .any(|l| l.starts_with("created task #1: call asha")),
            "{out:?}"
        );
        let out = a.handle_streaming("draft a mail to asha", &mut yes(), &mut |p| {
            parts.push(p.into())
        });
        assert_eq!(parts, ["email draft of: draft a mail to asha"]);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn with_the_skills_on_small_talk_names_them_too() {
        let (mut plain, _) = assistant("greet_plain", true);
        let out = plain.handle("hello", &mut yes());
        assert_eq!(
            out.last().unwrap(),
            "Hi! I can find, open and summarize your notes, and keep your tasks."
        );
        let (a, _) = assistant("greet_kinds", true);
        let mut a = a.with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU));
        let out = a.handle("hello", &mut yes());
        assert_eq!(
            out.last().unwrap(),
            "Hi! I can find, open and summarize your notes and answer questions from them, keep \
             your tasks, draft emails and messages, rewrite or translate text, write and fix small \
             Python code, and chat."
        );
    }

    /// Finds the standup note by meaning for every query.
    struct Standup;

    impl crate::meaning::Meaning for Standup {
        fn find(&mut self, _: &str) -> Vec<kestrel_tools::tools::Note> {
            vec![kestrel_tools::tools::Note {
                name: "2026-10-24-standup.md".into(),
                text: "Standup.".into(),
            }]
        }
        fn k(&self) -> usize {
            3
        }
        fn closest(&mut self, q: &str, n: usize) -> Vec<kestrel_tools::tools::Note> {
            assert_eq!((q, n), ("who runs the standup?", crate::notes_qa::N));
            self.find(q)
        }
    }

    #[test]
    fn a_notes_question_is_answered_from_the_closest_notes_and_needs_them() {
        let (a, _) = assistant("notes_qa", true);
        let mut a = a.with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU));
        // Without notes by meaning the question takes the old path.
        let out = a.handle("who runs the standup?", &mut yes());
        assert!(!out.iter().any(|l| l.starts_with("Asked about")), "{out:?}");
        let mut a = a.with_meaning(Box::new(Standup));
        assert_eq!(
            a.handle("who runs the standup?", &mut yes()),
            ["Asked about 1 notes.", "(from 2026-10-24-standup.md)"]
        );
    }

    #[test]
    fn a_search_the_keywords_miss_finds_notes_by_meaning() {
        let (a, _) = assistant("meaning", true);
        let mut a = a.with_meaning(Box::new(Standup));
        let out = a.handle("search my notes for saving money", &mut yes());
        assert_eq!(
            out[out.len() - 2..],
            ["found 1 note:", "- 2026-10-24-standup.md (by meaning)"]
        );
    }

    #[test]
    fn add_this_to_notes_saves_the_text_as_typed_after_a_yes() {
        let (mut a, d) = assistant("save", true);
        let long = "The sources cover SVMs and k-Means, and also Adam and AdamW. add this to notes";
        let mut asked = Vec::new();
        let out = a.handle(long, &mut |q| {
            asked.push(q.to_string());
            false
        });
        assert_eq!(out, ["Not saved."]);
        assert!(
            asked[0].starts_with("Save this as a new note, "),
            "{asked:?}"
        );
        let out = a.handle(long, &mut yes());
        let name = out[0].strip_prefix("Saved to your notes: ").expect("saved");
        assert!(
            name.ends_with("-the-sources-cover-svms-and-kmeans-and-also.md"),
            "{name}"
        );
        let saved = std::fs::read_to_string(d.join("notes").join(name)).unwrap();
        assert_eq!(
            saved,
            "The sources cover SVMs and k-Means, and also Adam and AdamW.\n"
        );
        assert_eq!(
            a.handle("add this to my notes", &mut yes()),
            [
                "What should I save? Put the text before or after it, for example: add to notes: call the bank on monday"
            ]
        );
    }

    #[test]
    fn requests_and_task_commands_share_one_memory_file() {
        let (mut a, _) = assistant("share", true);
        let out = a.handle("remind me to call asha tomorrow", &mut yes());
        assert!(
            out.iter()
                .any(|l| l.starts_with("created task #1: call asha")),
            "{out:?}"
        );
        let out = a.handle("show my tasks", &mut yes());
        assert!(out.iter().any(|l| l.contains("#1 call asha")), "{out:?}");
        // Typed the way people text: "hogya" is "ho gaya".
        assert_eq!(a.handle("task 1 hogya", &mut yes()), ["done: #1 call asha"]);
        assert_eq!(a.handle("show my tasks", &mut yes()), ["no tasks"]);
    }

    #[test]
    fn also_joins_two_requests_and_brief_hides_the_tier_report() {
        let (mut a, _) = assistant("join", true);
        let out = a.handle(
            "search my notes for report, also remind me to buy milk",
            &mut yes(),
        );
        assert!(
            out.iter().any(|l| l.contains("2026-10-24-standup.md")),
            "{out:?}"
        );
        assert!(
            out.iter().any(|l| l.contains("created task #1: buy milk")),
            "{out:?}"
        );
        assert!(
            !out.iter().any(|l| l.contains("=>")),
            "brief has no tier report: {out:?}"
        );
        let (mut full, _) = assistant("full", false);
        let out = full.handle("remind me to call ravi", &mut yes());
        assert!(out.iter().any(|l| l.contains("=> create_task")), "{out:?}");
    }
}
