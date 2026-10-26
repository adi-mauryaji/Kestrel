//! v3's decision before the cascade (ADR-037, ADR-038): which skill a request asks for. The kind
//! head (Tier 3's encoder with the Kestrel-Kinds-v0 head, an `EncoderTier<_, Kind>`) gives one
//! probability per kind: `app` keeps the Stage 2 path unchanged, a skill at or above τ is written
//! by Gemma (the worker, src/remote.rs), and below τ the user is asked first. The head sees the
//! normalized request it was trained on; the writer gets the request as typed, because a rewrite or
//! a translation needs its case and punctuation.

use kestrel_core::{KestrelError, Request, TierModel};

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
                 its meaning and its language."
            }
            Kind::Translate => {
                "You translate the text given in the request into the language it asks for. \
                 Answer with the translation only. Hinglish and Roman Hindi mean Hindi written in \
                 Latin letters; Hindi and other Indian languages otherwise use their own script."
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

#[derive(Debug, PartialEq)]
pub enum Choice {
    App,
    Skill(Kind),
    Ask(Kind),
}

pub fn decide(probs: &[(Kind, f32)], tau: f32) -> Choice {
    match probs.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
        None | Some((Kind::App, _)) => Choice::App,
        Some(&(k, p)) if p >= tau => Choice::Skill(k),
        Some(&(k, _)) => Choice::Ask(k),
    }
}

/// Writes what a skill request asks for.
pub trait Writer {
    fn write(&self, kind: Kind, request: &str) -> Result<String, KestrelError>;
}

pub struct Kinds<'a> {
    head: Box<dyn TierModel<Kind> + 'a>,
    writer: Box<dyn Writer + 'a>,
    tau: f32,
}

impl<'a> Kinds<'a> {
    pub fn new(
        head: Box<dyn TierModel<Kind> + 'a>,
        writer: Box<dyn Writer + 'a>,
        tau: f32,
    ) -> Self {
        Self { head, writer, tau }
    }

    /// The answer when `req` asks for a skill; `None` sends it the Stage 2 way.
    pub fn handle(
        &self,
        req: &Request,
        typed: &str,
        confirm: &mut dyn FnMut(&str) -> bool,
    ) -> Option<Vec<String>> {
        // A head that fails leaves the request to the Stage 2 path.
        let probs = self.head.classify(req).ok()?;
        let kind = match decide(&probs, self.tau) {
            Choice::App => return None,
            Choice::Skill(k) => k,
            Choice::Ask(k) if confirm(k.question()) => k,
            Choice::Ask(_) => return None,
        };
        Some(match self.writer.write(kind, typed.trim()) {
            Ok(text) => {
                let mut lines: Vec<String> = text.lines().map(String::from).collect();
                if kind == Kind::Chat {
                    lines.push(CAVEAT.into());
                }
                lines
            }
            Err(e) => vec![format!("I couldn't write that: {e}")],
        })
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
        fn write(&self, kind: Kind, request: &str) -> Result<String, KestrelError> {
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
        let out = k.handle(&req, TYPED, &mut |_| panic!("no question when sure"));
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
            k.handle(&req, "leave mail", &mut |q| {
                questions.push(q.to_string());
                false
            }),
            None
        );
        assert_eq!(questions, ["Write this as an email or message draft?"]);
        assert!(asked.borrow().is_empty());
        assert_eq!(
            k.handle(&req, "leave mail", &mut |_| true),
            Some(vec!["Subject: Leave".into()])
        );
    }

    #[test]
    fn app_requests_and_a_failed_head_go_the_old_way_and_a_failed_writer_says_so() {
        let (k, asked) = kinds(Ok(vec![(Kind::App, 0.99)]), Ok("never"));
        assert_eq!(
            k.handle(&Request::new(1, "remind me"), "remind me", &mut |_| true),
            None
        );
        let (broken, _) = kinds(Err(()), Ok("never"));
        assert_eq!(
            broken.handle(&Request::new(1, "x"), "x", &mut |_| true),
            None
        );
        assert!(asked.borrow().is_empty());
        let (k, _) = kinds(Ok(vec![(Kind::Translate, 0.9)]), Err(()));
        let out = k
            .handle(
                &Request::new(1, "translate hi"),
                "translate hi",
                &mut |_| true,
            )
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            out[0].starts_with("I couldn't write that: ") && out[0].ends_with("worker gone"),
            "{out:?}"
        );
    }
}
