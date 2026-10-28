//! `kestrel memory` (Week 10, Ch. 17.3, 19.4): see, approve, delete and export what Kestrel
//! remembers, without loading a model.
//!
//!     kestrel memory [--db PATH] list | pending | tasks
//!     kestrel memory [--db PATH] approve ID | delete ID
//!     kestrel memory [--db PATH] export | purge | vacuum
//!
//! The memory file is `--db PATH`, else $KESTREL_MEMORY, else memory.db in Kestrel's data folder
//! (kestrel-platform: $XDG_DATA_HOME/kestrel or ~/.local/share/kestrel on Linux, ~/Library/Application
//! Support/kestrel on macOS, %LOCALAPPDATA%\kestrel on Windows). On Unix its directory is created
//! 0700 and the file is 0600.

use std::error::Error;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kestrel_memory::{Filter, Memory, context};
use kestrel_platform::Os;

pub const USAGE: &str = "usage: kestrel memory [--db PATH] list | pending | tasks | approve ID | delete ID | export | purge | vacuum";

pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (path, rest) = match args.as_slice() {
        ["--db", path, rest @ ..] => (PathBuf::from(*path), rest),
        rest => (default_db(Os::current(), &kestrel_platform::env)?, rest),
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    command(&mut open(&path)?, rest, i64::try_from(now)?, out)
}

/// $KESTREL_MEMORY, else memory.db in `os`'s data folder (empty variables count as unset).
pub(crate) fn default_db(
    os: Os,
    var: &dyn Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, Box<dyn Error>> {
    if let Some(p) = var("KESTREL_MEMORY").filter(|v| !v.is_empty()) {
        return Ok(p.into());
    }
    let data = kestrel_platform::data_dir(os, var)
        .ok_or("no memory file: set KESTREL_MEMORY, or pass --db PATH")?;
    Ok(data.join("memory.db"))
}

/// Opens the memory file, creating a missing directory 0700 (an existing one is left as it is).
pub(crate) fn open(path: &Path) -> Result<Memory, Box<dyn Error>> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && !dir.exists()
    {
        private_dir(dir)?;
    }
    Ok(Memory::open(path)?)
}

#[cfg(unix)]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

fn command(
    m: &mut Memory,
    args: &[&str],
    now: i64,
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    match args {
        ["list"] => list(m, Filter::All, now, out),
        ["pending"] => list(m, Filter::Pending, now, out),
        ["tasks"] => {
            for t in m.tasks()? {
                let due = t.due.map(|d| format!(" (due {d})")).unwrap_or_default();
                writeln!(out, "#{} {}{due}", t.id, t.title)?;
            }
            Ok(())
        }
        ["approve", id] => {
            let id = parse_id(id)?;
            m.approve(id, now)?;
            Ok(writeln!(out, "approved #{id}")?)
        }
        ["delete", id] => {
            let id = parse_id(id)?;
            if !m.delete(id, now)? {
                return Err(format!("no memory #{id}").into());
            }
            Ok(writeln!(out, "deleted #{id}")?)
        }
        ["export"] => Ok(writeln!(out, "{}", m.export_json()?)?),
        ["purge"] => Ok(writeln!(out, "purged {} expired", m.purge_expired(now)?)?),
        ["vacuum"] => {
            m.vacuum()?;
            Ok(writeln!(out, "vacuumed")?)
        }
        _ => Err(USAGE.into()),
    }
}

/// One line per item: id, provenance line, then whether it waits for approval or expires.
fn list(m: &Memory, filter: Filter, now: i64, out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    for it in m.items(filter)? {
        let mut s = format!("#{} {}", it.id, context::line(&it));
        if !it.approved {
            s.push_str(" (pending approval)");
        }
        match it.expires_at {
            Some(at) if at <= now => s.push_str(" (expired)"),
            Some(at) => s.push_str(&format!(" (expires {})", context::utc_date(at))),
            None => {}
        }
        writeln!(out, "{s}")?;
    }
    Ok(())
}

fn parse_id(s: &str) -> Result<i64, Box<dyn Error>> {
    s.parse()
        .map_err(|_| format!("not a memory id: {s:?}\n{USAGE}").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_memory::{Draft, Kind, Source};

    const NOW: i64 = 1_792_713_600; // 2026-10-23T00:00:00Z
    const DAY: i64 = 86_400;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kestrel_cli_memory_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn seeded() -> Memory {
        let mut m = Memory::in_memory().unwrap();
        let pref = Draft::new(Kind::Preference, "answer_length", "short");
        m.propose(&pref, Source::UserExplicit, NOW).unwrap();
        let guess = Draft::new(Kind::Fact, "advisor", "Dr. Rao").expires_at(NOW + 30 * DAY);
        m.propose(&guess, Source::ModelInferred, NOW).unwrap();
        m.create_task("Call Asha", Some("2026-10-24"), NOW).unwrap();
        m
    }

    fn say(m: &mut Memory, args: &[&str]) -> Result<String, String> {
        let mut out = Vec::new();
        command(m, args, NOW, &mut out).map_err(|e| e.to_string())?;
        Ok(String::from_utf8(out).unwrap())
    }

    #[test]
    fn list_shows_ids_provenance_pending_and_expiry() {
        let mut m = seeded();
        assert_eq!(
            say(&mut m, &["list"]).unwrap(),
            "#1 [pref|user_explicit|2026-10-23] answer_length: short\n\
             #2 [fact|model_inferred|2026-10-23] advisor: Dr. Rao (pending approval) (expires 2026-11-22)\n"
        );
        assert_eq!(
            say(&mut m, &["pending"]).unwrap(),
            "#2 [fact|model_inferred|2026-10-23] advisor: Dr. Rao (pending approval) (expires 2026-11-22)\n"
        );
        assert_eq!(
            say(&mut m, &["tasks"]).unwrap(),
            "#1 Call Asha (due 2026-10-24)\n"
        );
    }

    #[test]
    fn approve_delete_purge_and_export() {
        let mut m = seeded();
        assert_eq!(say(&mut m, &["approve", "2"]).unwrap(), "approved #2\n");
        assert_eq!(say(&mut m, &["pending"]).unwrap(), "");
        assert!(say(&mut m, &["approve", "2"]).is_err(), "nothing pending");
        assert_eq!(say(&mut m, &["delete", "1"]).unwrap(), "deleted #1\n");
        assert!(
            say(&mut m, &["delete", "1"])
                .unwrap_err()
                .contains("no memory #1")
        );
        let export: serde_json::Value =
            serde_json::from_str(&say(&mut m, &["export"]).unwrap()).unwrap();
        assert_eq!(export.as_array().map(Vec::len), Some(1));
        assert_eq!(export[0]["key"], "advisor");
        assert_eq!(say(&mut m, &["purge"]).unwrap(), "purged 0 expired\n");
        assert_eq!(say(&mut m, &["vacuum"]).unwrap(), "vacuumed\n");
    }

    #[test]
    fn bad_commands_are_usage_errors() {
        let mut m = seeded();
        for args in [
            &[][..],
            &["drop"],
            &["delete"],
            &["delete", "x"],
            &["list", "extra"],
        ] {
            assert!(say(&mut m, args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn the_default_file_follows_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        let path = |pairs| default_db(Os::Linux, &env(pairs)).unwrap();
        assert_eq!(
            path(&[("KESTREL_MEMORY", "/x/m.db"), ("HOME", "/home/u")]),
            PathBuf::from("/x/m.db")
        );
        assert_eq!(
            path(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/u")]),
            PathBuf::from("/data/kestrel/memory.db")
        );
        assert_eq!(
            path(&[("HOME", "/home/u")]),
            PathBuf::from("/home/u/.local/share/kestrel/memory.db")
        );
        assert!(default_db(Os::Linux, &env(&[])).is_err());
        // The other systems' folders (kestrel-platform's tests hold the rest).
        assert_eq!(
            default_db(
                Os::Windows,
                &env(&[("LOCALAPPDATA", "C:/L"), ("HOME", "/home/u")])
            )
            .unwrap(),
            PathBuf::from("C:/L").join("kestrel").join("memory.db")
        );
        assert_eq!(
            default_db(Os::MacOs, &env(&[("KESTREL_MEMORY", "/x/m.db")])).unwrap(),
            PathBuf::from("/x/m.db")
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_directory_and_the_file_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("private");
        let path = d.join("kestrel").join("memory.db");
        open(&path).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn run_takes_the_db_flag() {
        let d = dir("flag");
        let db = d.join("m.db");
        let args: Vec<String> = ["--db", db.to_str().unwrap(), "list"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut out = Vec::new();
        run(&args, &mut out).unwrap();
        assert!(out.is_empty(), "a new memory is empty");
        assert!(db.exists());
    }
}
