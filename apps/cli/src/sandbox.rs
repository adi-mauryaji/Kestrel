//! v3: model-written Python run where it can do no harm (ADR-044), the same command line as
//! experiments/v3/sandbox.py: bubblewrap with every namespace unshared (no network, own PIDs), /usr
//! read-only, an empty /tmp and a fresh work folder as the only writable place, a clean environment,
//! no /home; prlimit inside (512 MiB, 10 s CPU, 10 MiB files, 64 processes) and a wall-clock timeout;
//! `python3 -E -S` (standard library only).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PYTHON: &str = "/usr/bin/python3";

#[derive(Debug, PartialEq)]
pub struct Run {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// The sandbox cannot run at all (no bubblewrap): nothing was checked.
#[derive(Debug, PartialEq)]
pub struct Unavailable(pub String);

fn command(work: &Path, script: &str) -> Command {
    let mut c = Command::new("bwrap");
    c.args(["--unshare-all", "--die-with-parent", "--new-session"])
        .args(["--ro-bind", "/usr", "/usr", "--symlink", "usr/lib", "/lib"])
        .args([
            "--symlink",
            "usr/lib64",
            "/lib64",
            "--symlink",
            "usr/bin",
            "/bin",
        ])
        .args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"])
        .arg("--bind")
        .arg(work)
        .args(["/work", "--chdir", "/work", "--clearenv"])
        .args(["--setenv", "PATH", "/usr/bin", "--setenv", "HOME", "/work"])
        .args(["--setenv", "PYTHONDONTWRITEBYTECODE", "1"])
        .args([
            "prlimit",
            "--as=536870912",
            "--cpu=10",
            "--fsize=10485760",
            "--nproc=64",
            "--",
        ])
        .args([PYTHON, "-E", "-S", script]);
    c
}

/// A fresh folder under the system's temp directory, removed when dropped.
struct Work(PathBuf);

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn work_dir() -> std::io::Result<Work> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let d = std::env::temp_dir().join(format!("kestrel-sandbox-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&d)?;
    Ok(Work(d))
}

/// `code` as main.py in an empty folder, with `files` beside it, fed `stdin`.
pub fn run_python(
    code: &str,
    files: &[(&str, &str)],
    stdin: &str,
    timeout: Duration,
) -> Result<Run, Unavailable> {
    let fail = |e: std::io::Error| Unavailable(e.to_string());
    let work = work_dir().map_err(fail)?;
    for (name, text) in files.iter().chain([&("main.py", code)]) {
        std::fs::write(work.0.join(name), text).map_err(fail)?;
    }
    let mut child = command(&work.0, "main.py")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Unavailable(format!("bubblewrap: {e}")))?;
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin.as_bytes());
    }
    let start = Instant::now();
    let mut timed_out = false;
    while child.try_wait().map_err(fail)?.is_none() {
        if start.elapsed() >= timeout {
            let _ = child.kill();
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().map_err(fail)?;
    // The CPU limit ends a run with SIGKILL or SIGXCPU: a time limit too.
    let code = out.status.code();
    let cpu_out = matches!(code, Some(137) | Some(152));
    Ok(Run {
        ok: out.status.success() && !timed_out,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        timed_out: timed_out || cpu_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_secs(15);

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn code_runs_with_its_input_and_an_error_comes_back() {
        let r = run_python(
            "print(sum(int(x) for x in input().split()))",
            &[],
            "1 2 3\n",
            T,
        )
        .unwrap();
        assert_eq!((r.ok, r.stdout.as_str()), (true, "6\n"));
        let r = run_python("1 / 0", &[], "", T).unwrap();
        assert!(!r.ok && r.stderr.contains("ZeroDivisionError"));
    }

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn no_network_no_home_and_files_beside_the_code() {
        let code = "import os, socket\n\
                    try:\n    socket.create_connection(('1.1.1.1', 80), timeout=2); print('net')\n\
                    except OSError:\n    print('no net')\n\
                    print(os.path.exists('/home'))\n\
                    import solution\nprint(solution.f())";
        let r = run_python(code, &[("solution.py", "def f():\n    return 7\n")], "", T).unwrap();
        assert_eq!(r.stdout, "no net\nFalse\n7\n", "{r:?}");
    }

    #[test]
    #[ignore = "needs bubblewrap (Fedora: installed)"]
    fn an_endless_loop_is_stopped() {
        let r = run_python("while True:\n    pass", &[], "", Duration::from_secs(2)).unwrap();
        assert!(r.timed_out && !r.ok);
    }
}
