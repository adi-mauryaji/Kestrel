//! A model file (blueprint §4.3, §4.8): into its `.part` file, continuing what is there. Public https
//! addresses only, checked at each redirect. The core holds the hash and checks the whole file, and it
//! stops this program when the file stops growing, so there is no time limit on the body here.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;

use serde_json::{Value, json};

use crate::fetch::next_url;

/// A model's address goes to a store's own host and on to its CDN; a few are plenty.
const MAX_REDIRECTS: usize = 5;

/// What a server answered.
pub struct Reply {
    pub status: u16,
    pub location: Option<String>,
    /// "bytes 3-5/6", for a part of the file.
    pub content_range: Option<String>,
    pub body: Box<dyn Read>,
}

/// Where the bytes come from: the internet, or a stand-in in tests.
pub trait Source {
    /// May this address be asked at all?
    fn check(&self, url: &str) -> Result<(), String>;
    /// Asks for the file from byte `from` on.
    fn get(&self, url: &str, from: u64) -> Result<Reply, String>;
}

/// The file a request may write: a model's part file and no other.
fn is_part_file(to: &Path) -> bool {
    to.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".gguf.part"))
}

/// Does this part start at byte `from`? "bytes 3-5/6".
fn starts_at(range: Option<&str>, from: u64) -> bool {
    range
        .and_then(|r| r.strip_prefix("bytes "))
        .and_then(|r| r.split('-').next())
        .and_then(|start| start.parse::<u64>().ok())
        == Some(from)
}

pub fn download(source: &dyn Source, url: &str, to: &Path) -> Result<Value, String> {
    if !is_part_file(to) {
        return Err("a download goes only into a model's .part file".into());
    }
    let have = std::fs::metadata(to).map_or(0, |m| m.len());
    let mut at = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        source.check(&at)?;
        let mut reply = source.get(&at, have)?;
        let mut file = match reply.status {
            300..400 => {
                let location = reply.location.ok_or("a redirect without an address")?;
                at = next_url(&at, &location).ok_or("a redirect away from https")?;
                continue;
            }
            // Nothing is left to send: the part is the whole file, and the core checks its hash.
            416 => return Ok(json!({"bytes": have})),
            206 if starts_at(reply.content_range.as_deref(), have) => {
                OpenOptions::new().append(true).create(true).open(to)
            }
            206 => return Err("the server sent another part of the file".into()),
            200 => File::create(to),
            other => return Err(format!("the server said {other}")),
        }
        .map_err(|e| format!("{} could not be written: {e}", to.display()))?;
        std::io::copy(&mut reply.body, &mut file)
            .map_err(|e| format!("the download stopped: {e}"))?;
        let bytes = file
            .metadata()
            .map_err(|e| format!("{} could not be read: {e}", to.display()))?
            .len();
        return Ok(json!({"bytes": bytes}));
    }
    Err("too many redirects".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_dl_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn part(d: &Path, has: &[u8]) -> PathBuf {
        let p = d.join("model.gguf.part");
        if !has.is_empty() {
            std::fs::write(&p, has).unwrap();
        }
        p
    }

    /// Status, location, content range, body.
    type Answer = (
        u16,
        Option<&'static str>,
        Option<&'static str>,
        &'static [u8],
    );

    /// Answers each request from a list; keeps what was asked and checked.
    #[derive(Default)]
    struct Fake {
        replies: RefCell<Vec<Answer>>,
        asked: RefCell<Vec<(String, u64)>>,
        checked: RefCell<Vec<String>>,
        refuse: Option<&'static str>,
    }

    impl Source for Fake {
        fn check(&self, url: &str) -> Result<(), String> {
            self.checked.borrow_mut().push(url.to_string());
            match self.refuse {
                Some(host) if url.contains(host) => {
                    Err(format!("{host} is not on the public internet"))
                }
                _ => Ok(()),
            }
        }
        fn get(&self, url: &str, from: u64) -> Result<Reply, String> {
            self.asked.borrow_mut().push((url.to_string(), from));
            let (status, location, range, body) = self.replies.borrow_mut().remove(0);
            Ok(Reply {
                status,
                location: location.map(String::from),
                content_range: range.map(String::from),
                body: Box::new(body),
            })
        }
    }

    fn answers(r: Vec<Answer>) -> Fake {
        Fake {
            replies: RefCell::new(r),
            ..Fake::default()
        }
    }

    const URL: &str = "https://models.example/m.gguf";

    #[test]
    fn a_new_download_is_written_whole() {
        let d = folder("new");
        let p = part(&d, b"");
        let f = answers(vec![(200, None, None, b"abcdef")]);
        assert_eq!(download(&f, URL, &p).unwrap()["bytes"], 6);
        assert_eq!(std::fs::read(&p).unwrap(), b"abcdef");
        assert_eq!(f.asked.borrow()[0].1, 0);
    }

    #[test]
    fn a_part_continues_from_its_end() {
        let d = folder("resume");
        let p = part(&d, b"abc");
        let f = answers(vec![(206, None, Some("bytes 3-5/6"), b"def")]);
        assert_eq!(download(&f, URL, &p).unwrap()["bytes"], 6);
        assert_eq!(std::fs::read(&p).unwrap(), b"abcdef");
        assert_eq!(f.asked.borrow()[0].1, 3);
    }

    #[test]
    fn a_server_that_ignores_the_range_starts_again() {
        let d = folder("ignores");
        let p = part(&d, b"abc");
        let f = answers(vec![(200, None, None, b"abcdef")]);
        assert_eq!(download(&f, URL, &p).unwrap()["bytes"], 6);
        assert_eq!(std::fs::read(&p).unwrap(), b"abcdef");
    }

    #[test]
    fn a_range_from_elsewhere_is_refused_and_the_part_kept() {
        let d = folder("elsewhere");
        for range in [Some("bytes 0-5/6"), Some("nonsense"), None] {
            let p = part(&d, b"abc");
            let f = answers(vec![(206, None, range, b"def")]);
            assert!(download(&f, URL, &p).is_err(), "{range:?}");
            assert_eq!(std::fs::read(&p).unwrap(), b"abc", "{range:?}");
        }
    }

    #[test]
    fn a_part_already_whole_is_done() {
        let d = folder("whole");
        let p = part(&d, b"abcdef");
        let f = answers(vec![(416, None, None, b"")]);
        assert_eq!(download(&f, URL, &p).unwrap()["bytes"], 6);
        assert_eq!(std::fs::read(&p).unwrap(), b"abcdef");
    }

    #[test]
    fn redirects_are_followed_and_each_address_checked() {
        let d = folder("redirect");
        let p = part(&d, b"");
        let f = answers(vec![
            (302, Some("https://cdn.example/x"), None, b""),
            (200, None, None, b"abc"),
        ]);
        download(&f, URL, &p).unwrap();
        assert_eq!(*f.checked.borrow(), [URL, "https://cdn.example/x"]);
        let f = Fake {
            refuse: Some("192.168.1.1"),
            ..answers(vec![(302, Some("https://192.168.1.1/x"), None, b"")])
        };
        let e = download(&f, URL, &p).unwrap_err();
        assert!(e.contains("not on the public internet"), "{e}");
        assert_eq!(f.asked.borrow().len(), 1, "never asked");
        let f = answers(vec![(302, Some("http://cdn.example/x"), None, b"")]);
        assert!(download(&f, URL, &p).is_err(), "away from https");
    }

    #[test]
    fn too_many_redirects_end_it() {
        let d = folder("loop");
        let p = part(&d, b"");
        let f = answers(vec![(302, Some("/again"), None, b""); 9]);
        assert!(download(&f, URL, &p).unwrap_err().contains("redirects"));
    }

    #[test]
    fn an_error_is_said_and_nothing_is_written() {
        let d = folder("error");
        let p = part(&d, b"");
        let f = answers(vec![(404, None, None, b"not here")]);
        assert!(download(&f, URL, &p).unwrap_err().contains("404"));
        assert!(!p.exists());
    }

    #[test]
    fn only_a_model_part_file_is_written() {
        let d = folder("only");
        for name in [".bashrc", "model.gguf", "notes.part"] {
            let f = answers(vec![(200, None, None, b"abc")]);
            assert!(download(&f, URL, &d.join(name)).is_err(), "{name}");
            assert!(!d.join(name).exists(), "{name}");
            assert!(f.asked.borrow().is_empty(), "{name}");
        }
    }
}
