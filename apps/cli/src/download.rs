//! A model from the internet (v4 C, blueprint §4.3): through `kestrel-net`, logged first, into a `.part`
//! file that a later try continues, and kept only when its SHA-256 is the catalog's. The page starts
//! it and asks how far it is (`Job`, `/api/model/download`), on its own thread so the service keeps
//! answering. It ends when asked, when the internet is turned off, or when the file stops growing.

use std::cell::Cell;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kestrel_capability::{Catalog, Model};
use kestrel_memory::Memory;
use kestrel_net_proto::Op;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::send::Link;

/// With no new byte for this long, the download is stopped; the next try continues it.
const STALL: Duration = Duration::from_secs(120);

/// How a download is watched.
pub struct Watch<'a> {
    pub progress: &'a dyn Fn(u64, u64),
    /// The file is whole and its hash is being checked.
    pub checking: &'a dyn Fn(),
    pub stop: &'a dyn Fn() -> bool,
    pub stall: Duration,
}

/// How it ended, when nothing went wrong.
#[derive(Debug, PartialEq)]
pub enum Ended {
    Done(PathBuf),
    Stopped,
}

/// The part file a model comes down into.
fn part_of(model: &Model, data: &Path) -> PathBuf {
    data.join("models").join(format!("{}.part", model.file))
}

/// The SHA-256 of a file.
fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let (mut hash, mut buf) = (Sha256::new(), vec![0; 1 << 20]);
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Brings `model` into `data`/models through `link`; the log says what happened.
pub fn get_model(
    model: &Model,
    data: &Path,
    link: &dyn Link,
    memory: &mut Memory,
    now: i64,
    watch: &Watch<'_>,
) -> Result<Ended, String> {
    let (Some(url), Some(sha256)) = (model.url.as_deref(), model.sha256.as_deref()) else {
        return Err(format!("{} has no address to download it from", model.id));
    };
    let host = kestrel_net_proto::https_host(url).ok_or("only https addresses")?;
    let dir = data.join("models");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let part = part_of(model, data);
    let what = format!(
        "{} ({})",
        model.id,
        kestrel_capability::size(model.file_mib)
    );
    // In the log before anything is asked: a download that dies on the way stays visible.
    let id = memory
        .log_outbound(now, "download", &host, &what, true)
        .map_err(|e| e.to_string())?;
    let all = model.file_mib * (1 << 20);
    let bytes = || std::fs::metadata(&part).map_or(0, |m| m.len());
    let (mut seen, mut grew) = (bytes(), Instant::now());
    let (stopped, stalled) = (Cell::new(false), Cell::new(false));
    let mut still = || {
        let now = bytes();
        (watch.progress)(now, all);
        if now != seen {
            (seen, grew) = (now, Instant::now());
        }
        stopped.set((watch.stop)());
        stalled.set(grew.elapsed() >= watch.stall);
        !stopped.get() && !stalled.get()
    };
    let asked = link.ask_while(
        Op::Download {
            url: url.to_string(),
            to: part.to_string_lossy().into_owned(),
        },
        &mut still,
    );
    let finished = |memory: &mut Memory, result: &str| {
        memory
            .finish_outbound(id, result)
            .map_err(|e| e.to_string())
    };
    if stopped.get() {
        finished(memory, "stopped")?;
        return Ok(Ended::Stopped);
    }
    if stalled.get() {
        let why = "the file stopped coming; Download continues from here";
        finished(memory, &format!("failed: {why}"))?;
        return Err(why.into());
    }
    if let Err(why) = asked {
        finished(memory, &format!("failed: {why}"))?;
        return Err(why);
    }
    (watch.checking)();
    let same = sha256_of(&part).map_err(|e| e.to_string())? == sha256;
    if !same {
        // Not kept: a later try must not continue a file that is wrong.
        let _ = std::fs::remove_file(&part);
        let why = format!(
            "the file did not match {}'s SHA-256: nothing was kept",
            model.id
        );
        finished(memory, &format!("failed: {why}"))?;
        return Err(why);
    }
    let file = dir.join(&model.file);
    std::fs::rename(&part, &file).map_err(|e| e.to_string())?;
    finished(memory, "done")?;
    Ok(Ended::Done(file))
}

/// What the state of the job is.
#[derive(Clone, Debug, PartialEq)]
enum State {
    Idle,
    Getting { id: String, done: u64, all: u64 },
    Checking { id: String },
    Done { id: String },
    Stopped { id: String },
    Failed { why: String },
}

/// One download at a time, for the page.
#[derive(Clone)]
pub struct Job {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl Default for Job {
    fn default() -> Self {
        Job {
            state: Arc::new(Mutex::new(State::Idle)),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Job {
    /// Starts bringing `model` in on a thread of its own; `off` says the person turned the internet
    /// off, and `open` makes its way out and its memory there.
    pub fn start(
        &self,
        model: Model,
        data: PathBuf,
        off: impl Fn() -> bool + Send + 'static,
        open: impl FnOnce() -> Result<(Box<dyn Link>, Memory), String> + Send + 'static,
    ) -> Result<(), String> {
        let id = model.id.clone();
        {
            let mut now = self
                .state
                .lock()
                .map_err(|_| "the download's state was lost")?;
            if matches!(*now, State::Getting { .. } | State::Checking { .. }) {
                return Err("a model is being downloaded already".into());
            }
            *now = State::Getting {
                id: id.clone(),
                done: 0,
                all: model.file_mib * (1 << 20),
            };
        }
        self.stop.store(false, Ordering::Relaxed);
        let (state, stop) = (self.state.clone(), self.stop.clone());
        std::thread::spawn(move || {
            let set = |s: State| {
                if let Ok(mut now) = state.lock() {
                    *now = s;
                }
            };
            let got = open().and_then(|(link, mut memory)| {
                let unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                let watch = Watch {
                    progress: &|done, all| {
                        set(State::Getting {
                            id: id.clone(),
                            done,
                            all,
                        })
                    },
                    checking: &|| set(State::Checking { id: id.clone() }),
                    stop: &|| stop.load(Ordering::Relaxed) || off(),
                    stall: STALL,
                };
                get_model(&model, &data, link.as_ref(), &mut memory, unix, &watch)
            });
            set(match got {
                Ok(Ended::Done(_)) => State::Done { id: id.clone() },
                Ok(Ended::Stopped) => State::Stopped { id: id.clone() },
                Err(why) => State::Failed { why },
            });
        });
        Ok(())
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Where it stands, for the page.
    pub fn json(&self) -> Value {
        let now = self.state.lock().map(|s| s.clone());
        match now.unwrap_or(State::Failed {
            why: "the download's state was lost".into(),
        }) {
            State::Idle => json!({"state": "idle"}),
            State::Getting { id, done, all } => {
                json!({"state": "getting", "model": id, "done": done, "all": all})
            }
            State::Checking { id } => json!({"state": "checking", "model": id}),
            State::Done { id } => json!({"state": "done", "model": id}),
            State::Stopped { id } => json!({"state": "stopped", "model": id}),
            State::Failed { why } => json!({"state": "failed", "why": why}),
        }
    }
}

/// The models that can come down here: an address and a hash, and not on this device yet.
pub fn offers(catalog: &Catalog, have: &dyn Fn(&str) -> bool) -> Vec<Model> {
    (catalog.models.iter())
        .filter(|m| m.url.is_some() && m.sha256.is_some() && !have(&m.file))
        .cloned()
        .collect()
}

/// What `GET /api/model/download` answers. `here`: how much of a model's file came already.
pub fn page_json(
    job: &Job,
    catalog: &Catalog,
    have: &dyn Fn(&str) -> bool,
    internet: bool,
    here: &dyn Fn(&Model) -> u64,
) -> Value {
    let mut v = job.json();
    v["internet"] = internet.into();
    v["offers"] = offers(catalog, have)
        .iter()
        .map(|m| json!({"id": m.id, "mib": m.file_mib, "here": here(m)}))
        .collect();
    v
}

/// How much of `model` a stopped download left in `data`.
pub fn part_bytes(model: &Model, data: &Path) -> u64 {
    std::fs::metadata(part_of(model, data)).map_or(0, |m| m.len())
}

/// `POST /api/model/download`: {"stop": true}, or {"id": ...} to start: refused with the internet off.
pub fn change(
    job: &Job,
    catalog: &Catalog,
    have: &dyn Fn(&str) -> bool,
    internet: bool,
    change: &Value,
    start: impl FnOnce(&Job, Model) -> Result<(), String>,
) -> Result<(), String> {
    if change["stop"] == true {
        job.stop();
        return Ok(());
    }
    let id = change["id"]
        .as_str()
        .ok_or("send {\"id\": \"...\"} or {\"stop\": true}")?;
    if !internet {
        return Err("The internet is off for Kestrel: turn it on in Settings first.".into());
    }
    let model = offers(catalog, have)
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("{id} is not a model to download here"))?;
    start(job, model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::send::fake::Fake;
    use std::cell::RefCell;

    /// SHA-256 of "abc" (FIPS 180-2, appendix B.1).
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn folder(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kestrel_getmodel_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn model() -> Model {
        serde_json::from_value(json!({
            "id": "tiny-writer", "file": "tiny.gguf", "file_mib": 1, "memory_mib": 1,
            "role": "writer", "estimate": false, "sha256": ABC, "source": "test",
            "url": "https://models.example/tiny.gguf"
        }))
        .unwrap()
    }

    fn watch<'a>(stop: &'a dyn Fn() -> bool) -> Watch<'a> {
        Watch {
            progress: &|_, _| {},
            checking: &|| {},
            stop,
            stall: Duration::from_secs(60),
        }
    }

    /// Writes `bytes` to the file it is asked for, then answers `end`; `then` runs while it "downloads".
    struct Writes {
        bytes: &'static [u8],
        end: Result<(), &'static str>,
        asked: RefCell<Vec<Op>>,
        during: Box<dyn Fn()>,
        polls: u32,
    }

    impl Writes {
        fn new(bytes: &'static [u8], end: Result<(), &'static str>) -> Self {
            Self {
                bytes,
                end,
                asked: RefCell::default(),
                during: Box::new(|| {}),
                polls: 0,
            }
        }
    }

    impl Link for Writes {
        fn key(&self) -> String {
            "key".into()
        }
        fn ask(&self, _: Op) -> Result<Value, String> {
            panic!("a download is watched: ask_while");
        }
        fn ask_while(&self, op: Op, still: &mut dyn FnMut() -> bool) -> Result<Value, String> {
            let Op::Download { to, .. } = &op else {
                panic!("{op:?}");
            };
            let mut have = std::fs::read(to).unwrap_or_default();
            have.extend_from_slice(self.bytes);
            std::fs::write(to, have).unwrap();
            (self.during)();
            self.asked.borrow_mut().push(op.clone());
            for _ in 0..self.polls {
                if !still() {
                    return Err("stopped".into());
                }
            }
            match self.end {
                Ok(()) => Ok(json!({"bytes": self.bytes.len()})),
                Err(why) => Err(why.to_string()),
            }
        }
    }

    #[test]
    fn a_model_comes_down_checked_and_logged_first() {
        let d = folder("whole");
        let db = d.join("memory.db");
        let mut m = Memory::open(&db).unwrap();
        let during = db.clone();
        let link = Writes {
            during: Box::new(move || {
                let log = Memory::open(&during).unwrap().outbound(9).unwrap();
                assert_eq!(log.len(), 1, "in the log before anything comes");
                assert_eq!(log[0].result, None);
                assert_eq!(
                    (log[0].kind.as_str(), log[0].host.as_str()),
                    ("download", "models.example")
                );
                assert!(log[0].what.contains("tiny-writer"));
            }),
            ..Writes::new(b"abc", Ok(()))
        };
        let checked = Cell::new(false);
        let w = Watch {
            checking: &|| checked.set(true),
            ..watch(&|| false)
        };
        let got = get_model(&model(), &d, &link, &mut m, 5, &w).unwrap();
        let file = d.join("models/tiny.gguf");
        assert_eq!(got, Ended::Done(file.clone()));
        assert_eq!(std::fs::read(&file).unwrap(), b"abc");
        assert!(!d.join("models/tiny.gguf.part").exists());
        assert!(checked.get());
        assert_eq!(m.outbound(9).unwrap()[0].result.as_deref(), Some("done"));
        let asked = link.asked.borrow();
        let Op::Download { url, to } = &asked[0] else {
            panic!();
        };
        assert_eq!(url, "https://models.example/tiny.gguf");
        assert!(to.ends_with("tiny.gguf.part"), "{to}");
    }

    #[test]
    fn a_file_that_is_not_the_model_is_not_kept() {
        let d = folder("wrong");
        let mut m = Memory::in_memory().unwrap();
        let link = Writes::new(b"not the model", Ok(()));
        let e = get_model(&model(), &d, &link, &mut m, 5, &watch(&|| false)).unwrap_err();
        assert!(e.contains("SHA-256"), "{e}");
        assert!(!d.join("models/tiny.gguf").exists());
        assert!(
            !d.join("models/tiny.gguf.part").exists(),
            "a bad part must not be continued"
        );
        assert!(
            m.outbound(9).unwrap()[0]
                .result
                .as_deref()
                .unwrap()
                .contains("SHA-256")
        );
    }

    #[test]
    fn a_failure_keeps_what_came_and_says_why_and_the_next_try_continues() {
        let d = folder("again");
        let mut m = Memory::in_memory().unwrap();
        let link = Writes::new(b"ab", Err("the download stopped: reset"));
        let e = get_model(&model(), &d, &link, &mut m, 5, &watch(&|| false)).unwrap_err();
        assert!(e.contains("reset"), "{e}");
        assert_eq!(
            std::fs::read(d.join("models/tiny.gguf.part")).unwrap(),
            b"ab"
        );
        assert!(
            m.outbound(9).unwrap()[0]
                .result
                .as_deref()
                .unwrap()
                .contains("reset")
        );
        let link = Writes::new(b"c", Ok(()));
        let got = get_model(&model(), &d, &link, &mut m, 6, &watch(&|| false)).unwrap();
        assert_eq!(got, Ended::Done(d.join("models/tiny.gguf")));
        assert_eq!(m.outbound(9).unwrap().len(), 2, "each try in the log");
    }

    #[test]
    fn stop_ends_it_and_keeps_what_came() {
        let d = folder("stop");
        let mut m = Memory::in_memory().unwrap();
        let link = Writes {
            polls: 1,
            ..Writes::new(b"ab", Ok(()))
        };
        let got = get_model(&model(), &d, &link, &mut m, 5, &watch(&|| true)).unwrap();
        assert_eq!(got, Ended::Stopped);
        assert!(d.join("models/tiny.gguf.part").exists());
        assert_eq!(m.outbound(9).unwrap()[0].result.as_deref(), Some("stopped"));
    }

    #[test]
    fn a_file_that_stops_growing_ends_it() {
        let d = folder("stall");
        let mut m = Memory::in_memory().unwrap();
        let link = Writes {
            polls: 3,
            ..Writes::new(b"ab", Ok(()))
        };
        let w = Watch {
            stall: Duration::ZERO,
            ..watch(&|| false)
        };
        let e = get_model(&model(), &d, &link, &mut m, 5, &w).unwrap_err();
        assert!(e.contains("stopped coming"), "{e}");
        assert!(d.join("models/tiny.gguf.part").exists());
    }

    #[test]
    fn a_model_without_an_address_or_a_hash_is_not_tried() {
        let d = folder("no_url");
        let mut m = Memory::in_memory().unwrap();
        let link = Fake::default();
        for strip in [
            |x: &mut Model| x.url = None,
            |x: &mut Model| x.sha256 = None,
        ] {
            let mut bare = model();
            strip(&mut bare);
            assert!(get_model(&bare, &d, &link, &mut m, 5, &watch(&|| false)).is_err());
        }
        assert!(m.outbound(9).unwrap().is_empty());
    }

    fn catalog() -> Catalog {
        let mut no_url = model();
        no_url.id = "other".into();
        no_url.file = "other.gguf".into();
        no_url.url = None;
        Catalog {
            models: vec![model(), no_url],
        }
    }

    #[test]
    fn only_a_model_with_an_address_that_is_not_here_is_offered() {
        let c = catalog();
        let ids = |have: &dyn Fn(&str) -> bool| -> Vec<String> {
            offers(&c, have).into_iter().map(|m| m.id).collect()
        };
        assert_eq!(ids(&|_| false), ["tiny-writer"]);
        assert!(ids(&|f| f == "tiny.gguf").is_empty());
        let v = page_json(&Job::default(), &c, &|_| false, true, &|_| 7);
        assert_eq!(v["state"], "idle");
        assert_eq!(v["internet"], true);
        assert_eq!(v["offers"][0]["id"], "tiny-writer");
        assert_eq!(v["offers"][0]["mib"], 1);
        assert_eq!(v["offers"][0]["here"], 7);
    }

    #[test]
    fn starting_needs_the_internet_and_a_model_on_offer_and_stop_needs_neither() {
        let (c, job) = (catalog(), Job::default());
        let start = |_: &Job, m: Model| -> Result<(), String> {
            assert_eq!(m.id, "tiny-writer");
            Ok(())
        };
        let no = |_: &Job, m: Model| -> Result<(), String> { panic!("started {}", m.id) };
        let id = json!({"id": "tiny-writer"});
        let e = change(&job, &c, &|_| false, false, &id, no).unwrap_err();
        assert!(e.contains("internet is off"), "{e}");
        let other = json!({"id": "other"});
        assert!(
            change(&job, &c, &|_| false, true, &other, no).is_err(),
            "no address"
        );
        assert!(
            change(&job, &c, &|_| true, true, &id, no).is_err(),
            "already here"
        );
        assert!(change(&job, &c, &|_| false, true, &json!({"x": 1}), no).is_err());
        change(&job, &c, &|_| false, true, &id, start).unwrap();
        change(&job, &c, &|_| false, false, &json!({"stop": true}), no).unwrap();
        assert!(job.stop.load(Ordering::Relaxed));
    }

    #[test]
    fn turning_the_internet_off_ends_a_download() {
        let d = folder("off");
        let job = Job::default();
        job.start(
            model(),
            d.clone(),
            || true,
            || {
                let link = Writes {
                    polls: 1,
                    ..Writes::new(b"ab", Ok(()))
                };
                Ok((
                    Box::new(link) as Box<dyn Link>,
                    Memory::in_memory().map_err(|e| e.to_string())?,
                ))
            },
        )
        .unwrap();
        for _ in 0..200 {
            if job.json()["state"] == "stopped" {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(job.json()["state"], "stopped");
        assert!(d.join("models/tiny.gguf.part").exists());
    }

    #[test]
    fn the_job_runs_on_its_own_thread_and_one_at_a_time() {
        let d = folder("job");
        let job = Job::default();
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let waits = gate.clone();
        job.start(
            model(),
            d.clone(),
            || false,
            move || {
                drop(waits.lock());
                Ok((
                    Box::new(Writes::new(b"abc", Ok(()))) as Box<dyn Link>,
                    Memory::in_memory().map_err(|e| e.to_string())?,
                ))
            },
        )
        .unwrap();
        assert_eq!(job.json()["state"], "getting");
        let again = job.start(model(), d.clone(), || false, || Err("never".into()));
        assert!(again.unwrap_err().contains("already"));
        drop(held);
        for _ in 0..200 {
            if job.json()["state"] != "getting" && job.json()["state"] != "checking" {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(job.json()["state"], "done");
        assert_eq!(job.json()["model"], "tiny-writer");
        assert_eq!(std::fs::read(d.join("models/tiny.gguf")).unwrap(), b"abc");
    }

    #[test]
    fn a_job_that_cannot_open_says_why_and_can_start_again() {
        let d = folder("job_fail");
        let job = Job::default();
        job.start(model(), d.clone(), || false, || Err("no way out".into()))
            .unwrap();
        for _ in 0..200 {
            if job.json()["state"] == "failed" {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(job.json()["why"], "no way out");
        let started = job.start(model(), d, || false, || Err("still no way out".into()));
        assert!(started.is_ok());
    }
}
