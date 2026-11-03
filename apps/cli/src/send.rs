//! Sending what Kestrel drafted (v4 C, blueprint §4.8): "send it to Ravi", "ravi ko bhej do". Nothing
//! leaves without a yes: the exact mail is shown, the request goes to the log first, and the yes
//! becomes the one token `kestrel-net` accepts. Recipients come only from the person's contacts, so a
//! draft or a web page cannot name where it goes.

use std::path::Path;

use kestrel_core::KestrelError;
use kestrel_memory::Memory;
use kestrel_net_proto::{Mail, Op, approval};
use serde_json::Value;

/// Gmail's server: what the log names as the host.
const GMAIL: &str = "smtp.gmail.com";
/// Words a contact's name may have.
const NAME_WORDS: usize = 3;
/// The latest requests the page lists.
const LOG_SHOWN: u32 = 50;
/// What the mail card waits for (capabilities/95-email.send.json).
pub const MAIL_SETUP: &str = "gmail.app_password";
/// What a draft is called when it has no "Subject:" line.
const NO_SUBJECT: &str = "A message";
/// How a draft leaves a blank for the person's own name.
const OWN_NAME: [&str; 4] = ["[your name]", "[aapka naam]", "[apna naam]", "[mera naam]"];
/// What "send ..." may name, in English.
const THE_DRAFT: [&str; 10] = [
    "it",
    "this",
    "that",
    "the mail",
    "this mail",
    "the email",
    "this email",
    "the draft",
    "the message",
    "this message",
];
/// How "bhej do" may end, in Hinglish.
const SEND_IT: [&str; 5] = ["bhej do", "bhej dena", "bhej dijiye", "bhej dijie", "bhejo"];
/// What may stand beside "bhej do" and still mean the draft.
const THE_DRAFT_HI: [&str; 10] = [
    "ye", "yeh", "ise", "isse", "is", "mail", "email", "message", "draft", "ko",
];

/// Kestrel's way out: `kestrel-net`, or a stand-in in tests.
pub trait Link {
    /// The key approval tokens are made with.
    fn key(&self) -> String;
    fn ask(&self, op: Op) -> Result<Value, String>;
}

/// What a request says about sending and contacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    Contact { name: String, email: String },
    Send { to: Option<String> },
}

/// One plain mail address.
pub fn is_address(text: &str) -> bool {
    let bad = |c: char| c.is_whitespace() || c.is_control() || "<>,;:\"\\()[]".contains(c);
    match text.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
                && !text.contains(bad)
        }
        None => false,
    }
}

/// A name of up to `NAME_WORDS` words.
fn name_of(words: &[&str]) -> Option<String> {
    (!words.is_empty() && words.len() <= NAME_WORDS).then(|| words.join(" "))
}

/// "add contact Ravi ravi@x.in", "Ravi's email is ravi@x.in", "ravi ka email ravi@x.in hai".
fn contact(words: &[&str], lower: &[String]) -> Option<Said> {
    let said = |name: Option<String>, email: &str| {
        Some(Said::Contact {
            name: name?,
            email: email.to_string(),
        })
    };
    let n = words.len();
    if n >= 4 && lower[0] == "add" && lower[1] == "contact" && is_address(words[n - 1]) {
        return said(name_of(&words[2..n - 1]), words[n - 1]);
    }
    let is_mail = |w: &str| w == "email" || w == "mail";
    if n >= 4 && is_mail(&lower[n - 3]) && lower[n - 2] == "is" && is_address(words[n - 1]) {
        let mut name: Vec<&str> = words[..n - 3].to_vec();
        let last = name.pop()?;
        let owner = last.strip_suffix("'s").or(last.strip_suffix("\u{2019}s"))?;
        name.push(owner);
        return said(name_of(&name), words[n - 1]);
    }
    let at = lower.iter().position(|w| w == "ka" || w == "ki")?;
    match (lower.get(at + 1), lower.get(at + 2), lower.get(at + 3)) {
        (Some(m), Some(_), Some(hai)) if is_mail(m) && hai == "hai" && n == at + 4 => {
            let email = words[at + 2];
            if is_address(email) {
                return said(name_of(&words[..at]), email);
            }
            None
        }
        _ => None,
    }
}

/// "send it to Ravi", "mail it", "ravi ko mail bhej do", "bhej do".
fn to_send(words: &[&str], lower: &[String]) -> Option<Said> {
    let text = lower.join(" ");
    if matches!(lower[0].as_str(), "send" | "mail" | "email") {
        let rest = &text[lower[0].len()..];
        let rest = rest.trim_start();
        for draft in THE_DRAFT {
            let Some(after) = rest.strip_prefix(draft) else {
                continue;
            };
            if after.is_empty() {
                return Some(Said::Send { to: None });
            }
            if let Some(name) = after.strip_prefix(" to ") {
                let count = name.split_whitespace().count();
                return name_of(&words[words.len() - count..]).map(|n| Said::Send { to: Some(n) });
            }
        }
        return None;
    }
    let ending = SEND_IT
        .iter()
        .find(|e| text == **e || text.ends_with(&format!(" {e}")))?;
    let before = lower.len() - ending.split(' ').count();
    let mut end = before;
    // The words naming the draft, from either end; "ko" stays when a name is before it.
    let spare = |w: &String| THE_DRAFT_HI.contains(&w.as_str());
    while end > 0 && lower[end - 1] != "ko" && spare(&lower[end - 1]) {
        end -= 1;
    }
    let mut start = 0;
    while start < end && lower[start] != "ko" && spare(&lower[start]) {
        start += 1;
    }
    if start == end {
        return Some(Said::Send { to: None });
    }
    if lower[end - 1] != "ko" || end - start < 2 {
        return None;
    }
    name_of(&words[start..end - 1]).map(|n| Said::Send { to: Some(n) })
}

/// What `text` says about sending, if it does.
pub fn parse(text: &str) -> Option<Said> {
    let said = text.trim().trim_end_matches(['.', '!', '?']).trim();
    let words: Vec<&str> = said.split_whitespace().collect();
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    if words.is_empty() {
        return None;
    }
    contact(&words, &lower).or_else(|| to_send(&words, &lower))
}

/// A draft as subject and body: its "Subject:" line if it has one.
pub fn split_draft(draft: &str) -> (String, String) {
    let text = draft.trim();
    let first = text.lines().next().unwrap_or("");
    let plain = first.replace(['*', '#'], "");
    let plain = plain.trim();
    let subject = plain
        .get(.."subject:".len())
        .filter(|p| p.eq_ignore_ascii_case("subject:"))
        .map(|p| plain[p.len()..].trim())
        .filter(|s| !s.is_empty());
    match subject {
        Some(s) => (s.to_string(), text[first.len()..].trim().to_string()),
        None => (NO_SUBJECT.to_string(), text.to_string()),
    }
}

/// What the person has set up for sending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub internet: bool,
    pub from: Option<String>,
    /// The name the page greets: what a draft's "[Your Name]" becomes.
    pub name: Option<String>,
}

/// `text` with the blank for the person's own name filled, when the name is known.
fn with_name(text: &str, name: Option<&str>) -> String {
    let Some(name) = name else {
        return text.to_string();
    };
    let mut out = text.to_string();
    for blank in OWN_NAME {
        // The ASCII lowercase keeps every byte where it was, in any script.
        while let Some(at) = out.to_ascii_lowercase().find(blank) {
            out.replace_range(at..at + blank.len(), name);
        }
    }
    out
}

/// The y/N question: the mail exactly as it would leave.
fn preview(mail: &Mail) -> String {
    let blank = mail.body.contains('[') && mail.body.contains(']') || mail.subject.contains('[');
    let warning = if blank {
        "\n\nThis still has a [blank] in it."
    } else {
        ""
    };
    format!(
        "Send this mail?\n\nFrom: {}\nTo: {}\nSubject: {}\n\n{}{warning}",
        mail.from, mail.to, mail.subject, mail.body
    )
}

/// Sends `draft` to the contact `to`, after a yes; what to say. `nonce` is unique to this send.
#[allow(clippy::too_many_arguments)]
pub fn send(
    to: Option<&str>,
    draft: Option<&str>,
    account: &Account,
    memory: &mut Memory,
    link: &dyn Link,
    nonce: &str,
    now: i64,
    confirm: &mut dyn FnMut(&str) -> bool,
) -> Result<Vec<String>, KestrelError> {
    let one = |s: String| Ok(vec![s]);
    let Some(draft) = draft else {
        return one(
            "There is no draft ready to send: ask for one first, for example: write a mail to Ravi about Friday's leave."
                .into(),
        );
    };
    if !account.internet {
        return one(
            "The internet is off for Kestrel, so nothing can be sent. Turn it on in Settings."
                .into(),
        );
    }
    let Some(from) = account.from.as_deref() else {
        return one(
            "Mail is not set up: add your Gmail address and app password in Settings.".into(),
        );
    };
    let Some(to) = to else {
        return one("Send it to whom? For example: send it to Ravi".into());
    };
    let Some(contact) = memory.contact(to)? else {
        return one(format!(
            "{to} is not in your contacts. Add them first: add contact {to} name@example.com"
        ));
    };
    let draft = with_name(draft, account.name.as_deref());
    let (subject, body) = split_draft(&draft);
    let mail = Mail {
        from: from.to_string(),
        to: contact.email.clone(),
        subject,
        body,
    };
    let what = format!("to {}: {}", mail.to, mail.subject);
    if !confirm(&preview(&mail)) {
        let id = memory.log_outbound(now, "mail", GMAIL, &what, false)?;
        memory.finish_outbound(id, "you said no; nothing left")?;
        return one("Not sent.".into());
    }
    // In the log before it can leave: a send that dies on the way stays visible.
    let id = memory.log_outbound(now, "mail", GMAIL, &what, true)?;
    let token = approval(&link.key(), nonce, &mail);
    let sent = link.ask(Op::SmtpSend {
        mail,
        nonce: nonce.to_string(),
        approval: token,
    });
    match sent {
        Ok(reply) if reply["sent"] == true => {
            memory.finish_outbound(id, "sent")?;
            one(format!("Sent to {} ({}).", contact.name, contact.email))
        }
        Ok(reply) => {
            memory.finish_outbound(id, &format!("failed: {reply}"))?;
            one("The mail was not sent: kestrel-net gave no clear answer.".into())
        }
        Err(why) => {
            memory.finish_outbound(id, &format!("failed: {why}"))?;
            one(format!("The mail was not sent: {why}"))
        }
    }
}

/// The way out and who it sends as, for the assistant.
pub struct Sender {
    link: Box<dyn Link>,
    account: Box<dyn Fn() -> Account>,
}

impl Sender {
    pub fn new(link: Box<dyn Link>, account: Box<dyn Fn() -> Account>) -> Self {
        Self { link, account }
    }

    /// Answers what was said; `draft` is the last email written.
    pub fn handle(
        &self,
        said: Said,
        draft: Option<&str>,
        memory: &mut Memory,
        now: i64,
        confirm: &mut dyn FnMut(&str) -> bool,
    ) -> Vec<String> {
        match said {
            Said::Contact { name, email } => vec![match memory.add_contact(&name, &email) {
                Ok(()) => format!("Okay, {name}'s address is {email}."),
                Err(e) => format!("I could not keep that: {e}"),
            }],
            Said::Send { to } => {
                let not_sent =
                    |e: &dyn std::fmt::Display| vec![format!("The mail was not sent: {e}")];
                let os = kestrel_platform::Os::current();
                let nonce = match kestrel_platform::random_bytes(os, 16) {
                    Ok(bytes) => bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                    Err(e) => return not_sent(&e),
                };
                let account = (self.account)();
                let link = self.link.as_ref();
                send(
                    to.as_deref(),
                    draft,
                    &account,
                    memory,
                    link,
                    &nonce,
                    now,
                    confirm,
                )
                .unwrap_or_else(|e| not_sent(&e))
            }
        }
    }
}

/// The account in Kestrel's data folder.
pub fn account(data: Option<&Path>) -> Account {
    let s = kept(data);
    Account {
        internet: s.internet,
        from: s.mail_from,
        name: s.name,
    }
}

fn kept(data: Option<&Path>) -> crate::settings::Settings {
    data.and_then(|d| crate::settings::load(&crate::settings::path(d)).ok())
        .unwrap_or_default()
}

/// What `GET /api/outbound` answers.
pub fn page_json(memory: &Memory, data: Option<&Path>) -> Result<Value, KestrelError> {
    let s = kept(data);
    Ok(serde_json::json!({
        "internet": s.internet,
        "mail": s.mail_from,
        "contacts": memory.contacts()?,
        "log": memory.outbound(LOG_SHOWN)?,
    }))
}

/// `POST /api/outbound`: the state after, or why not. A password goes to the keyring through `link`
/// and nowhere else, and is never said back.
pub fn change(
    memory: &mut Memory,
    link: &dyn Link,
    data: Option<&Path>,
    change: &Value,
) -> Result<Value, String> {
    const NOT_A_CHANGE: &str = "not a change to the internet, mail or contacts";
    let data = data.ok_or("no data folder for Kestrel's settings")?;
    let (key, value) = change
        .as_object()
        .filter(|o| o.len() == 1)
        .and_then(|o| o.iter().next())
        .ok_or(NOT_A_CHANGE)?;
    let at = crate::settings::path(data);
    let settings = |edit: &dyn Fn(&mut crate::settings::Settings)| {
        let mut s = crate::settings::load(&at).map_err(|e| e.to_string())?;
        edit(&mut s);
        crate::settings::save(&at, &s).map_err(|e| e.to_string())
    };
    match (key.as_str(), value) {
        ("internet", Value::Bool(on)) => settings(&|s| s.internet = *on)?,
        ("mail", v) => {
            let from = v["from"].as_str().unwrap_or("").trim().to_string();
            if !is_address(&from) {
                return Err("that is not one mail address".into());
            }
            // Gmail shows the password in groups: the spaces are not part of it.
            let password: String = (v["password"].as_str().unwrap_or(""))
                .split_whitespace()
                .collect();
            if password.is_empty() {
                return Err("an app password is needed".into());
            }
            link.ask(Op::SecretSet { value: password })?;
            settings(&|s| s.mail_from = Some(from.clone()))?;
        }
        ("forget_mail", Value::Bool(true)) => {
            link.ask(Op::SecretDelete)?;
            settings(&|s| s.mail_from = None)?;
        }
        ("contact", v) => {
            let name = v["name"].as_str().unwrap_or("").trim();
            let email = v["email"].as_str().unwrap_or("").trim();
            if name.is_empty()
                || name.split_whitespace().count() > NAME_WORDS
                || name.chars().any(char::is_control)
            {
                return Err(format!("a name of 1 to {NAME_WORDS} words"));
            }
            if !is_address(email) {
                return Err("that is not one mail address".into());
            }
            memory.add_contact(name, email).map_err(|e| e.to_string())?;
        }
        ("remove_contact", Value::String(name)) => {
            memory.remove_contact(name).map_err(|e| e.to_string())?;
        }
        _ => return Err(NOT_A_CHANGE.into()),
    }
    page_json(memory, Some(data)).map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A stand-in for `kestrel-net`: keeps what it is asked and answers as told.
    #[derive(Clone, Default)]
    pub struct Fake {
        pub asked: Rc<RefCell<Vec<Op>>>,
        pub fail: Option<String>,
    }

    impl Link for Fake {
        fn key(&self) -> String {
            "key".into()
        }
        fn ask(&self, op: Op) -> Result<Value, String> {
            self.asked.borrow_mut().push(op);
            match &self.fail {
                Some(why) => Err(why.clone()),
                None => Ok(serde_json::json!({"sent": true})),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Fake;
    use super::*;

    #[test]
    fn asking_to_send_and_to_keep_a_contact_are_told_from_the_rest() {
        let to = |n: &str| Some(Said::Send { to: Some(n.into()) });
        for (text, want) in [
            ("send it to Ravi", to("Ravi")),
            ("send this to ravi", to("ravi")),
            ("send the mail to Asha.", to("Asha")),
            ("mail it to Ravi", to("Ravi")),
            ("ravi ko bhej do", to("ravi")),
            ("Ravi ko mail bhej do", to("Ravi")),
            ("asha ko email bhej dijiye", to("asha")),
            ("send it", Some(Said::Send { to: None })),
            ("bhej do", Some(Said::Send { to: None })),
            ("ye mail bhej do", Some(Said::Send { to: None })),
            (
                "add contact Ravi ravi@example.com",
                Some(Said::Contact {
                    name: "Ravi".into(),
                    email: "ravi@example.com".into(),
                }),
            ),
            (
                "Ravi's email is ravi@example.com",
                Some(Said::Contact {
                    name: "Ravi".into(),
                    email: "ravi@example.com".into(),
                }),
            ),
            (
                "ravi ka email ravi@example.com hai",
                Some(Said::Contact {
                    name: "ravi".into(),
                    email: "ravi@example.com".into(),
                }),
            ),
            ("send me the file", None),
            ("write a mail to Ravi", None),
            ("ravi ko mail likh do", None),
            ("send a mail to Ravi saying I am late", None),
            ("my email is me@example.com", None),
            ("add contact Ravi", None),
            ("add contact Ravi not-an-address", None),
            ("hello", None),
        ] {
            assert_eq!(parse(text), want, "{text}");
        }
    }

    #[test]
    fn a_draft_splits_at_its_subject_line() {
        assert_eq!(
            split_draft("Subject: Leave\n\nDear Asha,\nAway."),
            ("Leave".to_string(), "Dear Asha,\nAway.".to_string())
        );
        assert_eq!(
            split_draft("Dear Asha,\nAway."),
            ("A message".to_string(), "Dear Asha,\nAway.".to_string())
        );
        assert_eq!(split_draft("**Subject:** Leave\nBody").0, "Leave");
    }

    const DRAFT: &str = "Subject: Leave\n\nDear Ravi,\nAway on Friday.";

    fn ready() -> Account {
        Account {
            internet: true,
            from: Some("me@example.com".into()),
            name: None,
        }
    }

    fn memory() -> Memory {
        let mut m = Memory::in_memory().unwrap();
        m.add_contact("Ravi", "ravi@example.com").unwrap();
        m
    }

    fn go(
        to: Option<&str>,
        draft: Option<&str>,
        account: &Account,
        m: &mut Memory,
        f: &Fake,
        confirm: &mut dyn FnMut(&str) -> bool,
    ) -> Vec<String> {
        send(to, draft, account, m, f, "n1", 100, confirm).unwrap()
    }

    #[test]
    fn a_yes_after_the_exact_mail_sends_it_with_its_token_and_logs_it_first() {
        let (mut m, f) = (memory(), Fake::default());
        let mut shown = String::new();
        let said = go(Some("ravi"), Some(DRAFT), &ready(), &mut m, &f, &mut |q| {
            shown = q.to_string();
            true
        });
        for part in [
            "To: ravi@example.com",
            "From: me@example.com",
            "Subject: Leave",
            "Dear Ravi,\nAway on Friday.",
        ] {
            assert!(shown.contains(part), "{part}: {shown}");
        }
        assert!(
            said[0].contains("Sent") && said[0].contains("ravi@example.com"),
            "{said:?}"
        );
        let asked = f.asked.borrow();
        let Op::SmtpSend {
            mail,
            nonce,
            approval: token,
        } = &asked[0]
        else {
            panic!("{asked:?}");
        };
        assert_eq!(mail.to, "ravi@example.com");
        assert_eq!(mail.body, "Dear Ravi,\nAway on Friday.");
        assert_eq!(token, &approval("key", nonce, mail));
        let log = m.outbound(5).unwrap();
        assert_eq!(log.len(), 1);
        assert!(
            log[0].approved
                && log[0].what.contains("ravi@example.com")
                && log[0].what.contains("Leave")
        );
        assert!(
            !log[0].what.contains("Away on Friday"),
            "recipient and subject, not the text"
        );
        assert_eq!(log[0].result.as_deref(), Some("sent"));
    }

    #[test]
    fn a_no_sends_nothing_and_says_so_in_the_log() {
        let (mut m, f) = (memory(), Fake::default());
        let said = go(Some("Ravi"), Some(DRAFT), &ready(), &mut m, &f, &mut |_| {
            false
        });
        assert!(said[0].contains("Not sent"), "{said:?}");
        assert!(f.asked.borrow().is_empty());
        let log = m.outbound(5).unwrap();
        assert!(!log[0].approved);
        assert_eq!(log[0].result.as_deref(), Some("you said no; nothing left"));
    }

    #[test]
    fn nothing_is_even_asked_without_what_it_takes() {
        let (mut m, f) = (memory(), Fake::default());
        let mut never = |q: &str| -> bool { panic!("asked: {q}") };
        for (to, draft, account, want) in [
            (Some("Ravi"), None, ready(), "no draft"),
            (
                Some("Ravi"),
                Some(DRAFT),
                Account {
                    internet: false,
                    ..ready()
                },
                "internet",
            ),
            (
                Some("Ravi"),
                Some(DRAFT),
                Account {
                    from: None,
                    ..ready()
                },
                "Settings",
            ),
            (None, Some(DRAFT), ready(), "whom"),
            (Some("Asha"), Some(DRAFT), ready(), "not in your contacts"),
        ] {
            let said = go(to, draft, &account, &mut m, &f, &mut never);
            assert!(said[0].contains(want), "{want}: {said:?}");
        }
        assert!(f.asked.borrow().is_empty());
        assert!(
            m.outbound(5).unwrap().is_empty(),
            "nothing tried, nothing logged"
        );
    }

    #[test]
    fn a_draft_with_a_blank_left_says_so_before_the_yes() {
        let (mut m, f) = (memory(), Fake::default());
        let mut shown = String::new();
        go(
            Some("Ravi"),
            Some("Subject: Leave\n\nDear [Manager's name],"),
            &ready(),
            &mut m,
            &f,
            &mut |q| {
                shown = q.to_string();
                false
            },
        );
        assert!(shown.contains("[blank]"), "{shown}");
    }

    #[test]
    fn the_persons_name_fills_the_blank_for_it_and_only_that_one() {
        const WITH_BLANKS: &str =
            "Subject: Leave\n\nDear [Manager],\nAway.\n\nThanks,\n[Your Name]";
        let (mut m, f) = (memory(), Fake::default());
        let named = Account {
            name: Some("Meera".into()),
            ..ready()
        };
        let mut shown = String::new();
        go(
            Some("Ravi"),
            Some(WITH_BLANKS),
            &named,
            &mut m,
            &f,
            &mut |q| {
                shown = q.to_string();
                true
            },
        );
        assert!(shown.contains("Thanks,\nMeera"), "{shown}");
        assert!(
            shown.contains("[Manager]") && shown.contains("[blank]"),
            "{shown}"
        );
        let asked = f.asked.borrow();
        let Op::SmtpSend { mail, .. } = &asked[0] else {
            panic!("{asked:?}");
        };
        assert_eq!(
            mail.body, "Dear [Manager],\nAway.\n\nThanks,\nMeera",
            "what is shown is what leaves"
        );
        for (blank, name, want) in [
            ("Hi,\n[your name]", Some("Meera"), "Hi,\nMeera"),
            ("Hi,\n[YOUR NAME]", Some("Meera"), "Hi,\nMeera"),
            ("Hi,\n[Aapka Naam]", Some("Meera"), "Hi,\nMeera"),
            ("Hi,\n[Your Name]", None, "Hi,\n[Your Name]"),
            ("नमस्ते,\n[Your Name]", Some("मीरा"), "नमस्ते,\nमीरा"),
        ] {
            assert_eq!(with_name(blank, name), want);
        }
    }

    #[test]
    fn only_a_plain_address_is_an_address() {
        for good in ["ravi@example.com", "a.b+c@mail.example.in"] {
            assert!(is_address(good), "{good}");
        }
        for bad in [
            "ravi",
            "@x.in",
            "ravi@x",
            "a@b@c.in",
            "ravi@x.in,asha@x.in",
            "Ravi <ravi@x.in>",
            "ravi@x.in\r\nBcc: e@x.in",
            "ravi@.in",
        ] {
            assert!(!is_address(bad), "{bad}");
        }
    }

    #[test]
    fn a_failed_send_says_why_and_the_log_shows_it() {
        let mut m = memory();
        let f = Fake {
            fail: Some("Gmail did not take the mail".into()),
            ..Fake::default()
        };
        let said = go(Some("Ravi"), Some(DRAFT), &ready(), &mut m, &f, &mut |_| {
            true
        });
        assert!(
            said[0].contains("not sent") && said[0].contains("Gmail"),
            "{said:?}"
        );
        assert!(
            m.outbound(5).unwrap()[0]
                .result
                .as_deref()
                .unwrap()
                .contains("Gmail")
        );
    }

    fn folder(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_send_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_internet_is_off_at_first_and_turned_on_and_off_here() {
        let d = folder("internet");
        let (mut m, f) = (Memory::in_memory().unwrap(), Fake::default());
        let v = page_json(&m, Some(&d)).unwrap();
        assert_eq!(v["internet"], false);
        assert_eq!(v["mail"], Value::Null);
        assert_eq!(v["contacts"], serde_json::json!([]));
        assert_eq!(v["log"], serde_json::json!([]));
        let v = change(&mut m, &f, Some(&d), &serde_json::json!({"internet": true})).unwrap();
        assert_eq!(v["internet"], true);
        assert!(account(Some(&d)).internet);
        let v = change(
            &mut m,
            &f,
            Some(&d),
            &serde_json::json!({"internet": false}),
        )
        .unwrap();
        assert_eq!(v["internet"], false);
        assert!(!account(Some(&d)).internet);
        assert!(
            f.asked.borrow().is_empty(),
            "the switch reaches for nothing"
        );
    }

    #[test]
    fn the_app_password_goes_to_the_keyring_and_nowhere_else() {
        const CANARY: &str = "canary pass word 9876";
        let d = folder("password");
        let db = d.join("memory.db");
        let (mut m, f) = (Memory::open(&db).unwrap(), Fake::default());
        let saved = change(
            &mut m,
            &f,
            Some(&d),
            &serde_json::json!({"mail": {"from": "me@gmail.com", "password": CANARY}}),
        )
        .unwrap();
        // Gmail shows the password in groups: the spaces are not part of it.
        assert_eq!(
            *f.asked.borrow(),
            [Op::SecretSet {
                value: "canarypassword9876".into()
            }]
        );
        assert_eq!(saved["mail"], "me@gmail.com");
        assert_eq!(account(Some(&d)).from.as_deref(), Some("me@gmail.com"));
        drop(m);
        for file in [db, crate::settings::path(&d)] {
            let bytes = std::fs::read(&file).unwrap();
            for secret in [CANARY, "canarypassword9876"] {
                assert!(
                    !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                    "{}",
                    file.display()
                );
            }
        }
        assert!(!saved.to_string().contains("canary"), "{saved}");
    }

    #[test]
    fn a_bad_account_changes_nothing_and_never_says_the_password_back() {
        let d = folder("bad_account");
        let (mut m, f) = (Memory::in_memory().unwrap(), Fake::default());
        for (from, password) in [("not an address", "hunter2"), ("me@gmail.com", "   ")] {
            let e = change(
                &mut m,
                &f,
                Some(&d),
                &serde_json::json!({"mail": {"from": from, "password": password}}),
            )
            .unwrap_err();
            assert!(!e.contains("hunter2"), "{e}");
        }
        assert!(f.asked.borrow().is_empty());
        // The keyring refused: no account is kept.
        let f = Fake {
            fail: Some("no keyring here".into()),
            ..Fake::default()
        };
        let e = change(
            &mut m,
            &f,
            Some(&d),
            &serde_json::json!({"mail": {"from": "me@gmail.com", "password": "hunter2"}}),
        )
        .unwrap_err();
        assert!(e.contains("no keyring") && !e.contains("hunter2"), "{e}");
        assert_eq!(account(Some(&d)).from, None);
    }

    #[test]
    fn forgetting_the_account_removes_the_password_too() {
        let d = folder("forget");
        let (mut m, f) = (Memory::in_memory().unwrap(), Fake::default());
        change(
            &mut m,
            &f,
            Some(&d),
            &serde_json::json!({"mail": {"from": "me@gmail.com", "password": "abcd"}}),
        )
        .unwrap();
        let v = change(
            &mut m,
            &f,
            Some(&d),
            &serde_json::json!({"forget_mail": true}),
        )
        .unwrap();
        assert_eq!(v["mail"], Value::Null);
        assert_eq!(f.asked.borrow().last(), Some(&Op::SecretDelete));
        assert_eq!(account(Some(&d)).from, None);
    }

    #[test]
    fn contacts_are_added_and_removed_and_a_bad_address_refused() {
        let d = folder("contacts");
        let (mut m, f) = (Memory::in_memory().unwrap(), Fake::default());
        let add = |name: &str, email: &str| serde_json::json!({"contact": {"name": name, "email": email}});
        let v = change(&mut m, &f, Some(&d), &add("Ravi", "ravi@example.com")).unwrap();
        assert_eq!(v["contacts"][0]["email"], "ravi@example.com");
        assert!(change(&mut m, &f, Some(&d), &add("Asha", "asha, ravi@example.com")).is_err());
        assert!(change(&mut m, &f, Some(&d), &add(" ", "asha@example.com")).is_err());
        let v = change(
            &mut m,
            &f,
            Some(&d),
            &serde_json::json!({"remove_contact": "Ravi"}),
        )
        .unwrap();
        assert_eq!(v["contacts"], serde_json::json!([]));
        assert!(change(&mut m, &f, Some(&d), &serde_json::json!({"other": 1})).is_err());
    }

    #[test]
    fn the_page_shows_what_left() {
        let d = folder("log");
        let mut m = Memory::in_memory().unwrap();
        let id = m
            .log_outbound(7, "mail", GMAIL, "to r@x.in: Leave", true)
            .unwrap();
        m.finish_outbound(id, "sent").unwrap();
        let v = page_json(&m, Some(&d)).unwrap();
        assert_eq!(v["log"][0]["host"], GMAIL);
        assert_eq!(v["log"][0]["result"], "sent");
    }

    #[test]
    fn the_sender_keeps_a_contact_and_sends_after_a_yes() {
        let (mut m, f) = (Memory::in_memory().unwrap(), Fake::default());
        let sender = Sender::new(Box::new(f.clone()), Box::new(ready));
        let said = sender.handle(
            Said::Contact {
                name: "Ravi".into(),
                email: "ravi@example.com".into(),
            },
            None,
            &mut m,
            1,
            &mut |q| panic!("asked: {q}"),
        );
        assert!(said[0].contains("ravi@example.com"), "{said:?}");
        assert_eq!(m.contact("ravi").unwrap().unwrap().name, "Ravi");
        let to = |n: &str| Said::Send { to: Some(n.into()) };
        for _ in 0..2 {
            let said = sender.handle(to("Ravi"), Some(DRAFT), &mut m, 2, &mut |_| true);
            assert!(said[0].contains("Sent"), "{said:?}");
        }
        // Each send has its own nonce, or kestrel-net would refuse the second.
        let asked = f.asked.borrow();
        let nonces: Vec<&String> = asked
            .iter()
            .map(|op| match op {
                Op::SmtpSend { nonce, .. } => nonce,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(nonces.len(), 2);
        assert_ne!(nonces[0], nonces[1]);
        assert!(nonces[0].len() >= 32);
    }
}
