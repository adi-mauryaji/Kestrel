//! `kestrel model import FILE.gguf` (v4 A3, blueprint §4.3): a catalog model from a file (a USB
//! stick, a download made elsewhere) copied into Kestrel's data folder, where `slot::model_path`
//! finds it first, and kept only when its SHA-256 is the catalog's. The page imports the same way
//! (`Job`, `POST /api/model/import`), on its own thread so the service keeps answering.

use std::error::Error;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kestrel_capability::Catalog;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, PartialEq)]
pub struct Imported {
    pub id: String,
    pub path: PathBuf,
    pub bytes: u64,
}

/// Copies `src` into `data`/models under its catalog name, hashing as it copies; `progress` hears
/// (bytes done, bytes in all). A file whose SHA-256 is no catalog model's is refused and nothing of
/// it is left.
pub fn import(
    src: &Path,
    data: &Path,
    catalog: &Catalog,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Imported, Box<dyn Error>> {
    let mut from =
        std::fs::File::open(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    let bytes = from.metadata()?.len();
    let dir = data.join("models");
    std::fs::create_dir_all(&dir)?;
    // Copied and hashed in one read; named once its hash says which model it is.
    let part = dir.join(".import.part");
    let copied = (|| -> std::io::Result<String> {
        let mut to = std::fs::File::create(&part)?;
        let (mut hash, mut buf, mut done) = (Sha256::new(), vec![0; 1 << 20], 0);
        loop {
            let n = from.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            to.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, bytes);
        }
        to.sync_all()?;
        Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
    })();
    let known = copied.as_ref().ok().and_then(|sha| {
        (catalog.models.iter()).find(|m| m.sha256.as_deref() == Some(sha.as_str()))
    });
    let Some(m) = known else {
        let _ = std::fs::remove_file(&part);
        copied?;
        return Err(format!(
            "{} is not one of Kestrel's models: its SHA-256 is none in the catalog \
             (`kestrel model use FILE.gguf` writes with any GGUF file where it is)",
            src.display()
        )
        .into());
    };
    let path = dir.join(&m.file);
    std::fs::rename(&part, &path)?;
    Ok(Imported {
        id: m.id.clone(),
        path,
        bytes,
    })
}

/// The model file typed on the page, as a full path: `~` is the home folder, quotes around a pasted
/// path are dropped. Only a .gguf file (Kestrel's models), so no other big file is copied for
/// nothing.
// The service's page (a llama build) uses this and `Job`; the tests use them everywhere.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
pub fn typed_file(text: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let text = text.trim();
    let text = ['"', '\'']
        .iter()
        .find_map(|q| text.strip_prefix(*q).and_then(|t| t.strip_suffix(*q)))
        .unwrap_or(text);
    let file = match text.strip_prefix('~') {
        // "~/…"; "~bob/…" (someone else's home) stays relative and is refused below.
        Some(rest) if rest.starts_with(std::path::is_separator) => {
            let home = home.ok_or("no home folder here: type the full path")?;
            home.join(rest.trim_start_matches(std::path::is_separator))
        }
        _ => PathBuf::from(text),
    };
    if !file.is_absolute() {
        return Err("the model file's full path, e.g. ~/Downloads/gemma-4-E2B-it-Q4_0.gguf".into());
    }
    let gguf = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
    if !gguf {
        return Err("Kestrel's models are .gguf files".into());
    }
    Ok(file)
}

/// Where the page's import is: none yet, copying (bytes done of all), done, or failed and why.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
enum State {
    Idle,
    Copying { done: u64, all: u64 },
    Done { id: String, bytes: u64 },
    Failed(String),
}

/// The page's import (one at a time), run on its own thread; the page asks how far it is.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
#[derive(Clone)]
pub struct Job(Arc<Mutex<State>>);

impl Default for Job {
    fn default() -> Self {
        Job(Arc::new(Mutex::new(State::Idle)))
    }
}

#[cfg_attr(not(feature = "llama"), allow(dead_code))]
impl Job {
    /// Starts importing `src` into `data`/models; refused while another import runs.
    pub fn start(&self, src: PathBuf, data: PathBuf, catalog: Catalog) -> Result<(), String> {
        let mut now = self.0.lock().map_err(|_| "the import's state was lost")?;
        if matches!(*now, State::Copying { .. }) {
            return Err("a model is being imported already".into());
        }
        *now = State::Copying { done: 0, all: 0 };
        let state = self.0.clone();
        let set = move |s: State| {
            if let Ok(mut now) = state.lock() {
                *now = s;
            }
        };
        std::thread::spawn(move || {
            let got = import(&src, &data, &catalog, &mut |done, all| {
                set(State::Copying { done, all })
            });
            set(match got {
                Ok(m) => State::Done {
                    id: m.id,
                    bytes: m.bytes,
                },
                Err(e) => State::Failed(e.to_string()),
            });
        });
        Ok(())
    }

    /// What `GET /api/model/import` answers.
    pub fn json(&self) -> Value {
        let now = self.0.lock().map(|s| s.clone());
        match now.unwrap_or(State::Failed("the import's state was lost".into())) {
            State::Idle => json!({"state": "idle"}),
            State::Copying { done, all } => json!({"state": "copying", "done": done, "all": all}),
            State::Done { id, bytes } => json!({"state": "done", "model": id, "bytes": bytes}),
            State::Failed(why) => json!({"state": "failed", "why": why}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of "abc" (FIPS 180-2, appendix B.1).
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn catalog() -> Catalog {
        serde_json::from_value(serde_json::json!({"models": [{
            "id": "tiny-writer", "file": "tiny-writer.gguf", "file_mib": 1, "memory_mib": 1,
            "role": "writer", "estimate": false, "sha256": ABC, "source": "test"
        }]}))
        .unwrap()
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_import_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn left_in(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|r| {
                r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_file_with_a_catalog_hash_is_copied_under_its_catalog_name() {
        let d = dir("ok");
        let src = d.join("downloaded copy.gguf");
        std::fs::write(&src, "abc").unwrap();
        let mut heard = Vec::new();
        let got = import(&src, &d.join("data"), &catalog(), &mut |done, all| {
            heard.push((done, all))
        })
        .unwrap();
        let path = d.join("data/models/tiny-writer.gguf");
        assert_eq!(
            got,
            Imported {
                id: "tiny-writer".into(),
                path: path.clone(),
                bytes: 3
            }
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "abc");
        assert!(src.is_file(), "the original stays");
        assert_eq!(heard.last(), Some(&(3, 3)));
        assert_eq!(left_in(&d.join("data/models")), ["tiny-writer.gguf"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn any_other_file_is_refused_and_nothing_of_it_is_left() {
        let d = dir("other");
        let src = d.join("tiny-writer.gguf");
        std::fs::write(&src, "abd").unwrap();
        let e = import(&src, &d.join("data"), &catalog(), &mut |_, _| {}).unwrap_err();
        assert!(e.to_string().contains("SHA-256"), "{e}");
        assert_eq!(left_in(&d.join("data/models")), Vec::<String>::new());
        let e = import(
            &d.join("nowhere.gguf"),
            &d.join("data"),
            &catalog(),
            &mut |_, _| {},
        );
        assert!(e.unwrap_err().to_string().contains("nowhere.gguf"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A full path on this OS (the tests run on Linux, macOS and Windows CI).
    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" })
    }

    #[test]
    fn the_page_names_a_gguf_file_by_its_full_path() {
        let home = root().join("home").join("a");
        let file = root().join("media").join("usb").join("gemma.gguf");
        assert_eq!(
            typed_file("~/Downloads/gemma.gguf", Some(&home)),
            Ok(home.join("Downloads").join("gemma.gguf"))
        );
        for pasted in [
            format!("  {}  ", file.display()),
            format!("'{}'", file.display()),
        ] {
            assert_eq!(typed_file(&pasted, None), Ok(file.clone()), "{pasted}");
        }
        let upper = root().join("m").join("GEMMA.GGUF");
        assert_eq!(
            typed_file(&format!("\"{}\"", upper.display()), None),
            Ok(upper)
        );
        let movie = root().join("m").join("movie.mkv");
        for bad in [
            "gemma.gguf".to_string(),
            String::new(),
            movie.display().to_string(),
        ] {
            assert!(typed_file(&bad, Some(&home)).is_err(), "{bad:?}");
        }
        assert!(
            typed_file("~/gemma.gguf", None).is_err(),
            "no home folder known"
        );
    }

    /// Waits until the job has ended (or 10 s).
    fn ended(job: &Job) -> Value {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while job.json()["state"] == "copying" && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        job.json()
    }

    #[test]
    fn the_page_import_runs_beside_the_service_and_says_how_it_ended() {
        let d = dir("job");
        let job = Job::default();
        assert_eq!(job.json(), json!({"state": "idle"}));
        std::fs::write(d.join("a.gguf"), "abc").unwrap();
        job.start(d.join("a.gguf"), d.join("data"), catalog())
            .unwrap();
        assert_eq!(
            ended(&job),
            json!({"state": "done", "model": "tiny-writer", "bytes": 3})
        );
        assert!(d.join("data/models/tiny-writer.gguf").is_file());
        std::fs::write(d.join("b.gguf"), "abd").unwrap();
        job.start(d.join("b.gguf"), d.join("data"), catalog())
            .unwrap();
        let failed = ended(&job);
        assert_eq!(failed["state"], "failed");
        assert!(
            failed["why"].as_str().unwrap().contains("SHA-256"),
            "{failed}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_import_at_a_time_and_its_progress_is_shown() {
        let d = dir("busy");
        let job = Job::default();
        *job.0.lock().unwrap() = State::Copying {
            done: 1 << 20,
            all: 4 << 20,
        };
        assert_eq!(
            job.json(),
            json!({"state": "copying", "done": 1 << 20, "all": 4 << 20})
        );
        let e = job.start(d.join("a.gguf"), d.join("data"), catalog());
        assert!(e.unwrap_err().contains("already"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
