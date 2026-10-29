//! `kestrel model [use ID|FILE.gguf | auto]` (v4 A2, the model slot; blueprint §4.3): which writer
//! this device uses. Automatic = kestrel-capability's choice (the largest model here that fits and
//! passes the writing gates, measured or estimated from a measured one); or the user's: a catalog
//! model, or any GGUF file (its sizes then estimated from the file). Kept in settings.json in
//! Kestrel's data folder. The service keeps writing with its config's model until `kestrel
//! calibrate` has measured the new one on this device (capabilities::service_writer).

use std::error::Error;
use std::io::Write;
use std::path::Path;

use kestrel_capability::{Catalog, Choice, Device, Model};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Settings {
    #[serde(default)]
    pub writer: Writer,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Writer {
    #[default]
    Auto,
    Catalog {
        id: String,
    },
    File {
        path: String,
    },
}

/// settings.json in `data`; none or unreadable: the defaults.
pub fn load(data: Option<&Path>) -> Settings {
    data.and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(data: &Path, s: &Settings) -> std::io::Result<()> {
    std::fs::create_dir_all(data)?;
    let text = serde_json::to_string_pretty(s).map_err(std::io::Error::other)?;
    std::fs::write(data.join("settings.json"), text + "\n")
}

/// A GGUF file the user names, as a catalog writer: its id the file's stem, its memory estimated
/// from the file the way the catalog's ladder is (1.44 × the file).
pub fn file_model(path: &Path, bytes: u64) -> Model {
    let file_mib = bytes >> 20;
    Model {
        id: path
            .file_stem()
            .map_or_else(|| "your-model".into(), |s| s.to_string_lossy().into_owned()),
        file: path.to_string_lossy().into_owned(),
        file_mib,
        memory_mib: ((file_mib as f64 * 1.44) / 100.0).round() as u64 * 100,
        active_mib: None,
        role: kestrel_capability::Role::Writer,
        estimate: true,
        sha256: None,
        tau2: None,
        source: "your file; memory estimated as 1.44 x the file".into(),
    }
}

/// The writer this device uses and how it was chosen.
pub fn effective(s: &Settings, choice: &Choice) -> Option<(String, &'static str)> {
    match &s.writer {
        Writer::Auto => choice.best.clone().map(|b| (b, "automatic")),
        Writer::Catalog { id } => Some((id.clone(), "your choice")),
        Writer::File { path } => Some((file_model(Path::new(path), 0).id, "your file")),
    }
}

/// What `kestrel model` shows: the writer, then every writer model and how it would do here.
pub fn render(
    d: &Device,
    choice: &Choice,
    catalog: &Catalog,
    writer: Option<(&str, &str)>,
    calibrated: &[kestrel_capability::Calibrated],
) -> String {
    use kestrel_capability::size;
    let budget = size(kestrel_capability::memory_budget_mib(d));
    let mut s = match writer {
        Some((id, how)) => format!("Writer: {id} ({how}). A model may use {budget} here.\n"),
        None => format!("Writer: none chosen yet. A model may use {budget} here.\n"),
    };
    let width = choice.fits.iter().map(|f| f.model.len()).max().unwrap_or(0);
    for f in &choice.fits {
        let Some(m) = catalog.models.iter().find(|m| m.id == f.model) else {
            continue;
        };
        let speed = match (f.speed, f.estimated) {
            (Some(sp), false) => format!("{:.1} tokens/s, measured", sp.decode_tok_s),
            (Some(sp), true) => format!("about {:.1} tokens/s", sp.decode_tok_s),
            (None, _) => "speed not measured".into(),
        };
        let status = if !f.fits {
            format!("needs {} of memory: too big here", size(m.memory_mib))
        } else if f.passes == Some(false) {
            format!("{speed}: too slow here")
        } else if f.here {
            format!("{speed}: here")
        } else if f.passes == Some(true) {
            format!("{speed}: could run here (not downloaded)")
        } else {
            format!("{speed}: not downloaded")
        };
        let mark = if writer.is_some_and(|(id, _)| id == f.model) {
            "*"
        } else {
            " "
        };
        s.push_str(&format!(
            "{mark} {:<width$}  {:>7}  {status}\n",
            f.model,
            size(m.file_mib)
        ));
    }
    // The models here, and what `kestrel calibrate` found with each.
    let here: Vec<&str> = (choice.fits.iter())
        .filter(|f| f.here)
        .map(|f| f.model.as_str())
        .collect();
    if !here.is_empty() {
        s.push_str("Checked on this device (`kestrel calibrate`):\n");
        let width = here.iter().map(|m| m.len()).max().unwrap_or(0);
        for id in here {
            let found = match calibrated.iter().find(|c| c.model == id) {
                Some(c) => crate::pass_rates::checked(c, false),
                None => "not yet".into(),
            };
            s.push_str(&format!("  {id:<width$}  {found}\n"));
        }
    }
    s
}

pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let data = kestrel_platform::data_dir(kestrel_platform::Os::current(), &kestrel_platform::env);
    run_in(Path::new("."), data.as_deref(), args, out)
}

pub fn run_in(
    root: &Path,
    data: Option<&Path>,
    args: &[String],
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    const USE: &str = "usage: kestrel model [use ID|FILE.gguf | auto]";
    let mut settings = load(data);
    let now = crate::capabilities::context(root, data, None)?;
    let budget = kestrel_capability::memory_budget_mib(&now.device);
    let too_big = |m: &Model| -> Box<dyn Error> {
        format!(
            "{} is too big for this device: it needs about {} of memory, this device can spare {}",
            m.id,
            kestrel_capability::size(m.memory_mib),
            kestrel_capability::size(budget)
        )
        .into()
    };
    let changed = match args {
        [] => false,
        [a] if a == "auto" => {
            settings.writer = Writer::Auto;
            true
        }
        [u, what] if u == "use" && (what.ends_with(".gguf") || what.contains(['/', '\\'])) => {
            let path = std::path::absolute(what)?;
            let bytes = std::fs::metadata(&path)
                .map_err(|_| format!("no such file: {what}"))?
                .len();
            let m = file_model(&path, bytes);
            if m.memory_mib > budget {
                return Err(too_big(&m));
            }
            settings.writer = Writer::File {
                path: path.to_string_lossy().into_owned(),
            };
            true
        }
        [u, id] if u == "use" => {
            let m = now
                .catalog
                .models
                .iter()
                .find(|m| &m.id == id && m.role == kestrel_capability::Role::Writer)
                .ok_or_else(|| {
                    format!("{id} is not in the catalog's writers (`kestrel model` lists them)")
                })?;
            if m.memory_mib > budget {
                return Err(too_big(m));
            }
            if !root.join("models").join(&m.file).is_file() {
                return Err(format!(
                    "{id} is not downloaded: put {} in models/ (downloads come with the app, \
                     v4 phase A3)",
                    m.file
                )
                .into());
            }
            settings.writer = Writer::Catalog { id: id.clone() };
            true
        }
        _ => return Err(USE.into()),
    };
    if changed {
        save(
            data.ok_or("no data folder to keep the choice in")?,
            &settings,
        )?;
    }
    let now = crate::capabilities::context(root, data, None)?;
    let writer = now.device.writer.as_deref().zip(now.how);
    out.write_all(
        render(
            &now.device,
            &now.choice,
            &now.catalog,
            writer,
            &now.calibrated,
        )
        .as_bytes(),
    )?;
    if let Some(note) = crate::capabilities::service_note(&now) {
        writeln!(out, "{note}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_capability::{Fit, Role, Speed};
    use kestrel_platform::Os;

    fn repo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_slot_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn settings_default_to_automatic_and_keep_the_users_choice() {
        let d = temp("settings");
        assert_eq!(load(Some(&d)), Settings::default());
        assert_eq!(load(None).writer, Writer::Auto);
        for w in [
            Writer::Catalog {
                id: "gemma-4-E4B-it-Q4_0".into(),
            },
            Writer::File {
                path: "/m/mistral.gguf".into(),
            },
            Writer::Auto,
        ] {
            save(&d, &Settings { writer: w.clone() }).unwrap();
            assert_eq!(load(Some(&d)).writer, w);
        }
        std::fs::write(d.join("settings.json"), "not json").unwrap();
        assert_eq!(
            load(Some(&d)),
            Settings::default(),
            "a broken file: the defaults"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_users_gguf_becomes_an_estimated_writer() {
        let m = file_model(Path::new("/m/Mistral-7B-Q4.gguf"), 4_368 << 20);
        assert_eq!(m.id, "Mistral-7B-Q4");
        assert_eq!(m.file, "/m/Mistral-7B-Q4.gguf");
        assert_eq!((m.file_mib, m.memory_mib), (4_368, 6_300));
        assert_eq!(m.role, Role::Writer);
        assert!(m.estimate && m.sha256.is_none());
    }

    fn choice(best: Option<&str>) -> Choice {
        let f = |model: &str, fits: bool, here: bool, decode: Option<f64>, estimated: bool| Fit {
            model: model.into(),
            fits,
            here,
            speed: decode.map(|d| Speed {
                decode_tok_s: d,
                prefill_tok_s: d * 7.0,
            }),
            estimated,
            passes: decode.map(|d| d >= 7.0),
        };
        Choice {
            fits: vec![
                f("26b", false, false, Some(16.8), true),
                f("12b", true, false, Some(5.3), true),
                f("e4b", true, false, Some(8.0), true),
                f("e2b", true, true, Some(12.5), false),
            ],
            best: best.map(String::from),
            could_run: Some("e4b".into()),
        }
    }

    fn catalog() -> Catalog {
        serde_json::from_str(
            r#"{"models": [
              {"id": "e2b", "file": "e2b.gguf", "file_mib": 2710, "memory_mib": 3900, "role": "writer", "source": "t"},
              {"id": "e4b", "file": "e4b.gguf", "file_mib": 4250, "memory_mib": 6100, "role": "writer", "source": "t"},
              {"id": "12b", "file": "12b.gguf", "file_mib": 6350, "memory_mib": 9100, "role": "writer", "source": "t"},
              {"id": "26b", "file": "26b.gguf", "file_mib": 13390, "memory_mib": 19300, "role": "writer", "source": "t"}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn the_writer_is_the_users_or_else_the_automatic_choice() {
        let c = choice(Some("e2b"));
        let auto = Settings::default();
        assert_eq!(effective(&auto, &c), Some(("e2b".into(), "automatic")));
        let named = Settings {
            writer: Writer::Catalog { id: "e4b".into() },
        };
        assert_eq!(effective(&named, &c), Some(("e4b".into(), "your choice")));
        let file = Settings {
            writer: Writer::File {
                path: "/m/Mistral-7B-Q4.gguf".into(),
            },
        };
        assert_eq!(
            effective(&file, &c),
            Some(("Mistral-7B-Q4".into(), "your file"))
        );
        assert_eq!(effective(&auto, &choice(None)), None);
    }

    #[test]
    fn the_list_says_how_each_model_would_do_here() {
        let d = Device {
            os: Os::Linux,
            ram_total_mib: 15_308,
            gpus: vec![],
            tools: vec![],
            models: vec!["e2b".into()],
            writer: Some("e2b".into()),
            internet: false,
            setup_done: vec![],
        };
        assert_eq!(
            render(
                &d,
                &choice(Some("e2b")),
                &catalog(),
                Some(("e2b", "automatic")),
                &[]
            ),
            "Writer: e2b (automatic). A model may use 11.9 GB here.\n\
             \x20 26b  13.1 GB  needs 18.8 GB of memory: too big here\n\
             \x20 12b   6.2 GB  about 5.3 tokens/s: too slow here\n\
             \x20 e4b   4.2 GB  about 8.0 tokens/s: could run here (not downloaded)\n\
             * e2b   2.6 GB  12.5 tokens/s, measured: here\n\
             Checked on this device (`kestrel calibrate`):\n\
             \x20 e2b  not yet\n"
        );
        let mut k = calibrated_e2b();
        k.notes_qa = Some(kestrel_capability::NotesQa {
            answerable: kestrel_capability::Tally {
                n: 20,
                right: 20,
                wrong: 0,
            },
            unanswerable: kestrel_capability::Tally {
                n: 6,
                right: 4,
                wrong: 2,
            },
        });
        k.code_tasks = Some(kestrel_capability::CodeTasks {
            n: 18,
            pass1: 17,
            pass_loop: 17,
        });
        assert!(
            render(
                &d,
                &choice(Some("e2b")),
                &catalog(),
                Some(("e2b", "automatic")),
                &[k]
            )
            .ends_with(
                "Checked on this device (`kestrel calibrate`):\n\
                            \x20 e2b  notes questions 20 of 20 right, 2 of 6 answered that \
                            the notes do not answer; code 17 of 18 pass\n"
            )
        );
    }

    #[test]
    fn use_and_auto_keep_a_choice_the_device_can_take() {
        let data = temp("use");
        let root = repo();
        let mut out = Vec::new();
        let e = run_in(
            &root,
            Some(&data),
            &["use".into(), "llama-9000".into()],
            &mut out,
        )
        .unwrap_err();
        assert!(e.to_string().contains("not in the catalog"), "{e}");
        let e = run_in(
            &root,
            Some(&data),
            &["use".into(), "gemma-4-31B-it-Q4_0".into()],
            &mut out,
        )
        .unwrap_err();
        assert!(e.to_string().contains("too big"), "{e}");
        let e = run_in(
            &root,
            Some(&data),
            &["use".into(), "/no/such.gguf".into()],
            &mut out,
        )
        .unwrap_err();
        assert!(e.to_string().contains("no such file"), "{e}");
        run_in(&root, Some(&data), &["auto".into()], &mut out).unwrap();
        assert_eq!(load(Some(&data)).writer, Writer::Auto);
        if root.join("models/gemma-4-E2B-it-Q4_0.gguf").is_file() {
            run_in(
                &root,
                Some(&data),
                &["use".into(), "gemma-4-E2B-it-Q4_0".into()],
                &mut out,
            )
            .unwrap();
            assert_eq!(
                load(Some(&data)).writer,
                Writer::Catalog {
                    id: "gemma-4-E2B-it-Q4_0".into()
                }
            );
        }
        assert!(run_in(&root, Some(&data), &["nope".into()], &mut out).is_err());
        let _ = std::fs::remove_dir_all(&data);
    }

    fn calibrated_e2b() -> kestrel_capability::Calibrated {
        kestrel_capability::Calibrated {
            model: "e2b".into(),
            file: (1, 1),
            binary: (1, 1),
            temperature: 1.74,
            tau: 0.8,
            tau_g: 0.7,
            val_n: 152,
            val_wrong: 6,
            val_asked: 4,
            measured_at: 1,
            notes_qa: None,
            code_tasks: None,
        }
    }
}
