//! One request at a time: the operation, or why not. A mail needs its approval token (kestrel-net-proto),
//! each usable once; the log of what left is the core's, written before it asks.

use std::collections::HashSet;

use kestrel_net_proto::{Mail, Op, Request, approval};
use serde_json::{Value, json};

/// What reaches the outside world, apart for the tests.
pub trait Outside {
    fn fetch(&self, url: &str) -> Result<Value, String>;
    fn send(&self, mail: &Mail, password: &str) -> Result<(), String>;
    fn download(&self, url: &str, to: &str) -> Result<Value, String>;
    fn password(&self) -> Result<Option<String>, String>;
    fn set_password(&self, value: &str) -> Result<(), String>;
    fn delete_password(&self) -> Result<(), String>;
}

/// The real ones.
pub struct Real;

impl Outside for Real {
    fn fetch(&self, url: &str) -> Result<Value, String> {
        crate::fetch::fetch(url)
    }
    fn send(&self, mail: &Mail, password: &str) -> Result<(), String> {
        crate::mail::send_with_gmail(mail, password)
    }
    fn download(&self, url: &str, to: &str) -> Result<Value, String> {
        crate::download::download(&crate::fetch::Internet, url, std::path::Path::new(to))
    }
    fn password(&self) -> Result<Option<String>, String> {
        crate::secrets::get()
    }
    fn set_password(&self, value: &str) -> Result<(), String> {
        crate::secrets::set(value)
    }
    fn delete_password(&self) -> Result<(), String> {
        crate::secrets::delete()
    }
}

pub struct Worker<O> {
    key: String,
    used: HashSet<String>,
    outside: O,
}

impl<O: Outside> Worker<O> {
    pub fn new(key: String, outside: O) -> Self {
        Self {
            key,
            used: HashSet::new(),
            outside,
        }
    }

    /// The reply to one line.
    pub fn answer(&mut self, line: &str) -> Value {
        let Ok(request) = serde_json::from_str::<Request>(line) else {
            return json!({"error": "not an operation"});
        };
        let id = request.id;
        match self.carry_out(request.op) {
            Ok(Value::Object(mut reply)) => {
                reply.insert("id".into(), id.into());
                reply.into()
            }
            Ok(other) => json!({"id": id, "reply": other}),
            Err(why) => json!({"id": id, "error": why}),
        }
    }

    fn carry_out(&mut self, op: Op) -> Result<Value, String> {
        match op {
            Op::Fetch { url } => self.outside.fetch(&url),
            Op::SmtpSend {
                mail,
                nonce,
                approval: token,
            } => {
                if token != approval(&self.key, &nonce, &mail) {
                    return Err("this mail has no approval: nothing was sent".into());
                }
                // Used from here on, whatever happens: a yes sends at most once.
                if !self.used.insert(nonce) {
                    return Err("this approval was used already: nothing was sent".into());
                }
                let password = self
                    .outside
                    .password()?
                    .ok_or("no mail password in the keyring: add it in Settings")?;
                self.outside.send(&mail, &password)?;
                Ok(json!({"sent": true}))
            }
            Op::SecretSet { value } => {
                self.outside.set_password(&value)?;
                Ok(json!({"saved": true}))
            }
            Op::Download { url, to } => self.outside.download(&url, &to),
            Op::SecretHas => Ok(json!({"has": self.outside.password()?.is_some()})),
            Op::SecretDelete => {
                self.outside.delete_password()?;
                Ok(json!({"deleted": true}))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Seen {
        sent: Vec<Mail>,
        password: Option<String>,
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<RefCell<Seen>>);

    impl Outside for Fake {
        fn fetch(&self, url: &str) -> Result<Value, String> {
            Ok(json!({"status": 200, "text": url}))
        }
        fn send(&self, mail: &Mail, password: &str) -> Result<(), String> {
            assert_eq!(password, "app-password");
            self.0.borrow_mut().sent.push(mail.clone());
            Ok(())
        }
        fn download(&self, url: &str, to: &str) -> Result<Value, String> {
            Ok(json!({"bytes": url.len() + to.len()}))
        }
        fn password(&self) -> Result<Option<String>, String> {
            Ok(self.0.borrow().password.clone())
        }
        fn set_password(&self, value: &str) -> Result<(), String> {
            self.0.borrow_mut().password = Some(value.to_string());
            Ok(())
        }
        fn delete_password(&self) -> Result<(), String> {
            self.0.borrow_mut().password = None;
            Ok(())
        }
    }

    fn mail() -> Mail {
        Mail {
            from: "me@example.com".into(),
            to: "ravi@example.com".into(),
            subject: "Leave".into(),
            body: "Away on Friday.".into(),
        }
    }

    fn send(id: u64, mail: &Mail, nonce: &str, token: &str) -> String {
        serde_json::to_string(&Request {
            id,
            op: Op::SmtpSend {
                mail: mail.clone(),
                nonce: nonce.into(),
                approval: token.into(),
            },
        })
        .unwrap()
    }

    fn worker() -> (Worker<Fake>, Fake) {
        let fake = Fake::default();
        let mut w = Worker::new("key".into(), fake.clone());
        w.answer(r#"{"id":1,"op":"secret_set","value":"app-password"}"#);
        (w, fake)
    }

    #[test]
    fn a_mail_goes_only_with_its_token_and_only_once() {
        let (mut w, fake) = worker();
        let good = approval("key", "n1", &mail());
        for (line, why) in [
            (send(2, &mail(), "n1", "nonsense"), "a made-up token"),
            (
                send(3, &mail(), "n1", &approval("other", "n1", &mail())),
                "another key's",
            ),
            (send(4, &mail(), "n2", &good), "another nonce's"),
        ] {
            assert!(w.answer(&line)["error"].is_string(), "{why}");
        }
        let mut changed = mail();
        changed.to = "evil@example.com".into();
        assert!(
            w.answer(&send(5, &changed, "n1", &good))["error"].is_string(),
            "the text changed"
        );
        assert!(fake.0.borrow().sent.is_empty(), "nothing has left");

        let v = w.answer(&send(6, &mail(), "n1", &good));
        assert_eq!(v, json!({"id": 6, "sent": true}));
        assert_eq!(fake.0.borrow().sent.len(), 1);
        let v = w.answer(&send(7, &mail(), "n1", &good));
        assert!(v["error"].as_str().unwrap().contains("used"), "{v}");
        assert_eq!(fake.0.borrow().sent.len(), 1, "once");
    }

    #[test]
    fn a_failed_check_does_not_use_up_a_token() {
        let (mut w, fake) = worker();
        let good = approval("key", "n1", &mail());
        assert!(w.answer(&send(2, &mail(), "n1", "wrong"))["error"].is_string());
        assert_eq!(w.answer(&send(3, &mail(), "n1", &good))["sent"], true);
        assert_eq!(fake.0.borrow().sent.len(), 1);
    }

    #[test]
    fn without_a_password_nothing_is_sent_and_the_password_never_comes_back() {
        let fake = Fake::default();
        let mut w = Worker::new("key".into(), fake.clone());
        let line = send(1, &mail(), "n1", &approval("key", "n1", &mail()));
        let v = w.answer(&line);
        assert!(v["error"].as_str().unwrap().contains("password"), "{v}");
        assert_eq!(
            w.answer(r#"{"id":2,"op":"secret_has"}"#),
            json!({"id": 2, "has": false})
        );
        let v = w.answer(r#"{"id":3,"op":"secret_set","value":"app-password"}"#);
        assert_eq!(v, json!({"id": 3, "saved": true}));
        assert!(!v.to_string().contains("app-password"));
        assert_eq!(
            w.answer(r#"{"id":4,"op":"secret_has"}"#),
            json!({"id": 4, "has": true})
        );
        w.answer(r#"{"id":5,"op":"secret_delete"}"#);
        assert_eq!(w.answer(r#"{"id":6,"op":"secret_has"}"#)["has"], false);
    }

    #[test]
    fn a_page_comes_back_and_what_is_not_an_operation_is_refused() {
        let (mut w, _) = worker();
        let v = w.answer(r#"{"id":2,"op":"fetch","url":"https://example.com/"}"#);
        assert_eq!(v["id"], 2);
        assert_eq!(v["status"], 200);
        for line in ["not json", r#"{"id":3,"op":"shell","cmd":"ls"}"#, "{}"] {
            assert!(w.answer(line)["error"].is_string(), "{line}");
        }
    }
}
