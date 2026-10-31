//! `kestrel settings [notes DIR]` (v4 A3): what Kestrel keeps between starts, in settings.json in
//! its data folder. The app's onboarding and Settings write it the same way. Now the notes folder,
//! which `kestrel serve` uses when it is given none.

use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const USAGE: &str = "usage: kestrel settings              what Kestrel keeps between starts\n       kestrel settings notes DIR     the notes folder `kestrel serve` uses when given none";

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<PathBuf>,
}

/// settings.json in Kestrel's data folder.
pub fn path(data: &Path) -> PathBuf {
    data.join("settings.json")
}

/// The settings at `path`; none saved yet is the defaults. A file that is not settings is an error,
/// so it is never written over.
pub fn load(path: &Path) -> Result<Settings, Box<dyn Error>> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not Kestrel's settings ({e})", path.display()).into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), Box<dyn Error>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written whole beside it first, so a stop mid-write never leaves half a file.
    let new = path.with_extension("json.new");
    std::fs::write(&new, serde_json::to_string_pretty(settings)? + "\n")?;
    std::fs::rename(&new, path)?;
    Ok(())
}

/// The notes folder saved in Kestrel's data folder `data`, if any.
pub fn notes(data: Option<&Path>) -> Option<PathBuf> {
    load(&path(data?)).ok()?.notes
}

/// `kestrel settings [notes DIR]` with Kestrel's data folder `data`.
pub fn run(
    args: &[String],
    data: Option<&Path>,
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let data = || data.ok_or("no data folder for Kestrel's settings (HOME is not set)");
    match args {
        [] => {
            let settings = load(&path(data()?))?;
            writeln!(out, "{}", serde_json::to_string_pretty(&settings)?)?;
        }
        [key, dir] if key == "notes" => {
            let dir = Path::new(dir)
                .canonicalize()
                .ok()
                .filter(|d| d.is_dir())
                .ok_or_else(|| format!("no folder at {dir}"))?;
            let at = path(data()?);
            let mut settings = load(&at)?;
            settings.notes = Some(dir.clone());
            save(&at, &settings)?;
            writeln!(out, "notes folder: {}", dir.display())?;
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kestrel-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nothing_saved_is_the_defaults_and_a_save_reads_back() {
        let d = dir("roundtrip");
        let p = path(&d.join("data"));
        assert_eq!(load(&p).unwrap(), Settings::default());
        let s = Settings {
            notes: Some("/home/me/notes".into()),
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p).unwrap(), s);
        assert_eq!(notes(Some(&d.join("data"))), Some("/home/me/notes".into()));
        assert_eq!(notes(None), None);
    }

    #[test]
    fn a_file_that_is_not_settings_is_an_error_and_kept() {
        let d = dir("bad");
        let p = path(&d);
        std::fs::write(&p, "{not json").unwrap();
        assert!(load(&p).is_err());
        let mut out = Vec::new();
        assert!(run(&args(&["notes", d.to_str().unwrap()]), Some(&d), &mut out).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{not json");
    }

    #[test]
    fn the_notes_folder_is_saved_as_a_full_path_and_must_exist() {
        let d = dir("notes");
        let notes_dir = d.join("my notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        let data = d.join("data");
        let mut out = Vec::new();
        let missing = d.join("nowhere");
        assert!(
            run(
                &args(&["notes", missing.to_str().unwrap()]),
                Some(&data),
                &mut out
            )
            .is_err()
        );
        assert!(!path(&data).exists());
        run(
            &args(&["notes", notes_dir.to_str().unwrap()]),
            Some(&data),
            &mut out,
        )
        .unwrap();
        let saved = notes(Some(&data)).unwrap();
        assert!(saved.is_absolute());
        assert_eq!(saved, notes_dir.canonicalize().unwrap());
        let mut shown = Vec::new();
        run(&[], Some(&data), &mut shown).unwrap();
        let shown: serde_json::Value = serde_json::from_slice(&shown).unwrap();
        assert_eq!(shown["notes"], saved.to_str().unwrap());
    }

    #[test]
    fn anything_else_is_the_usage_and_no_data_folder_is_said() {
        let mut out = Vec::new();
        let e = run(&args(&["colour", "blue"]), Some(Path::new("/x")), &mut out).unwrap_err();
        assert_eq!(e.to_string(), USAGE);
        let e = run(&[], None, &mut out).unwrap_err();
        assert!(e.to_string().contains("no data folder"), "{e}");
    }
}
