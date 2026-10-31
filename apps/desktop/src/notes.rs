//! The notes folder on a first start (v4 A3, blueprint §4.10 onboarding): the folder typed on the
//! shell's own page, and the one `kestrel settings` has saved. The CLI owns settings.json; the shell
//! only reads its answer and asks it to save.

use std::path::{Path, PathBuf};

/// The folder typed on the shell's page, as a full path (`~` is the home folder). A relative path is
/// refused: it would depend on the folder the app happened to start in.
pub fn typed(text: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let text = text.trim();
    let dir = match text.strip_prefix('~') {
        // "~" or "~/…"; "~bob/…" (someone else's home) stays relative and is refused below.
        Some(rest) if rest.is_empty() || rest.starts_with(std::path::is_separator) => {
            let home = home.ok_or("No home folder here: type the full path")?;
            home.join(rest.trim_start_matches(std::path::is_separator))
        }
        _ => PathBuf::from(text),
    };
    if text.is_empty() || !dir.is_absolute() {
        return Err("Type the full path, e.g. ~/Documents/Notes".into());
    }
    Ok(dir)
}

/// The notes folder in the answer of `kestrel settings` (its settings.json), if one is saved.
pub fn saved(settings: &str) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_str(settings).ok()?;
    v["notes"].as_str().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full path on this OS (the tests run on Linux, macOS and Windows CI).
    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" })
    }

    #[test]
    fn a_full_path_is_kept_without_the_spaces_around_it() {
        let dir = root().join("srv").join("notes");
        let text = format!("  {}  ", dir.display());
        assert_eq!(typed(&text, None), Ok(dir));
    }

    #[test]
    fn the_tilde_is_the_home_folder() {
        let home = root().join("home").join("a");
        assert_eq!(
            typed("~/Documents/Notes", Some(&home)),
            Ok(home.join("Documents").join("Notes"))
        );
        assert_eq!(typed("~", Some(&home)), Ok(home.clone()));
        assert!(typed("~/notes", None).is_err(), "no home folder known");
    }

    #[test]
    fn relative_and_empty_paths_are_refused() {
        let home = root().join("home").join("a");
        for text in ["notes", "./notes", "~bob/notes", "", "   "] {
            assert!(typed(text, Some(&home)).is_err(), "{text:?}");
        }
    }

    #[test]
    fn the_saved_folder_is_read_from_the_settings() {
        let answer = "{\n  \"notes\": \"/home/a/notes\",\n  \"name\": \"Aditya\"\n}\n";
        assert_eq!(saved(answer), Some(PathBuf::from("/home/a/notes")));
        assert_eq!(saved("{}\n"), None);
        assert_eq!(saved("{\"name\": \"Aditya\"}"), None);
        assert_eq!(saved("kestrel: not settings"), None);
    }
}
