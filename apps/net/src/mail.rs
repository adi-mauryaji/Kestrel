//! A mail through Gmail's SMTP with an app password (blueprint §4.8). One sender, one recipient; a
//! line break in the subject cannot become a header.

use std::time::Duration;

use kestrel_net_proto::Mail;
use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::SmtpTransport;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, Transport};

/// The message, or why it cannot be one.
pub fn message(mail: &Mail) -> Result<Message, String> {
    let address = |text: &str| {
        text.trim()
            .parse::<Mailbox>()
            .map_err(|_| format!("{text:?} is not one mail address"))
    };
    let subject: String = mail
        .subject
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    Message::builder()
        .from(address(&mail.from)?)
        .to(address(&mail.to)?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(mail.body.clone())
        .map_err(|e| format!("the mail could not be made: {e}"))
}

pub fn send_with_gmail(mail: &Mail, password: &str) -> Result<(), String> {
    let message = message(mail)?;
    let transport = SmtpTransport::relay("smtp.gmail.com")
        .map_err(|e| format!("Gmail could not be reached: {e}"))?
        .credentials(Credentials::new(mail.from.clone(), password.to_string()))
        .timeout(Some(Duration::from_secs(30)))
        .build();
    transport
        .send(&message)
        .map(|_| ())
        .map_err(|e| format!("Gmail did not take the mail: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn mail() -> Mail {
        Mail {
            from: "me@example.com".into(),
            to: "ravi@example.com".into(),
            subject: "Leave".into(),
            body: "Away on Friday.\nअगले हफ्ते.".into(),
        }
    }

    /// An SMTP server on this machine that keeps what it is sent.
    fn server() -> (u16, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            let mut all = String::new();
            let mut data = false;
            writeln!(w, "220 test\r").unwrap();
            let mut line = String::new();
            while r.read_line(&mut line).unwrap() > 0 {
                all.push_str(&line);
                let upper = line.trim_end().to_uppercase();
                let reply = if data {
                    if line.trim_end() == "." {
                        data = false;
                        "250 ok"
                    } else {
                        ""
                    }
                } else if upper == "DATA" {
                    data = true;
                    "354 go"
                } else if upper == "QUIT" {
                    writeln!(w, "221 bye\r").unwrap();
                    break;
                } else {
                    "250 ok"
                };
                if !reply.is_empty() {
                    writeln!(w, "{reply}\r").unwrap();
                }
                line.clear();
            }
            all
        });
        (port, h)
    }

    #[test]
    fn the_message_reaches_the_server_as_previewed() {
        let (port, h) = server();
        let t = SmtpTransport::builder_dangerous("127.0.0.1")
            .port(port)
            .build();
        t.send(&message(&mail()).unwrap()).unwrap();
        drop(t);
        let said = h.join().unwrap();
        assert!(said.contains("MAIL FROM:<me@example.com>"), "{said}");
        assert!(said.contains("RCPT TO:<ravi@example.com>"), "{said}");
        assert!(said.contains("Subject: Leave"), "{said}");
    }

    #[test]
    fn nothing_extra_can_get_into_the_headers_or_the_recipients() {
        let mut m = mail();
        m.subject = "Hi\r\nBcc: evil@example.com".into();
        let raw = String::from_utf8(message(&m).unwrap().formatted()).unwrap();
        let headers = raw.split("\r\n\r\n").next().unwrap();
        assert!(
            !headers
                .lines()
                .any(|l| l.to_lowercase().starts_with("bcc:")),
            "{headers}"
        );
        for to in [
            "a@example.com, b@example.com",
            "not an address",
            "a@example.com\r\nBcc: x@y.z",
            "",
        ] {
            let mut m = mail();
            m.to = to.into();
            assert!(message(&m).is_err(), "{to}");
        }
    }
}
