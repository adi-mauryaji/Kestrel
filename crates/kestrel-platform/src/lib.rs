//! What differs between Linux, macOS and Windows (ADR-052, v4 blueprint §4.1), and nothing else.
//!
//! Standard library only. Every function takes the OS and an environment lookup, so the rules for
//! all three systems are tested on any one of them; `Os::current()` and `env` are the only links to
//! the machine. Linux keeps v3's paths and commands exactly.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

/// An environment lookup: `env` for the process, a closure in tests.
pub type Var<'a> = &'a dyn Fn(&str) -> Option<OsString>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    /// Linux and the other Unix systems: XDG folders.
    Linux,
    MacOs,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// The process environment.
pub fn env(k: &str) -> Option<OsString> {
    std::env::var_os(k)
}

/// `var` with empty values read as unset (v3's rule for every variable).
fn set(var: Var, k: &str) -> Option<OsString> {
    var(k).filter(|v| !v.is_empty())
}

/// Kestrel's data folder (the memory file; later indexes and models).
pub fn data_dir(os: Os, var: Var) -> Option<PathBuf> {
    let home = || set(var, "HOME").map(PathBuf::from);
    match os {
        Os::Linux => set(var, "XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
            .map(|d| d.join("kestrel")),
        Os::MacOs => home().map(|h| h.join("Library/Application Support/kestrel")),
        // Local, not Roaming: models are gigabytes and must not follow a roaming profile.
        Os::Windows => set(var, "LOCALAPPDATA").map(|d| PathBuf::from(d).join("kestrel")),
    }
}

/// Kestrel's cache folder (the model worker's saved prompt prefix).
pub fn cache_dir(os: Os, var: Var) -> Option<PathBuf> {
    let home = || set(var, "HOME").map(PathBuf::from);
    match os {
        Os::Linux => set(var, "XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".cache")))
            .map(|d| d.join("kestrel")),
        Os::MacOs => home().map(|h| h.join("Library/Caches/kestrel")),
        Os::Windows => data_dir(os, var).map(|d| d.join("cache")),
    }
}

/// The running service's socket on Unix. On Windows no socket is made: the path only names the
/// page's address files, which take its name with another extension (`.url`, `.lan.url`).
pub fn socket_path(os: Os, var: Var) -> PathBuf {
    let per_user_tmp = || {
        let user = var("USER")
            .and_then(|u| u.into_string().ok())
            .unwrap_or_else(|| "user".into());
        PathBuf::from(format!("/tmp/kestrel-{user}.sock"))
    };
    match os {
        // $XDG_RUNTIME_DIR is a per-user 0700 folder.
        Os::Linux => set(var, "XDG_RUNTIME_DIR")
            .map(|d| PathBuf::from(d).join("kestrel.sock"))
            .unwrap_or_else(per_user_tmp),
        // macOS gives each user a private $TMPDIR (and sockaddr_un holds 104 bytes there).
        Os::MacOs => set(var, "TMPDIR")
            .map(|d| PathBuf::from(d).join("kestrel.sock"))
            .unwrap_or_else(per_user_tmp),
        Os::Windows => data_dir(os, var)
            .or_else(|| set(var, "TEMP").map(PathBuf::from))
            .unwrap_or_else(std::env::temp_dir)
            .join("kestrel.sock"),
    }
}

/// Opens `url` in the default browser.
pub fn open_url(os: Os, url: &str) -> Command {
    let (program, before): (&str, &[&str]) = match os {
        Os::Linux => ("xdg-open", &[]),
        Os::MacOs => ("open", &[]),
        // Hands the URL over whole, its fragment (the page's key) included; `cmd /c start` would
        // read characters such as & itself.
        Os::Windows => ("rundll32", &["url.dll,FileProtocolHandler"]),
    };
    let mut c = Command::new(program);
    c.args(before).arg(url);
    c
}

/// A desktop notification; `None` where only the Kestrel app can show one (Windows, phase A3).
pub fn notify(os: Os, title: &str, body: &str) -> Option<Command> {
    match os {
        Os::Linux => {
            let mut c = Command::new("notify-send");
            c.args(["--app-name=Kestrel", "--icon=appointment-soon", title, body]);
            Some(c)
        }
        Os::MacOs => {
            let script = format!(
                "display notification {} with title {}",
                applescript_string(body),
                applescript_string(title)
            );
            let mut c = Command::new("osascript");
            c.args(["-e", &script]);
            Some(c)
        }
        Os::Windows => None,
    }
}

/// `text` as an AppleScript string literal: the text never ends the literal early.
fn applescript_string(text: &str) -> String {
    let mut s = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '\n' => s.push_str("\\n"),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

/// Prints the system's UTC offset now, daylight saving included.
pub fn utc_offset_command(os: Os) -> Command {
    match os {
        Os::Linux | Os::MacOs => {
            let mut c = Command::new("date");
            c.arg("+%z");
            c
        }
        Os::Windows => {
            let mut c = Command::new("powershell");
            c.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[int][TimeZoneInfo]::Local.GetUtcOffset([DateTime]::Now).TotalMinutes",
            ]);
            c
        }
    }
}

/// `utc_offset_command`'s output as seconds east of UTC.
pub fn parse_utc_offset(os: Os, out: &str) -> Option<i64> {
    let out = out.trim_end();
    match os {
        // "+0530" -> 19 800 s, "-0800" -> -28 800 s.
        Os::Linux | Os::MacOs => {
            let (sign, digits) = match out.as_bytes().first()? {
                b'+' => (1, &out[1..]),
                b'-' => (-1, &out[1..]),
                _ => return None,
            };
            if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let (h, m): (i64, i64) = (digits[..2].parse().ok()?, digits[2..].parse().ok()?);
            (h <= 14 && m < 60).then_some(sign * (h * 3600 + m * 60))
        }
        // Whole minutes: "330" -> 19 800 s. Real offsets lie within ±14 h.
        Os::Windows => {
            // PowerShell writes "-480" or "330", never a "+": refuse the Unix form ("+0530"),
            // which i64::from_str would read as 530 minutes.
            let digits = out.strip_prefix('-').unwrap_or(out);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let minutes: i64 = out.parse().ok()?;
            (minutes.abs() <= 14 * 60).then_some(minutes * 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    /// An environment holding exactly `pairs`.
    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |k: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    /// A command as its program followed by its arguments.
    fn words(c: &Command) -> Vec<&OsStr> {
        std::iter::once(c.get_program())
            .chain(c.get_args())
            .collect()
    }

    #[test]
    fn this_machine_is_one_of_the_three() {
        let os = Os::current();
        assert_eq!(os == Os::Windows, cfg!(windows));
        assert_eq!(os == Os::MacOs, cfg!(target_os = "macos"));
    }

    #[test]
    fn linux_keeps_the_xdg_folders_of_v3() {
        let os = Os::Linux;
        let both = env_of(&[
            ("XDG_DATA_HOME", "/data"),
            ("XDG_CACHE_HOME", "/c"),
            ("HOME", "/home/u"),
        ]);
        assert_eq!(data_dir(os, &both), Some(PathBuf::from("/data/kestrel")));
        assert_eq!(cache_dir(os, &both), Some(PathBuf::from("/c/kestrel")));
        let home = env_of(&[("HOME", "/home/u")]);
        assert_eq!(
            data_dir(os, &home),
            Some(PathBuf::from("/home/u/.local/share/kestrel"))
        );
        assert_eq!(
            cache_dir(os, &home),
            Some(PathBuf::from("/home/u/.cache/kestrel"))
        );
        // An empty variable counts as unset (v3's rule).
        let empty = env_of(&[
            ("XDG_DATA_HOME", ""),
            ("XDG_CACHE_HOME", ""),
            ("HOME", "/home/u"),
        ]);
        assert_eq!(
            data_dir(os, &empty),
            Some(PathBuf::from("/home/u/.local/share/kestrel"))
        );
        assert_eq!(
            cache_dir(os, &empty),
            Some(PathBuf::from("/home/u/.cache/kestrel"))
        );
        assert_eq!(data_dir(os, &env_of(&[])), None);
        assert_eq!(cache_dir(os, &env_of(&[])), None);
    }

    #[test]
    fn macos_uses_the_library_folders_and_ignores_xdg() {
        let os = Os::MacOs;
        let e = env_of(&[("XDG_DATA_HOME", "/data"), ("HOME", "/Users/u")]);
        assert_eq!(
            data_dir(os, &e),
            Some(PathBuf::from(
                "/Users/u/Library/Application Support/kestrel"
            ))
        );
        assert_eq!(
            cache_dir(os, &e),
            Some(PathBuf::from("/Users/u/Library/Caches/kestrel"))
        );
        assert_eq!(data_dir(os, &env_of(&[])), None);
    }

    #[test]
    fn windows_keeps_everything_in_local_app_data() {
        let os = Os::Windows;
        let local = r"C:\Users\u\AppData\Local";
        let e = env_of(&[
            ("LOCALAPPDATA", r"C:\Users\u\AppData\Local"),
            ("HOME", "/ignored"),
        ]);
        assert_eq!(data_dir(os, &e), Some(PathBuf::from(local).join("kestrel")));
        assert_eq!(
            cache_dir(os, &e),
            Some(PathBuf::from(local).join("kestrel").join("cache"))
        );
        assert_eq!(data_dir(os, &env_of(&[("HOME", "/ignored")])), None);
    }

    #[test]
    fn the_socket_path_is_v3s_on_linux() {
        let os = Os::Linux;
        let run = env_of(&[("XDG_RUNTIME_DIR", "/run/user/1000"), ("USER", "adi")]);
        assert_eq!(
            socket_path(os, &run),
            PathBuf::from("/run/user/1000/kestrel.sock")
        );
        let user = env_of(&[("XDG_RUNTIME_DIR", ""), ("USER", "adi")]);
        assert_eq!(
            socket_path(os, &user),
            PathBuf::from("/tmp/kestrel-adi.sock")
        );
        assert_eq!(
            socket_path(os, &env_of(&[])),
            PathBuf::from("/tmp/kestrel-user.sock")
        );
    }

    #[test]
    fn the_socket_path_on_macos_and_windows() {
        // macOS gives each user a private TMPDIR.
        let mac = env_of(&[("TMPDIR", "/var/folders/ab/T/"), ("USER", "u")]);
        assert_eq!(
            socket_path(Os::MacOs, &mac),
            PathBuf::from("/var/folders/ab/T/kestrel.sock")
        );
        assert_eq!(
            socket_path(Os::MacOs, &env_of(&[("USER", "u")])),
            PathBuf::from("/tmp/kestrel-u.sock")
        );
        let local = r"C:\Users\u\AppData\Local";
        let win = env_of(&[
            ("LOCALAPPDATA", r"C:\Users\u\AppData\Local"),
            ("TEMP", r"C:\T"),
        ]);
        assert_eq!(
            socket_path(Os::Windows, &win),
            PathBuf::from(local).join("kestrel").join("kestrel.sock")
        );
        assert_eq!(
            socket_path(Os::Windows, &env_of(&[("TEMP", r"C:\T")])),
            PathBuf::from(r"C:\T").join("kestrel.sock")
        );
    }

    #[test]
    fn each_system_opens_a_browser_its_own_way() {
        let url = "http://127.0.0.1:7811/#t=ab12";
        assert_eq!(words(&open_url(Os::Linux, url)), ["xdg-open", url]);
        assert_eq!(words(&open_url(Os::MacOs, url)), ["open", url]);
        // rundll32 passes the URL on whole, the fragment (the page's key) included; `cmd /c start`
        // would treat characters such as & specially.
        assert_eq!(
            words(&open_url(Os::Windows, url)),
            ["rundll32", "url.dll,FileProtocolHandler", url]
        );
    }

    #[test]
    fn notifications_on_linux_are_v3s_notify_send() {
        let c = notify(Os::Linux, "Kestrel", "Call mom\n9:00").unwrap();
        assert_eq!(
            words(&c),
            [
                "notify-send",
                "--app-name=Kestrel",
                "--icon=appointment-soon",
                "Kestrel",
                "Call mom\n9:00"
            ]
        );
    }

    #[test]
    fn notifications_on_macos_quote_the_text_for_applescript() {
        let c = notify(Os::MacOs, "Due \"today\"", "a\\b\nc").unwrap();
        assert_eq!(
            words(&c),
            [
                "osascript",
                "-e",
                r#"display notification "a\\b\nc" with title "Due \"today\"""#
            ]
        );
    }

    #[test]
    fn notifications_on_windows_wait_for_the_app() {
        assert!(notify(Os::Windows, "t", "b").is_none());
    }

    #[test]
    fn the_utc_offset_on_unix_comes_from_date() {
        for os in [Os::Linux, Os::MacOs] {
            assert_eq!(words(&utc_offset_command(os)), ["date", "+%z"]);
            assert_eq!(parse_utc_offset(os, "+0530"), Some(19_800));
            assert_eq!(parse_utc_offset(os, "-0800"), Some(-28_800));
            assert_eq!(parse_utc_offset(os, "+0000"), Some(0));
            assert_eq!(parse_utc_offset(os, "+0545\n"), Some(20_700)); // Nepal; `date` ends with \n
            for bad in [
                "", "0530", "+530", "+05:30", "+1500", "+0560", "IST", "+05a0",
            ] {
                assert_eq!(parse_utc_offset(os, bad), None, "{bad:?}");
            }
        }
    }

    #[test]
    fn the_utc_offset_on_windows_comes_from_powershell_in_minutes() {
        let c = utc_offset_command(Os::Windows);
        assert_eq!(c.get_program(), "powershell");
        assert!(c.get_args().any(|a| a == "-NoProfile"));
        assert_eq!(parse_utc_offset(Os::Windows, "330\r\n"), Some(19_800));
        assert_eq!(parse_utc_offset(Os::Windows, "-480"), Some(-28_800));
        assert_eq!(parse_utc_offset(Os::Windows, "0"), Some(0));
        assert_eq!(parse_utc_offset(Os::Windows, "345"), Some(20_700));
        for bad in ["", "+0530", "5.5", "900", "-900", "x"] {
            assert_eq!(parse_utc_offset(Os::Windows, bad), None, "{bad:?}");
        }
    }
}
