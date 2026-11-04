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
    sender: Option<crate::send::Sender>,
    online: Option<crate::online::Online>,
    /// How the last answer was made (src/why.rs).
    why: Option<crate::why::Why>,
    /// The last research, until "save research" keeps it (src/research.rs).
    research: Option<String>,
    /// The way this request took, for the daily counts (kestrel-memory's usage.rs).
    took: Option<&'static str>,
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
            sender: None,
            online: None,
            why: None,
            research: None,
            took: None,
        }
    }

    /// v4: the weather and the news, through `kestrel-net` (src/online.rs).
    pub fn with_online(mut self, online: crate::online::Online) -> Self {
        self.online = Some(online);
        self
    }

    /// v4: a draft can leave, after a yes, through `kestrel-net` (src/send.rs).
    pub fn with_sender(mut self, sender: crate::send::Sender) -> Self {
        self.sender = Some(sender);
        self
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
        self.took = None;
        let lines = self.answer(text, confirm, part);
        if let Some(act) = self.act.as_mut() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            act.count_use(now, "request");
            if let Some(took) = self.took {
                act.count_use(now, took);
            }
        }
        lines
    }

    fn answer(
        &mut self,
        text: &str,
        confirm: &mut dyn FnMut(&str) -> bool,
        mut part: Option<&mut dyn FnMut(&str)>,
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
        // v4.1: "why this answer": told from what happened, and it leaves the last answer's story alone.
        if crate::why::parse(text) {
            self.took = Some("why");
            let left = match (&self.why, &self.act) {
                (Some(why), Some(act)) => act.left_since(why.at),
                _ => Vec::new(),
            };
            return crate::why::lines(self.why.as_ref(), &left);
        }
        use crate::why::Why;
        // v3: "add this to notes" reads the text as typed, before anything takes it for a request.
        if let Some(act) = self.act.as_ref()
            && let Some(asked) = crate::notes_save::parse(text)
        {
            self.took = Some("note_saved");
            self.why = Some(Why::new(
                now,
                "By rules: you asked to save text as a note, no model.",
            ));
            return act.save_note(asked, now, confirm);
        }
        // v4: "mera PAN kahan hai": rules over the notes folder, nothing for the model.
        if let Some(act) = self.act.as_mut()
            && let Some(asked) = crate::documents::parse(text)
        {
            self.took = Some("documents");
            self.why = Some(Why::new(
                now,
                "By rules, from the documents in your notes folder, no model.",
            ));
            return act.about_documents(asked, now, confirm);
        }
        // v4: "file this ticket ~/Downloads/irctc.pdf": read by rules, filed after a yes.
        if let Some(act) = self.act.as_mut()
            && let Some(path) = crate::ticket::parse(text)
        {
            self.took = Some("ticket");
            self.why = Some(Why::new(
                now,
                "By rules: the journey was read from the PDF's text, no model.",
            ));
            return act.file_ticket(&path, now, confirm);
        }
        // v4: "send it to Ravi": the last draft leaves after a yes, through kestrel-net (src/send.rs).
        if let (Some(sender), Some(act)) = (self.sender.as_ref(), self.act.as_mut())
            && let Some(said) = crate::send::parse(text)
        {
            self.took = Some("mail");
            let draft = self.kinds.as_ref().and_then(|k| k.draft());
            self.why = Some(Why::new(
                now,
                "By rules: your words named a draft and a contact, and it went only after your yes.",
            ));
            return act.send_or_keep(sender, said, draft.as_deref(), now, confirm);
        }
        // v4: "send an email to Anup about Friday": written, then the same yes as for any draft.
        if let (Some(sender), Some(kinds), Some(act)) =
            (self.sender.as_ref(), self.kinds.as_ref(), self.act.as_mut())
            && let Some(asked) = crate::send::compose(text, &act.contact_names())
        {
            self.took = Some("mail");
            self.why = Some(Why::new(
                now,
                "By rules: your words asked for a mail to a contact. The model wrote it, and it went only after your yes.",
            ));
            let to = crate::send::Said::Send {
                to: Some(asked.to.clone()),
            };
            if !asked.has_words {
                let draft = kinds.draft();
                return act.send_or_keep(sender, to, draft.as_deref(), now, confirm);
            }
            let known = act.contact_names();
            if !known.iter().any(|c| c.eq_ignore_ascii_case(&asked.to)) {
                return vec![crate::send::not_a_contact(&asked.to)];
            }
            let mut lines = kinds.email(text, part);
            if let Some(draft) = kinds.draft() {
                lines.extend(act.send_or_keep(sender, to, Some(&draft), now, confirm));
            }
            return lines;
        }
        // v4: the weather and the news: rules, and what comes back is shown, never acted on.
        if let (Some(online), Some(act)) = (self.online.as_ref(), self.act.as_mut())
            && let Some(asked) = crate::online::parse(text)
        {
            self.took = Some(match asked {
                crate::online::Asked::Weather(_) => "weather",
                crate::online::Asked::News => "news",
                crate::online::Asked::SetPlace(_) | crate::online::Asked::AddFeed(_) => {
                    "online_choice"
                }
            });
            self.why = Some(Why::new(
                now,
                "By rules: this came from the services you chose, shown as they said it, no model.",
            ));
            return act.online(online, asked, now);
        }
        // v4: the morning brief, on asking.
        if let (Some(online), Some(act)) = (self.online.as_ref(), self.act.as_mut())
            && crate::brief::parse(text)
        {
            self.took = Some("brief");
            self.why = Some(Why::new(
                now,
                "By rules: your tasks and documents, and what you chose from the internet, no model.",
            ));
            return act.brief(online, now);
        }
        // v4.1: research: the same rules as a web search, page by page; a note only on asking.
        if let Some(act) = self.act.as_ref()
            && crate::research::asks_to_save(text)
        {
            self.took = Some("research_saved");
            return match self.research.take() {
                Some(note) => act.save_note(crate::notes_save::Asked::Save(note), now, confirm),
                None => vec!["There is no research to save yet.".to_string()],
            };
        }
        if let (Some(online), Some(act)) = (self.online.as_ref(), self.act.as_mut())
            && let Some(topic) = crate::research::parse(text)
        {
            self.took = Some("research");
            self.why = Some(Why::new(
                now,
                "Your words went to your search provider, and the model read each page on its own, with nothing of yours beside it.",
            ));
            let writer = self.kinds.as_ref().map(|k| k.writer());
            let found = act.research(online, &topic, writer, now);
            self.research = found.note;
            return found.lines;
        }
        // v4: a web search: the person's words go out, and the answer is only shown.
        if let (Some(online), Some(act)) = (self.online.as_ref(), self.act.as_mut())
            && let Some(query) = crate::web_search::parse(text)
        {
            self.took = Some("web_search");
            self.why = Some(Why::new(
                now,
                "Your words went to your search provider, and the model answered only from what came back.",
            ));
            let writer = self.kinds.as_ref().map(|k| k.writer());
            return act.web_search(online, &query, writer, now);
        }
        // The request routed is the one with the user's aliases resolved.
        let req = match self.personal.before(&said, now, confirm) {
            Before::Correction(lines) => {
                self.took = Some("correction");
                self.why = Some(Why::new(
                    now,
                    "By rules: a correction or a word you taught, no model.",
                ));
                return lines;
            }
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
            self.took = Some("task_command");
            out.extend(act.command(&cmd, now));
            self.why = Some(Why::new(now, "By rules: a task command, no model."));
            return out;
        }
        // A task named by its words: "bill bharna ho gaya", "reschedule doctor to monday".
        if let Some(act) = self.act.as_mut()
            && let Some(lines) = act.task_by_name(&req.text, now, confirm)
        {
            self.took = Some("task_command");
            out.extend(lines);
            self.why = Some(Why::new(
                now,
                "By rules: your words named a task, no model.",
            ));
            return out;
        }
        // v4: what the person says about themselves is kept, and asked back, without the model.
        if let Some(act) = self.act.as_mut()
            && let Some(lines) = act.about_the_person(text, now)
        {
            self.took = Some("fact");
            out.extend(lines);
            self.why = Some(Why::new(
                now,
                "By rules: what you told Kestrel to remember, word for word, no model.",
            ));
            return out;
        }
        // v3 (ADR-037): a request for a skill is written now, from the text as typed.
        let meaning = (self.meaning.as_deref_mut()).map(|m| m as &mut dyn crate::meaning::Meaning);
        // `part` is still wanted below when no skill takes this.
        let streams = part.is_some();
        let mut forward = |piece: &str| {
            if let Some(show) = part.as_mut() {
                show(piece);
            }
        };
        let stream: Option<&mut dyn FnMut(&str)> = streams.then_some(&mut forward);
        if let Some(kinds) = self.kinds.as_ref()
            && let Some(lines) = kinds.handle(&req, text, confirm, stream, meaning)
        {
            self.took = Some(
                kinds
                    .took()
                    .map_or("new_conversation", crate::kinds::Kind::label),
            );
            out.extend(lines);
            // A draft on its own: what to say to send it.
            if self.sender.is_some()
                && kinds.took() == Some(crate::kinds::Kind::Email)
                && kinds.draft().is_some()
                && let Some(act) = self.act.as_ref()
            {
                out.push(crate::send::next_step(text, &act.contact_names()));
            }
            let way = kinds
                .why()
                .unwrap_or_else(|| "It was written by the model.".to_string());
            self.why = Some(Why::new(now, &way));
            return out;
        }
        let t0 = Instant::now();
        let (outcome, trace) = self.cascade.route(&req);
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        self.took = Some(match &outcome {
            Outcome::Accepted(p) => p.intent.label(),
            Outcome::Abstained { .. } => "not_understood",
        });
        let accept = self.cascade.config().accept;
        let report = render(&req, &self.log.borrow(), &outcome, &trace, accept, ms);
        if !self.brief {
            out.extend(report.lines().map(String::from));
        }
        self.why = Some(Why::new(now, &crate::why::routed(&outcome)));
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
        // A question or a remark nothing else took: the notes if they answer it, else a chat. A search
        // in question's clothes ("who shares the plan") gets the answer first, and stays a search.
        if let Some(kinds) = self.kinds.as_ref()
            && crate::kinds::conversational(text)
        {
            let unplaced = match &outcome {
                Outcome::Accepted(p) => p.intent == Intent::Unsupported,
                Outcome::Abstained { .. } => true,
            };
            let search =
                matches!(&outcome, Outcome::Accepted(p) if p.intent == Intent::SearchNotes);
            if unplaced || search {
                let n = crate::notes_qa::N;
                let mut notes = (self.meaning.as_deref_mut())
                    .map_or_else(Vec::new, |m| m.closest(text.trim(), n));
                if let (true, Some(act)) = (notes.is_empty(), self.act.as_ref()) {
                    notes = act.notes_sharing(text, n);
                }
                if let Some(lines) = kinds.answer_from_notes(text, &notes) {
                    self.took = Some("notes");
                    self.why = Some(Why::new(
                        now,
                        "I read this as a question about your notes, and it was answered by the model from your closest notes.",
                    ));
                    out.extend(lines);
                    return out;
                }
            }
            if unplaced {
                self.took = Some("chat");
                self.why = Some(Why::new(
                    now,
                    "I read this as a chat, and it was written by the model.",
                ));
                out.extend(kinds.chat(text, part));
                return out;
            }
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
                Some("recall") => Intent::RetrieveMemory,
                Some("shrug") => return Ok(vec![]),
                Some("how" | "book" | "whose" | "i") => Intent::Unsupported,
                Some("whom") => Intent::SearchNotes,
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
    fn a_draft_goes_to_a_contact_after_a_yes_and_each_try_is_in_the_log() {
        use crate::send::{Account, Sender, fake::Fake};
        use kestrel_net_proto::Op;
        let (a, d) = assistant("send", true);
        let f = Fake::default();
        let account = || Account {
            internet: true,
            from: Some("me@example.com".into()),
            name: None,
        };
        let mut a = a
            .with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU))
            .with_sender(Sender::new(Box::new(f.clone()), Box::new(account)));
        let out = a.handle("add contact Ravi ravi@example.com", &mut |q| panic!("{q}"));
        assert!(out.last().unwrap().contains("ravi@example.com"), "{out:?}");
        let out = a.handle("send it to Ravi", &mut |q| panic!("{q}"));
        assert!(out.last().unwrap().contains("no draft"), "{out:?}");
        a.handle("draft a mail to Ravi: Leave on Friday", &mut yes());
        let mut shown = String::new();
        let out = a.handle("ravi ko bhej do", &mut |q| {
            shown = q.to_string();
            false
        });
        assert_eq!(out, ["Not sent."]);
        assert!(shown.contains("To: ravi@example.com"), "{shown}");
        assert!(
            shown.contains("email draft of: draft a mail to Ravi"),
            "{shown}"
        );
        assert!(f.asked.borrow().is_empty());
        let out = a.handle("send it to ravi", &mut yes());
        assert!(out.last().unwrap().contains("Sent to Ravi"), "{out:?}");
        assert_eq!(f.asked.borrow().len(), 1);
        assert!(matches!(f.asked.borrow()[0], Op::SmtpSend { .. }));
        let log = kestrel_memory::Memory::open(&d.join("memory.db"))
            .unwrap()
            .outbound(9)
            .unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].result.as_deref(), Some("sent"));
        assert!(!log[1].approved);
    }

    /// An assistant that can write and send, with Anup in the contacts.
    fn mailer(name: &str) -> (Assistant<'static>, crate::send::fake::Fake) {
        use crate::send::{Account, Sender, fake::Fake};
        let (a, _) = assistant(name, true);
        let f = Fake::default();
        let account = || Account {
            internet: true,
            from: Some("me@example.com".into()),
            name: None,
        };
        let mut a = a
            .with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU))
            .with_sender(Sender::new(Box::new(f.clone()), Box::new(account)));
        a.handle("add contact Anup anup@example.com", &mut |q| panic!("{q}"));
        (a, f)
    }

    #[test]
    fn one_request_writes_the_mail_and_it_leaves_only_after_a_yes() {
        for (n, text) in [
            "send an email to anup regarding a day off tomorrow",
            "anup ko mail kr do mai kl chutti pr hu",
        ]
        .into_iter()
        .enumerate()
        {
            let (mut a, f) = mailer(&format!("compose{n}"));
            let mut shown = String::new();
            let out = a.handle(text, &mut |q| {
                shown = q.to_string();
                false
            });
            assert!(shown.contains("To: anup@example.com"), "{shown}");
            assert!(
                shown.contains(&format!("email draft of: {text}")),
                "{shown}"
            );
            assert_eq!(out.last().map(String::as_str), Some("Not sent."), "{out:?}");
            assert!(f.asked.borrow().is_empty());
            // The no left the draft where it was.
            let out = a.handle("send it to anup", &mut yes());
            assert!(out.last().unwrap().contains("Sent to Anup"), "{out:?}");
            assert_eq!(f.asked.borrow().len(), 1);
            // And with a yes at once.
            let out = a.handle(text, &mut yes());
            assert!(out.last().unwrap().contains("Sent to Anup"), "{out:?}");
            assert_eq!(f.asked.borrow().len(), 2);
        }
    }

    #[test]
    fn someone_not_in_the_contacts_is_told_before_anything_is_written() {
        let (mut a, f) = mailer("compose_unknown");
        let out = a.handle("boss ko mail kr do mai late hu", &mut |q| panic!("{q}"));
        assert_eq!(
            out,
            ["boss is not in your contacts. Add them first: add contact boss name@example.com"]
        );
        let out = a.handle("send it to anup", &mut |q| panic!("{q}"));
        assert!(
            out.last().unwrap().contains("no draft"),
            "nothing written: {out:?}"
        );
        assert!(f.asked.borrow().is_empty());
    }

    #[test]
    fn asking_to_mail_someone_with_no_words_sends_the_draft_there_is() {
        let (mut a, f) = mailer("compose_draft");
        let out = a.handle("send an email to anup", &mut |q| panic!("{q}"));
        assert!(out.last().unwrap().contains("no draft"), "{out:?}");
        a.handle("draft a mail to anup: leave on friday", &mut |q| {
            panic!("{q}")
        });
        let out = a.handle("anup ko mail kr do", &mut yes());
        assert!(out.last().unwrap().contains("Sent to Anup"), "{out:?}");
        assert_eq!(f.asked.borrow().len(), 1);
    }

    #[test]
    fn a_draft_on_its_own_says_how_to_send_it_and_asks_nothing() {
        let (mut a, f) = mailer("compose_hint");
        let out = a.handle("draft a mail to anup: leave on friday", &mut |q| {
            panic!("{q}")
        });
        assert_eq!(
            out.last().map(String::as_str),
            Some("To send it, say: send it to Anup"),
            "{out:?}"
        );
        assert!(f.asked.borrow().is_empty());
    }

    #[test]
    fn a_question_or_a_remark_gets_a_chat_reply_and_an_action_is_still_refused() {
        let (a, _) = assistant("converse", true);
        let mut a = a.with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU));
        for text in [
            "how do I send an email from my phone",
            "i am planning a trip to Goa",
        ] {
            let out = a.handle(text, &mut |q| panic!("{q}"));
            assert!(
                out.iter()
                    .any(|l| l.contains(&format!("chat draft of: {text}"))),
                "{out:?}"
            );
            assert!(
                out.iter().any(|l| l.contains("small model")),
                "the caveat: {out:?}"
            );
        }
        for text in ["book me a flight", "book a table please"] {
            let out = a.handle(text, &mut |q| panic!("{q}"));
            assert!(
                out.last().unwrap().starts_with("I can't do that"),
                "{out:?}"
            );
        }
    }

    #[test]
    fn a_question_the_notes_answer_is_answered_from_them_first() {
        struct Closest(Vec<kestrel_tools::tools::Note>);
        impl crate::meaning::Meaning for Closest {
            fn find(&mut self, _: &str) -> Vec<kestrel_tools::tools::Note> {
                Vec::new()
            }
            fn k(&self) -> usize {
                2
            }
            fn closest(&mut self, _: &str, _: usize) -> Vec<kestrel_tools::tools::Note> {
                self.0.clone()
            }
        }
        let note = kestrel_tools::tools::Note {
            name: "budget.md".into(),
            text: "Ravi shares the plan.".into(),
        };
        let (a, _) = assistant("converse_notes", true);
        let mut a = a
            .with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU))
            .with_meaning(Box::new(Closest(vec![note])));
        for text in ["whose plan is it", "whom does the plan go to"] {
            let out = a.handle(text, &mut |q| panic!("{q}"));
            assert!(
                out.iter().any(|l| l.contains("Asked about 1 notes")),
                "{text}: {out:?}"
            );
        }
        // With no note close, a question nothing else took is a chat, and a search stays a search.
        let (a, _) = assistant("converse_none", true);
        let mut a = a
            .with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU))
            .with_meaning(Box::new(Closest(vec![])));
        let out = a.handle("whose plan is it", &mut |q| panic!("{q}"));
        assert!(out.iter().any(|l| l.contains("chat draft of")), "{out:?}");
        let out = a.handle("whom does the plan go to", &mut |q| panic!("{q}"));
        assert!(!out.iter().any(|l| l.contains("chat draft of")), "{out:?}");
        // Close by no measure of meaning, but it holds the question's words.
        let out = a.handle("whom does the standup report go to", &mut |q| panic!("{q}"));
        assert!(
            out.iter().any(|l| l.contains("Asked about 1 notes")),
            "{out:?}"
        );
    }

    #[test]
    fn rewriting_or_summing_up_given_text_is_written_without_asking() {
        let (a, _) = assistant("explicit", true);
        let mut a = a.with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU));
        let out = a.handle("rewrite this politely: send me the file now", &mut |q| {
            panic!("{q}")
        });
        assert!(
            out.iter().any(|l| l.starts_with("rewrite draft of")),
            "{out:?}"
        );
        let out = a.handle("summarize this: Kestrel keeps tasks.", &mut |q| {
            panic!("{q}")
        });
        assert!(
            out.iter().any(|l| l.starts_with("chat draft of")),
            "{out:?}"
        );
    }

    #[test]
    fn what_was_kept_comes_back_in_plain_words() {
        let (mut a, _) = assistant("plain_memory", true);
        a.handle("remember that I am allergic to peanuts", &mut yes());
        let out = a.handle("recall what I am allergic to", &mut yes());
        let all = out.join(" ");
        assert!(
            !all.contains("user_explicit") && !all.contains('['),
            "{all}"
        );
    }

    #[test]
    fn a_task_named_by_its_words_is_changed_and_no_new_task_is_made() {
        let (mut a, _) = assistant("by_name", true);
        for text in [
            "remind me to call asha",
            "remind me to buy milk",
            "remind me to see the doctor",
        ] {
            a.handle(text, &mut yes());
        }
        let count = |a: &mut Assistant<'_>| a.handle("show my tasks", &mut yes()).len();
        let before = count(&mut a);
        let out = a.handle("mark call asha as done", &mut |q| panic!("{q}"));
        assert_eq!(
            out.last().map(String::as_str),
            Some("done: #1 call asha"),
            "{out:?}"
        );
        let out = a.handle("reschedule doctor to monday", &mut |q| panic!("{q}"));
        assert!(out.last().unwrap().contains("task #3 now due"), "{out:?}");
        let mut asked = String::new();
        let out = a.handle("buy milk wala task hata do", &mut |q| {
            asked = q.to_string();
            false
        });
        assert_eq!(asked, "Delete task #2, buy milk?");
        assert_eq!(
            out.last().map(String::as_str),
            Some("Task #2 stays."),
            "{out:?}"
        );
        let out = a.handle("buy milk wala task hata do", &mut yes());
        assert_eq!(
            out.last().map(String::as_str),
            Some("deleted: #2 buy milk"),
            "{out:?}"
        );
        assert!(count(&mut a) < before, "fewer tasks, none added");
    }

    #[test]
    fn the_weather_is_answered_before_the_model_and_only_with_a_way_out() {
        let d = std::env::temp_dir().join(format!("kestrel_assistant_w_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (a, _) = assistant("weather", true);
        let mut a = a.with_online(crate::online::Online::new(
            Box::new(crate::send::fake::Fake::default()),
            Some(d),
        ));
        let out = a.handle("aaj ka mausam", &mut |q| panic!("{q}"));
        assert!(out[0].contains("internet is off"), "{out:?}");
        let (mut plain, _) = assistant("weather_plain", true);
        let out = plain.handle("aaj ka mausam", &mut yes());
        assert!(
            !out.iter().any(|l| l.contains("internet is off")),
            "{out:?}"
        );
    }

    #[test]
    fn each_request_is_counted_by_the_way_it_took_and_nothing_of_what_was_said() {
        let (a, d) = assistant("usage", true);
        let mut a = a.with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU));
        for text in [
            "remind me to call CANARY-7f3a tomorrow",
            "show my tasks",
            "draft a mail to Asha: CANARY-7f3a leave on Friday",
            "my sister's name is CANARY-7f3a",
            "why this answer",
        ] {
            a.handle(text, &mut yes());
        }
        let m = crate::memory::open(&d.join("memory.db")).unwrap();
        let rows = m.usage().unwrap();
        let n = |what: &str| rows.iter().find(|r| r.what == what).map_or(0, |r| r.n);
        for (what, times) in [
            ("request", 5),
            ("create_task", 1),
            ("task_added", 1),
            ("task_command", 1),
            ("email", 1),
            ("fact", 1),
            ("why", 1),
        ] {
            assert_eq!(n(what), times, "{what}: {rows:?}");
        }
        let all = format!("{rows:?}");
        for said in ["CANARY", "asha", "Asha", "sister"] {
            assert!(!all.contains(said), "{said}: {all}");
        }
    }

    #[test]
    fn a_request_nothing_could_place_is_counted_as_that() {
        let (mut a, d) = assistant("usage_none", true);
        a.handle("shrug at this", &mut yes());
        let m = crate::memory::open(&d.join("memory.db")).unwrap();
        let rows = m.usage().unwrap();
        let said: Vec<(&str, i64)> = rows.iter().map(|r| (r.what.as_str(), r.n)).collect();
        assert_eq!(said, [("not_understood", 1), ("request", 1)]);
    }

    #[test]
    fn why_this_answer_tells_the_way_each_answer_took() {
        let (a, _) = assistant("why", true);
        let mut a = a.with_kinds(Kinds::new(Box::new(KindByFirstWord), Box::new(Drafts), TAU));
        let why = |a: &mut Assistant<'_>| {
            a.handle("why this answer", &mut |q| panic!("{q}"))
                .join(" ")
        };
        assert!(why(&mut a).contains("no answer to explain"));
        a.handle("remind me to call asha tomorrow", &mut yes());
        let w = why(&mut a);
        assert!(w.contains("create task") && !w.contains('_'), "{w}");
        assert!(!w.contains("tier"), "{w}");
        assert!(w.contains("Nothing left this device"), "{w}");
        a.handle("show my tasks", &mut yes());
        assert!(why(&mut a).contains("rules"));
        a.handle("draft a mail to Asha: Leave on Friday", &mut yes());
        let w = why(&mut a);
        assert!(
            w.contains("a mail") && w.contains("90%") && w.contains("written by the model"),
            "{w}"
        );
        a.handle("my sister's name is Meera", &mut yes());
        assert!(why(&mut a).contains("rules"), "kept by rules");
        // Asking twice explains the same answer, not the asking.
        let first = why(&mut a);
        assert_eq!(why(&mut a), first);
    }

    #[test]
    fn why_this_answer_lists_what_left_this_device() {
        let (a, d) = assistant("why_left", true);
        let notes = d.clone();
        let settings = crate::settings::Settings {
            internet: true,
            place: Some(crate::online::Place {
                name: "Delhi".into(),
                region: String::new(),
                lat: "28.6".into(),
                lon: "77.2".into(),
            }),
            ..Default::default()
        };
        crate::settings::save(&crate::settings::path(&notes), &settings).unwrap();
        let link = crate::online::tests::Answers::default();
        let mut a = a.with_online(crate::online::Online::new(Box::new(link), Some(notes)));
        a.handle("aaj ka mausam", &mut |q| panic!("{q}"));
        let w = a.handle("ye jawab kyun", &mut |q| panic!("{q}")).join("\n");
        assert!(
            w.contains("weather · api.open-meteo.com · Delhi (28.6, 77.2)"),
            "{w}"
        );
    }

    #[test]
    fn without_a_way_out_asking_to_send_takes_the_usual_path() {
        let (mut a, _) = assistant("no_sender", true);
        let out = a.handle("send it to Ravi", &mut yes());
        assert!(
            !out.iter()
                .any(|l| l.contains("no draft") || l.contains("Sent")),
            "{out:?}"
        );
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
