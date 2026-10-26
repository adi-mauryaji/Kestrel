//! v3's decision before the cascade (ADR-037, ADR-038): which skill a request asks for. The kind
//! head (Tier 3's encoder with the Kestrel-Kinds-v0 head, an `EncoderTier<_, Kind>`) gives one
//! probability per kind: `app` keeps the Stage 2 path unchanged, a skill at or above τ is written
//! by Gemma (the worker, src/remote.rs), and below τ the user is asked first. The head sees the
//! normalized request it was trained on; the writer gets the request as typed, because a rewrite or
//! a translation needs its case and punctuation. In the service the text is shown while Gemma
//! writes it (src/stream.rs); the answer's lines are then only what follows it.
//!
//! When the head's top kind is below τ, Gemma may decide instead (the fallback, ADR-037): its kind
//! at or above τ_g is taken, otherwise the head's answer stands (a skill is asked about). Gemma's
//! prompt, examples, temperature and τ_g are the dev run's (experiments/v3/kind_fallback.py).

use std::path::Path;

use kestrel_core::{KestrelError, Request, TierModel};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    App,
    Chat,
    Email,
    Rewrite,
    Translate,
}

/// In the head's class order (datasets/contracts/kinds.txt).
pub const KINDS: [Kind; 5] = [
    Kind::App,
    Kind::Chat,
    Kind::Email,
    Kind::Rewrite,
    Kind::Translate,
];

/// A draft τ until the final head (ADR-037): a skill below it is asked about first.
pub const TAU: f32 = 0.7;

/// Under every chat answer (ADR-038).
pub const CAVEAT: &str = "(Written offline by a small model: it may be wrong.)";

impl Kind {
    pub fn from_label(label: &str) -> Option<Kind> {
        KINDS.into_iter().find(|k| k.label() == label)
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::App => "app",
            Kind::Chat => "chat",
            Kind::Email => "email",
            Kind::Rewrite => "rewrite",
            Kind::Translate => "translate",
        }
    }

    /// The y/N question when the head is unsure.
    fn question(self) -> &'static str {
        match self {
            Kind::App => "Do this the usual way?",
            Kind::Chat => "Answer this as a chat reply?",
            Kind::Email => "Write this as an email or message draft?",
            Kind::Rewrite => "Rewrite the text in your request?",
            Kind::Translate => "Translate the text in your request?",
        }
    }

    /// The instructions Gemma writes this kind with (the worker adds the chat format).
    pub fn system(self) -> &'static str {
        match self {
            Kind::App | Kind::Chat => {
                "You are Kestrel, an assistant that runs offline on the user's laptop. Answer \
                 helpfully and briefly: a short paragraph or a few bullet points. Reply in the \
                 language and script of the request: English gets English; Hinglish (Hindi written \
                 in Latin letters) gets Hinglish in Latin letters, not Devanagari. If you are not \
                 sure of a fact, say so."
            }
            Kind::Email => {
                "You write drafts for the user: emails, letters and messages to other people. \
                 Write only the draft, ready to send: for an email a \"Subject:\" line, then the body \
                 with a greeting and a sign-off. Keep it short and natural, in the language of the \
                 request (Hinglish in Latin letters for Hinglish). Do not invent names, dates or \
                 facts the request does not give: write a [placeholder] instead."
            }
            Kind::Rewrite => {
                "You rewrite the text given in the request, the way the request asks (tone, \
                 length, grammar, form). Answer with the rewritten text only, no explanation. Keep \
                 its meaning and the language of the text itself, whatever language the \
                 instruction is in: Hinglish text (Hindi in Latin letters, like \"kal milte \
                 hain\") stays Hinglish in Latin letters, and English text stays English. Do not \
                 switch to English or to Devanagari."
            }
            Kind::Translate => {
                "You translate the text given in the request into the language it asks for. \
                 Answer with the translation only. Hindi means Devanagari script (like \"कल \
                 मिलते हैं\"). Hinglish or Roman Hindi means Hindi words in Latin letters (like \
                 \"kal milte hain\"). Other Indian languages use their own script."
            }
        }
    }

    /// The longest answer, in tokens.
    pub fn max_tokens(self) -> usize {
        match self {
            Kind::Email => 320,
            Kind::Translate => 192,
            _ => 256,
        }
    }
}

/// Common Hindi words in Latin letters; experiments/v3/writer_lang.py's check reads the same list.
const HINGLISH_WORDS: &str = include_str!("../../../datasets/contracts/hinglish_words.txt");

/// One of the common Hindi words (lowercase).
pub fn hindi_word(w: &str) -> bool {
    HINGLISH_WORDS
        .lines()
        .any(|l| !l.starts_with('#') && l.trim() == w)
}

/// Latin text with enough common Hindi words (at least 2, and 15% of its words) is Hinglish.
pub fn is_hinglish(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    let hits = words.iter().filter(|w| hindi_word(w)).count();
    hits >= 2 && hits * 100 >= 15 * words.len().max(1)
}

fn devanagari(text: &str) -> bool {
    let hindi = text
        .chars()
        .filter(|c| ('\u{0900}'..='\u{097F}').contains(c))
        .count();
    hindi > text.chars().filter(char::is_ascii_alphabetic).count()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Lang {
    Hindi,
    Hinglish,
    English,
}

/// The language a translation is asked into, from the instruction: the one after "to" / "into" /
/// "in" or before "mein" / "me", else the only one named; "roman hindi" is Hinglish.
fn target(instruction: &str) -> Option<Lang> {
    let lower = instruction
        .to_lowercase()
        .replace("roman hindi", "hinglish");
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let named: Vec<(usize, Lang)> = (words.iter().enumerate())
        .filter_map(|(i, w)| match *w {
            "hindi" => Some((i, Lang::Hindi)),
            "hinglish" => Some((i, Lang::Hinglish)),
            "english" => Some((i, Lang::English)),
            _ => None,
        })
        .collect();
    let marked: Vec<Lang> = (named.iter())
        .filter(|&&(i, _)| {
            (i > 0 && matches!(words[i - 1], "to" | "into" | "in"))
                || matches!(words.get(i + 1), Some(&("mein" | "me")))
        })
        .map(|&(_, l)| l)
        .collect();
    match (marked.as_slice(), named.as_slice()) {
        ([l], _) | ([], [(_, l)]) => Some(*l),
        _ => None,
    }
}

/// The language the answer must be in, said once more after the request (Gemma follows that
/// better than the system prompt alone; experiments/v3/writer_lang.py): a translation's target
/// from its instruction (before ":"), a rewrite's from the text after ":". None when unknown.
pub fn language_note(kind: Kind, request: &str) -> Option<&'static str> {
    let (instruction, text) = match request.split_once(':') {
        Some((i, t)) => (i, Some(t)),
        None => (request, None),
    };
    match kind {
        Kind::Translate => Some(match target(instruction)? {
            Lang::Hindi => "Write the translation in Hindi, in Devanagari script.",
            Lang::Hinglish => "Write the translation in Hinglish: Hindi words in Latin letters.",
            Lang::English => "Write the translation in English.",
        }),
        Kind::Rewrite => {
            let text = text?;
            Some(if devanagari(text) {
                "The text is Hindi: write the result in Hindi, in Devanagari script."
            } else if is_hinglish(text) {
                "The text is Hinglish: write the result in Hinglish, in Latin letters."
            } else {
                "The text is English: write the result in English."
            })
        }
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
pub enum Choice {
    App,
    Skill(Kind),
    Ask(Kind),
}

pub fn decide(probs: &[(Kind, f32)], tau: f32) -> Choice {
    match top(probs) {
        None | Some((Kind::App, _)) => Choice::App,
        Some((k, p)) if p >= tau => Choice::Skill(k),
        Some((k, _)) => Choice::Ask(k),
    }
}

/// `decide`, except that below τ the fallback's kind at or above `tau_g` is taken.
pub fn decide_fallback(
    probs: &[(Kind, f32)],
    tau: f32,
    fallback: Option<(Kind, f32)>,
    tau_g: f32,
) -> Choice {
    match (top(probs), fallback) {
        (Some((_, p)), Some((Kind::App, g))) if p < tau && g >= tau_g => Choice::App,
        (Some((_, p)), Some((k, g))) if p < tau && g >= tau_g => Choice::Skill(k),
        _ => decide(probs, tau),
    }
}

fn top(probs: &[(Kind, f32)]) -> Option<(Kind, f32)> {
    probs.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Where the service finds the fallback's settings (written by experiments/v3/kind_fallback.py).
pub const FALLBACK_CONFIG: &str = "experiments/configs/kinds_fallback_v0.json";

/// The fallback as the dev run measured it: Gemma's instructions and examples, the temperature
/// fitted on cal, and τ_g.
#[derive(Debug, Deserialize)]
pub struct FallbackConfig {
    pub model_file: String,
    pub system: String,
    pub shots: Vec<Shot>,
    pub temperature: f32,
    pub tau_g: f32,
}

#[derive(Debug, Deserialize)]
pub struct Shot {
    pub text: String,
    pub label: String,
}

impl FallbackConfig {
    pub fn load(path: &Path) -> Result<Self, KestrelError> {
        let bad = |e: String| KestrelError::Invalid(format!("{}: {e}", path.display()));
        let text = std::fs::read_to_string(path).map_err(|e| bad(e.to_string()))?;
        let cfg: Self = serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
        if let Some(s) = cfg
            .shots
            .iter()
            .find(|s| Kind::from_label(&s.label).is_none())
        {
            return Err(bad(format!("example label {:?} is not a kind", s.label)));
        }
        if !(cfg.temperature.is_finite()
            && cfg.temperature > 0.0
            && cfg.tau_g > 0.0
            && cfg.tau_g <= 1.0)
        {
            return Err(bad(format!(
                "temperature {} or tau_g {} out of range",
                cfg.temperature, cfg.tau_g
            )));
        }
        Ok(cfg)
    }
}

/// Writes what a skill request asks for: the whole text, after handing it to `on_part` piece by
/// piece as it is written (a writer that cannot stream may hand over nothing).
pub trait Writer {
    fn write(
        &self,
        kind: Kind,
        request: &str,
        on_part: &mut dyn FnMut(&str),
    ) -> Result<String, KestrelError>;
}

pub struct Kinds<'a> {
    head: Box<dyn TierModel<Kind> + 'a>,
    writer: Box<dyn Writer + 'a>,
    tau: f32,
    fallback: Option<(Box<dyn TierModel<Kind> + 'a>, f32)>,
}

impl<'a> Kinds<'a> {
    pub fn new(
        head: Box<dyn TierModel<Kind> + 'a>,
        writer: Box<dyn Writer + 'a>,
        tau: f32,
    ) -> Self {
        Self {
            head,
            writer,
            tau,
            fallback: None,
        }
    }

    /// Below τ, `fallback`'s kind at or above `tau_g` decides.
    pub fn with_fallback(mut self, fallback: Box<dyn TierModel<Kind> + 'a>, tau_g: f32) -> Self {
        self.fallback = Some((fallback, tau_g));
        self
    }

    /// The answer when `req` asks for a skill; `None` sends it the Stage 2 way. With `stream`, the
    /// written text goes there as it comes, and the lines hold only what follows it.
    pub fn handle(
        &self,
        req: &Request,
        typed: &str,
        confirm: &mut dyn FnMut(&str) -> bool,
        mut stream: Option<&mut dyn FnMut(&str)>,
    ) -> Option<Vec<String>> {
        // A head that fails leaves the request to the Stage 2 path.
        let probs = self.head.classify(req).ok()?;
        // Asked only when the head is unsure; a failed fallback leaves the head's answer.
        let unsure = top(&probs).is_some_and(|(_, p)| p < self.tau);
        let (fallback, tau_g) = match &self.fallback {
            Some((f, tau_g)) if unsure => (f.classify(req).ok().and_then(|p| top(&p)), *tau_g),
            _ => (None, 1.0),
        };
        let kind = match decide_fallback(&probs, self.tau, fallback, tau_g) {
            Choice::App => return None,
            Choice::Skill(k) => k,
            Choice::Ask(k) if confirm(k.question()) => k,
            Choice::Ask(_) => return None,
        };
        let mut shown = String::new();
        let written = self.writer.write(kind, typed.trim(), &mut |p| {
            if let Some(show) = stream.as_mut() {
                shown.push_str(p);
                show(p);
            }
        });
        let finished = written.is_ok();
        let mut lines = match (written, stream) {
            (Ok(text), Some(show)) => {
                // What the writer did not stream is shown now.
                if let Some(rest) = text.strip_prefix(shown.as_str()).filter(|r| !r.is_empty()) {
                    show(rest);
                }
                Vec::new()
            }
            (Ok(text), None) => text.lines().map(String::from).collect(),
            (Err(e), _) => vec![format!("I couldn't write that: {e}")],
        };
        // Model-written text on screen, even cut off, carries the caveat (ADR-038).
        if kind == Kind::Chat && (finished || !shown.is_empty()) {
            lines.push(CAVEAT.into());
        }
        Some(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::Tier;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn the_kinds_are_the_contracts_labels_in_its_order() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../datasets/contracts/kinds.txt"
        );
        let labels: Vec<String> = std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect();
        assert_eq!(labels, ["app", "chat", "email", "rewrite", "translate"]);
        for (label, kind) in labels.iter().zip(KINDS) {
            assert_eq!(Kind::from_label(label), Some(kind));
            assert_eq!(kind.label(), label);
        }
        assert_eq!(Kind::from_label("code"), None);
    }

    #[test]
    fn the_top_kind_decides_and_an_unsure_skill_is_asked_about() {
        use Kind::*;
        assert_eq!(decide(&[(App, 0.6), (Email, 0.4)], 0.7), Choice::App);
        assert_eq!(
            decide(&[(App, 0.1), (Email, 0.9)], 0.7),
            Choice::Skill(Email)
        );
        assert_eq!(decide(&[(Chat, 0.7), (App, 0.3)], 0.7), Choice::Skill(Chat));
        assert_eq!(
            decide(&[(App, 0.3), (Rewrite, 0.5), (Chat, 0.2)], 0.7),
            Choice::Ask(Rewrite)
        );
        assert_eq!(decide(&[], 0.7), Choice::App);
    }

    /// A head that always gives `probs`, or fails.
    struct Fixed(Result<Vec<(Kind, f32)>, ()>);

    impl TierModel<Kind> for Fixed {
        fn tier(&self) -> Tier {
            Tier::Encoder
        }
        fn est_cost_ms(&self) -> u32 {
            10
        }
        fn classify(&self, _: &Request) -> Result<Vec<(Kind, f32)>, KestrelError> {
            self.0
                .clone()
                .map_err(|()| KestrelError::Invalid("no head".into()))
        }
    }

    /// A writer that records what it was asked and answers `reply`.
    /// Every (kind, request) a writer was asked for.
    type Asked = Rc<RefCell<Vec<(Kind, String)>>>;

    struct Records(Asked, Result<&'static str, ()>);

    impl Writer for Records {
        fn write(
            &self,
            kind: Kind,
            request: &str,
            _: &mut dyn FnMut(&str),
        ) -> Result<String, KestrelError> {
            self.0.borrow_mut().push((kind, request.into()));
            self.1
                .map(String::from)
                .map_err(|()| KestrelError::Invalid("worker gone".into()))
        }
    }

    fn kinds(
        probs: Result<Vec<(Kind, f32)>, ()>,
        reply: Result<&'static str, ()>,
    ) -> (Kinds<'static>, Asked) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let k = Kinds::new(
            Box::new(Fixed(probs)),
            Box::new(Records(asked.clone(), reply)),
            TAU,
        );
        (k, asked)
    }

    const TYPED: &str = "  Make this polite: Send me the file NOW!  ";

    #[test]
    fn a_sure_skill_is_written_from_the_request_as_typed() {
        let (k, asked) = kinds(
            Ok(vec![(Kind::Rewrite, 0.9), (Kind::App, 0.1)]),
            Ok("Could you send me the file?\nThanks."),
        );
        let req = Request::new(1, TYPED);
        let out = k.handle(&req, TYPED, &mut |_| panic!("no question when sure"), None);
        assert_eq!(
            out,
            Some(vec!["Could you send me the file?".into(), "Thanks.".into()])
        );
        assert_eq!(
            *asked.borrow(),
            [(
                Kind::Rewrite,
                "Make this polite: Send me the file NOW!".into()
            )]
        );
    }

    #[test]
    fn a_chat_answer_carries_the_caveat() {
        let (k, _) = kinds(
            Ok(vec![(Kind::Chat, 0.95)]),
            Ok("Recursion is a function calling itself."),
        );
        let out = k.handle(
            &Request::new(1, "explain recursion"),
            "explain recursion",
            &mut |_| true,
            None,
        );
        assert_eq!(
            out,
            Some(vec![
                "Recursion is a function calling itself.".into(),
                CAVEAT.into()
            ])
        );
    }

    #[test]
    fn an_unsure_skill_is_asked_and_no_keeps_the_old_path() {
        let probs = vec![(Kind::Email, 0.5), (Kind::App, 0.4)];
        let (k, asked) = kinds(Ok(probs.clone()), Ok("Subject: Leave"));
        let mut questions = Vec::new();
        let req = Request::new(1, "leave mail");
        assert_eq!(
            k.handle(
                &req,
                "leave mail",
                &mut |q| {
                    questions.push(q.to_string());
                    false
                },
                None
            ),
            None
        );
        assert_eq!(questions, ["Write this as an email or message draft?"]);
        assert!(asked.borrow().is_empty());
        assert_eq!(
            k.handle(&req, "leave mail", &mut |_| true, None),
            Some(vec!["Subject: Leave".into()])
        );
    }

    #[test]
    fn app_requests_and_a_failed_head_go_the_old_way_and_a_failed_writer_says_so() {
        let (k, asked) = kinds(Ok(vec![(Kind::App, 0.99)]), Ok("never"));
        assert_eq!(
            k.handle(
                &Request::new(1, "remind me"),
                "remind me",
                &mut |_| true,
                None
            ),
            None
        );
        let (broken, _) = kinds(Err(()), Ok("never"));
        assert_eq!(
            broken.handle(&Request::new(1, "x"), "x", &mut |_| true, None),
            None
        );
        assert!(asked.borrow().is_empty());
        let (k, _) = kinds(Ok(vec![(Kind::Translate, 0.9)]), Err(()));
        let out = k
            .handle(
                &Request::new(1, "translate hi"),
                "translate hi",
                &mut |_| true,
                None,
            )
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            out[0].starts_with("I couldn't write that: ") && out[0].ends_with("worker gone"),
            "{out:?}"
        );
    }

    /// A writer that hands its answer over in two pieces; with `fails`, it stops after the first.
    struct Streams(&'static str, bool);

    impl Writer for Streams {
        fn write(
            &self,
            _: Kind,
            _: &str,
            on_part: &mut dyn FnMut(&str),
        ) -> Result<String, KestrelError> {
            let (a, b) = self.0.split_at(self.0.len() / 2);
            on_part(a);
            if self.1 {
                return Err(KestrelError::Invalid("worker gone".into()));
            }
            on_part(b);
            Ok(self.0.into())
        }
    }

    #[test]
    fn a_streamed_answer_goes_to_the_stream_and_only_what_follows_it_is_returned() {
        const ANSWER: &str = "Recursion is a function calling itself.";
        let chat = |fails| {
            Kinds::new(
                Box::new(Fixed(Ok(vec![(Kind::Chat, 0.95)]))),
                Box::new(Streams(ANSWER, fails)),
                TAU,
            )
        };
        let req = Request::new(1, "explain recursion");
        let mut parts: Vec<String> = Vec::new();
        let out = chat(false).handle(
            &req,
            "explain recursion",
            &mut |_| true,
            Some(&mut |p: &str| parts.push(p.into())),
        );
        assert_eq!((parts.len(), parts.concat()), (2, ANSWER.to_string()));
        assert_eq!(out, Some(vec![CAVEAT.into()]));
        // A writer that does not stream still shows its text, in one part.
        let (k, _) = kinds(
            Ok(vec![(Kind::Email, 0.9)]),
            Ok("Subject: Leave\n\nDear Asha,"),
        );
        let mut parts: Vec<String> = Vec::new();
        let out = k.handle(
            &req,
            "leave mail",
            &mut |_| true,
            Some(&mut |p: &str| parts.push(p.into())),
        );
        assert_eq!(parts, ["Subject: Leave\n\nDear Asha,"]);
        assert_eq!(out, Some(vec![]));
        // Cut off: what was shown stays, the failure is said, and a chat answer keeps its caveat.
        let mut parts: Vec<String> = Vec::new();
        let out = chat(true).handle(
            &req,
            "explain recursion",
            &mut |_| true,
            Some(&mut |p: &str| parts.push(p.into())),
        );
        assert_eq!(parts.len(), 1);
        let out = out.unwrap();
        assert!(out[0].starts_with("I couldn't write that: "), "{out:?}");
        assert_eq!(out[1..], [CAVEAT.to_string()]);
    }

    #[test]
    fn below_tau_a_sure_fallback_decides_and_an_unsure_one_leaves_the_head_alone() {
        use Kind::*;
        // The cases of experiments/v3/test_kind_fallback.py, which simulated this policy.
        let sure_email = [(App, 0.1), (Email, 0.9)];
        let unsure_chat = [(App, 0.4), (Chat, 0.5), (Email, 0.1)];
        let unsure_app = [(App, 0.55), (Chat, 0.45)];
        let d = |p: &[(Kind, f32)], g| decide_fallback(p, 0.7, g, 0.8);
        assert_eq!(d(&sure_email, Some((Chat, 0.99))), Choice::Skill(Email));
        assert_eq!(
            d(&unsure_chat, Some((Rewrite, 0.85))),
            Choice::Skill(Rewrite)
        );
        assert_eq!(d(&unsure_app, Some((Chat, 0.85))), Choice::Skill(Chat));
        assert_eq!(d(&unsure_chat, Some((Rewrite, 0.6))), Choice::Ask(Chat));
        assert_eq!(d(&unsure_app, Some((Chat, 0.6))), Choice::App);
        assert_eq!(d(&unsure_chat, Some((App, 0.95))), Choice::App);
        assert_eq!(
            d(&unsure_chat, None),
            Choice::Ask(Chat),
            "no fallback answer: the head alone"
        );
    }

    /// A fallback that answers `probs` (or fails) and counts its calls.
    struct Counted(Rc<std::cell::Cell<u32>>, Result<Vec<(Kind, f32)>, ()>);

    impl TierModel<Kind> for Counted {
        fn tier(&self) -> Tier {
            Tier::Generative
        }
        fn est_cost_ms(&self) -> u32 {
            400
        }
        fn classify(&self, _: &Request) -> Result<Vec<(Kind, f32)>, KestrelError> {
            self.0.set(self.0.get() + 1);
            self.1
                .clone()
                .map_err(|()| KestrelError::Invalid("worker gone".into()))
        }
    }

    #[test]
    fn the_fallback_is_asked_only_below_tau_and_a_failed_one_leaves_the_head_alone() {
        let calls = Rc::new(std::cell::Cell::new(0));
        let with = |probs: Vec<(Kind, f32)>, fallback| {
            let (k, asked) = kinds(Ok(probs), Ok("written"));
            (
                k.with_fallback(Box::new(Counted(calls.clone(), fallback)), 0.9),
                asked,
            )
        };
        let req = Request::new(1, "x");
        let no_question = &mut |q: &str| -> bool { panic!("asked {q}") };
        let (k, _) = with(vec![(Kind::Email, 0.9)], Ok(vec![(Kind::Chat, 0.99)]));
        assert_eq!(
            k.handle(&req, "x", no_question, None),
            Some(vec!["written".into()])
        );
        assert_eq!(calls.get(), 0, "a sure head needs no fallback");
        let (k, asked) = with(
            vec![(Kind::Chat, 0.5), (Kind::App, 0.4)],
            Ok(vec![(Kind::Rewrite, 0.95)]),
        );
        assert_eq!(
            k.handle(&req, "x", no_question, None),
            Some(vec!["written".into()])
        );
        assert_eq!((calls.get(), asked.borrow()[0].0), (1, Kind::Rewrite));
        let (k, _) = with(
            vec![(Kind::App, 0.55), (Kind::Chat, 0.45)],
            Ok(vec![(Kind::App, 0.97)]),
        );
        assert_eq!(
            k.handle(&req, "x", no_question, None),
            None,
            "the fallback's app: the old path"
        );
        let (k, _) = with(vec![(Kind::Chat, 0.5), (Kind::App, 0.4)], Err(()));
        let mut questions = 0;
        let out = k.handle(
            &req,
            "x",
            &mut |_| {
                questions += 1;
                false
            },
            None,
        );
        assert_eq!(
            (out, questions),
            (None, 1),
            "a failed fallback: asked, as the head alone"
        );
    }

    #[test]
    fn the_committed_fallback_config_loads() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../experiments/configs/kinds_fallback_v0.json"
        );
        let cfg = FallbackConfig::load(std::path::Path::new(path)).unwrap();
        assert!(cfg.tau_g > 0.0 && cfg.tau_g <= 1.0 && cfg.temperature > 0.0);
        assert!(cfg.shots.len() >= 10 && cfg.system.contains("\napp: "));
        assert!(FallbackConfig::load(std::path::Path::new("no/such.json")).is_err());
    }

    #[test]
    fn hinglish_is_told_by_its_hindi_words_as_in_the_python_check() {
        // experiments/v3/test_writer_lang.py's cases.
        for (text, want) in [
            ("Kya aap please abhi file bhej sakte hain?", true),
            ("Meeting shaam 5 baje hai.", true),
            ("Apna id card lena mat bhoolna.", true),
            ("Main traffic mein phans gaya hoon.", true),
            ("Could you please send the file now?", false),
            ("The main issue is the date.", false),
            ("Thank you, see you tomorrow.", false),
        ] {
            assert_eq!(is_hinglish(text), want, "{text}");
        }
    }

    #[test]
    fn the_writer_is_told_the_language_once_more() {
        use Kind::*;
        let hindi = Some("Write the translation in Hindi, in Devanagari script.");
        let hinglish = Some("Write the translation in Hinglish: Hindi words in Latin letters.");
        let english = Some("Write the translation in English.");
        assert_eq!(
            language_note(Translate, "translate this to hindi: the train is late"),
            hindi
        );
        assert_eq!(
            language_note(Translate, "isko hindi mein translate karo: call me"),
            hindi
        );
        assert_eq!(
            language_note(Translate, "ise hinglish mein likho: see you"),
            hinglish
        );
        assert_eq!(
            language_note(Translate, "roman hindi mein likho: i am late"),
            hinglish
        );
        assert_eq!(
            language_note(
                Translate,
                "translate this hindi line to english: mujhe der ho jayegi"
            ),
            english
        );
        assert_eq!(
            language_note(Translate, "isko english mein likho: kripya ruko"),
            english
        );
        // The text's own words do not choose the language.
        assert_eq!(
            language_note(Translate, "translate to english: hindi kaun bolta hai"),
            english
        );
        assert_eq!(language_note(Translate, "what does jaldi karo mean"), None);
        assert_eq!(
            language_note(Rewrite, "isko polite bana do: abhi file bhejo"),
            Some("The text is Hinglish: write the result in Hinglish, in Latin letters.")
        );
        assert_eq!(
            language_note(Rewrite, "isko polite bana do: send me the report today"),
            Some("The text is English: write the result in English.")
        );
        assert_eq!(
            language_note(Rewrite, "ise formal karo: मैं कल नहीं आऊँगा"),
            Some("The text is Hindi: write the result in Hindi, in Devanagari script.")
        );
        assert_eq!(
            language_note(Rewrite, "make this shorter"),
            None,
            "no text to tell"
        );
        assert_eq!(language_note(Chat, "explain recursion: simply"), None);
    }
}
