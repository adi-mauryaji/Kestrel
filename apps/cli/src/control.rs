//! Starting and stopping the always-on assistant (ADR-034). On Linux `kestrel stop` stops the
//! systemd user unit that scripts/install_service.sh installs: the service and its Gemma worker exit, and their
//! memory is free. `kestrel open`, `chat` and `say` start it again when it is stopped (rules +
//! encoder load in about a second); the next login starts it too.

use std::error::Error;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

/// The unit scripts/install_service.sh writes.
pub const UNIT: &str = "kestrel.service";

/// How long a start may take: `WAIT_TRIES` pauses of `PAUSE`.
const WAIT_TRIES: u32 = 240;
const PAUSE: std::time::Duration = std::time::Duration::from_millis(250);

/// `kestrel stop`: stops the installed service, its Gemma worker with it, and removes the socket
/// and page address it leaves behind.
pub fn stop(
    systemctl: &mut dyn FnMut(&[&str]) -> bool,
    answers: &mut dyn FnMut() -> bool,
    leftovers: &[PathBuf],
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    if !systemctl(&["is-active", "--quiet", UNIT]) {
        if answers() {
            return Err(
                "this Kestrel was started by hand (kestrel serve): stop it with Ctrl-C in its terminal"
                    .into(),
            );
        }
        writeln!(out, "Kestrel is not running")?;
        return Ok(());
    }
    if !systemctl(&["stop", UNIT]) {
        return Err(format!(
            "`systemctl --user stop {UNIT}` failed: `journalctl --user -u {UNIT}` says why"
        )
        .into());
    }
    // systemd ends the whole unit, the Gemma worker too; a stale page address would mislead `open`.
    for f in leftovers {
        let _ = std::fs::remove_file(f);
    }
    writeln!(
        out,
        "Kestrel stopped: its memory is free (kestrel open, chat or say starts it again)"
    )?;
    Ok(())
}

/// Before `kestrel open`, `chat` or `say`: starts the installed service when it is stopped and
/// waits until `ready`. Without the service it does nothing; the command then says what is missing.
pub fn wake(
    systemctl: &mut dyn FnMut(&[&str]) -> bool,
    ready: &mut dyn FnMut() -> bool,
    pause: &mut dyn FnMut(),
    err: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    if ready() || !systemctl(&["start", UNIT]) {
        return Ok(());
    }
    writeln!(err, "starting Kestrel (rules + encoder, about a second)")?;
    for _ in 0..WAIT_TRIES {
        if ready() {
            return Ok(());
        }
        pause();
    }
    Err(format!(
        "Kestrel did not start within {} s: `journalctl --user -u {UNIT}` says why",
        (PAUSE * WAIT_TRIES).as_secs()
    )
    .into())
}

fn var(k: &str) -> Option<OsString> {
    std::env::var_os(k)
}

/// `systemctl --user ARGS...` with its output dropped; false when it fails or there is none.
/// Only Linux has the unit: on macOS and Windows Kestrel is started by hand until the app (v4
/// phase A3), so `stop` and `wake` see no service manager there.
fn systemctl(args: &[&str]) -> bool {
    if kestrel_platform::Os::current() != kestrel_platform::Os::Linux {
        return false;
    }
    std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The service answers on its socket.
#[cfg(unix)]
fn answers() -> bool {
    std::os::unix::net::UnixStream::connect(crate::service::socket_path(&var)).is_ok()
}

/// The service answers on its page (Windows has no socket).
#[cfg(not(unix))]
fn answers() -> bool {
    crate::web::page_answers(&var)
}

/// `kestrel stop` on this machine.
pub fn stop_here(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let left = [
        crate::service::socket_path(&var),
        crate::web::url_file(&var),
    ];
    stop(&mut systemctl, &mut answers, &left, out)
}

/// `wake` on this machine: ready once the socket answers, and for `open` (`page`) once the page's
/// address is written too.
pub fn wake_here(page: bool) -> Result<(), Box<dyn Error>> {
    if answers() {
        return Ok(()); // running: `open` finds its page, or says it is off
    }
    let url = crate::web::url_file(&var);
    wake(
        &mut systemctl,
        &mut || answers() && (!page || url.exists()),
        &mut || std::thread::sleep(PAUSE),
        &mut std::io::stderr(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::rc::Rc;

    /// Each systemctl call, its arguments joined.
    type Calls = Rc<RefCell<Vec<String>>>;

    /// A systemctl that records its calls and succeeds for those in `ok`.
    fn fake(ok: &[&str]) -> (Calls, impl FnMut(&[&str]) -> bool) {
        let ok: Vec<String> = ok.iter().map(|&c| c.into()).collect();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let seen = calls.clone();
        let run = move |args: &[&str]| {
            let call = args.join(" ");
            seen.borrow_mut().push(call.clone());
            ok.contains(&call)
        };
        (calls, run)
    }

    #[test]
    fn stop_stops_the_installed_service_and_clears_what_it_left() {
        let dir = std::env::temp_dir().join(format!("kestrel_stop_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let left = [dir.join("kestrel.sock"), dir.join("kestrel.url")];
        for f in &left {
            std::fs::write(f, "").unwrap();
        }
        let (calls, mut systemctl) =
            fake(&["is-active --quiet kestrel.service", "stop kestrel.service"]);
        let mut out = Vec::new();
        stop(&mut systemctl, &mut || panic!("not asked"), &left, &mut out).unwrap();
        assert_eq!(
            *calls.borrow(),
            ["is-active --quiet kestrel.service", "stop kestrel.service"]
        );
        assert!(left.iter().all(|f| !f.exists()));
        assert!(
            String::from_utf8(out)
                .unwrap()
                .starts_with("Kestrel stopped")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stop_says_when_nothing_runs() {
        let (calls, mut systemctl) = fake(&[]);
        let mut out = Vec::new();
        stop(&mut systemctl, &mut || false, &[], &mut out).unwrap();
        assert_eq!(*calls.borrow(), ["is-active --quiet kestrel.service"]);
        assert_eq!(String::from_utf8(out).unwrap(), "Kestrel is not running\n");
    }

    #[test]
    fn stop_leaves_a_hand_started_serve_to_its_terminal() {
        let (_, mut systemctl) = fake(&[]);
        let e = stop(&mut systemctl, &mut || true, &[], &mut Vec::new()).unwrap_err();
        assert!(e.to_string().contains("Ctrl-C"), "{e}");
    }

    #[test]
    fn stop_reports_a_failed_stop() {
        let (_, mut systemctl) = fake(&["is-active --quiet kestrel.service"]);
        let e = stop(&mut systemctl, &mut || true, &[], &mut Vec::new()).unwrap_err();
        assert!(
            e.to_string()
                .contains("systemctl --user stop kestrel.service"),
            "{e}"
        );
    }

    #[test]
    fn wake_does_nothing_while_it_runs() {
        let (calls, mut systemctl) = fake(&[]);
        let mut err = Vec::new();
        wake(
            &mut systemctl,
            &mut || true,
            &mut || panic!("no wait"),
            &mut err,
        )
        .unwrap();
        assert!(calls.borrow().is_empty());
        assert!(err.is_empty());
    }

    #[test]
    fn wake_starts_a_stopped_service_and_waits_until_it_is_ready() {
        let (calls, mut systemctl) = fake(&["start kestrel.service"]);
        let mut looks = 0;
        let mut pauses = 0;
        let mut err = Vec::new();
        wake(
            &mut systemctl,
            &mut || {
                looks += 1;
                looks > 3
            },
            &mut || pauses += 1,
            &mut err,
        )
        .unwrap();
        assert_eq!(*calls.borrow(), ["start kestrel.service"]);
        assert_eq!(pauses, 2);
        assert!(
            String::from_utf8(err)
                .unwrap()
                .starts_with("starting Kestrel")
        );
    }

    #[test]
    fn wake_leaves_a_missing_service_to_the_command() {
        let (calls, mut systemctl) = fake(&[]);
        let mut err = Vec::new();
        wake(
            &mut systemctl,
            &mut || false,
            &mut || panic!("no wait"),
            &mut err,
        )
        .unwrap();
        assert_eq!(*calls.borrow(), ["start kestrel.service"]);
        assert!(err.is_empty());
    }

    #[test]
    fn wake_gives_up_when_it_never_gets_ready() {
        let (_, mut systemctl) = fake(&["start kestrel.service"]);
        let mut pauses = 0;
        let e = wake(
            &mut systemctl,
            &mut || false,
            &mut || pauses += 1,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(pauses, WAIT_TRIES);
        assert!(
            e.to_string()
                .contains("journalctl --user -u kestrel.service"),
            "{e}"
        );
    }
}
