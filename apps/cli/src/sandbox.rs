//! Model-written Python run where it can do no harm (ADR-044's limits; v4 A4, blueprint §4.9): the
//! same on Linux, macOS and Windows. `kestrel sandbox-python`, a process of its own, runs CPython in
//! WebAssembly (crates/kestrel-sandbox: one work folder, no network, no other file of this device,
//! 512 MiB, capped output). Here: the fresh work folder, the wall-clock limit, and a limit on what
//! the code writes to disk, each ended by killing that process.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What a run may write in its work folder, all files together.
const WORK_BYTES: u64 = 64 << 20;
/// How often the work folder is measured.
const WATCH: Duration = Duration::from_millis(100);
/// The first run compiles Python (some seconds; kept in the cache): not on a task's clock.
const PREPARE: Duration = Duration::from_secs(300);
/// The exit code when the sandbox itself could not run.
const NOT_RUN: i32 = 125;

#[derive(Debug, PartialEq)]
pub struct Run {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// The sandbox cannot run at all (no python.wasm): nothing was checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Unavailable(pub String);

/// CPython for WASI among Kestrel's files: `python-wasi` where the installers put it, else
/// `third_party/python-wasi` in the repository (scripts/fetch_python_wasi.sh).
pub fn python_dir(root: &Path) -> Option<PathBuf> {
    ["python-wasi", "third_party/python-wasi"]
        .iter()
        .map(|d| root.join(d))
        .find(|d| d.join("python.wasm").is_file())
}

/// `kestrel sandbox-python PYTHON_DIR CACHE_DIR|- WORK SCRIPT`, fed the code's stdin.
pub fn child(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [python, cache, work, script] = args else {
        return Err("usage: kestrel sandbox-python PYTHON_DIR CACHE_DIR|- WORK SCRIPT".into());
    };
    let mut stdin = Vec::new();
    std::io::stdin().read_to_end(&mut stdin)?;
    let cache = (cache != "-").then(|| Path::new(cache));
    match kestrel_sandbox::run(Path::new(python), cache, Path::new(work), script, stdin) {
        Ok(ended) => {
            std::io::stdout().write_all(&ended.stdout)?;
            std::io::stderr().write_all(&ended.stderr)?;
            std::process::exit(ended.code)
        }
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(NOT_RUN)
        }
    }
}

/// This program, which holds `sandbox-python`.
#[cfg(not(test))]
fn kestrel() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}

#[cfg(not(test))]
fn kestrel_files() -> PathBuf {
    crate::root()
}

/// In tests: the repository (they run in apps/cli).
#[cfg(test)]
fn kestrel_files() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// In tests: the `kestrel` that cargo builds beside the test program (target/debug/deps/..).
#[cfg(test)]
fn kestrel() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let name = format!("kestrel{}", std::env::consts::EXE_SUFFIX);
    Ok(exe
        .parent()
        .and_then(Path::parent)
        .map_or(exe.clone(), |d| d.join(name)))
}

/// A fresh folder under the system's temp directory, removed when dropped.
struct Work(PathBuf);

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn work_dir() -> std::io::Result<Work> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let d = std::env::temp_dir().join(format!(
        "kestrel-sandbox-{}-{nanos}-{n}",
        std::process::id()
    ));
    std::fs::create_dir(&d)?;
    Ok(Work(d))
}

/// All the bytes under `dir`.
fn bytes_under(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => bytes_under(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// One run in `work`: its output read as it comes (a full pipe must not hold the code up).
fn run_in(python: &Path, work: &Path, stdin: &str, timeout: Duration) -> Result<Run, Unavailable> {
    let fail = |e: std::io::Error| Unavailable(e.to_string());
    let var = |k: &str| std::env::var_os(k);
    let cache = kestrel_platform::cache_dir(kestrel_platform::Os::current(), &var)
        .map(|d| d.join("python-wasm"));
    let mut run = Command::new(kestrel().map_err(fail)?);
    run.arg("sandbox-python")
        .arg(python)
        .arg(cache.as_deref().unwrap_or(Path::new("-")))
        .arg(work)
        .arg("main.py");
    let mut child = run
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Unavailable(format!("the sandbox: {e}")))?;
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin.as_bytes());
    }
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut all = Vec::new();
            let _ = pipe.read_to_end(&mut all);
            String::from_utf8_lossy(&all).into_owned()
        })
    };
    let out = child.stdout.take().map(|p| read(Box::new(p)));
    let err = child.stderr.take().map(|p| read(Box::new(p)));
    let start = Instant::now();
    let mut watched = start;
    let (mut timed_out, mut too_much) = (false, false);
    while child.try_wait().map_err(fail)?.is_none() {
        if start.elapsed() >= timeout {
            timed_out = true;
        } else if watched.elapsed() >= WATCH {
            watched = Instant::now();
            too_much = bytes_under(work) > WORK_BYTES;
        }
        if timed_out || too_much {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().map_err(fail)?;
    let gather = |r: Option<std::thread::JoinHandle<String>>| {
        r.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    let (stdout, mut stderr) = (gather(out), gather(err));
    if status.code() == Some(NOT_RUN) {
        return Err(Unavailable(stderr.trim().to_string()));
    }
    if too_much {
        stderr.push_str("\nthe code wrote more than its limit, so it was stopped");
    }
    Ok(Run {
        ok: status.success() && !timed_out && !too_much,
        stdout,
        stderr,
        timed_out,
    })
}

/// The first run of this process compiles Python if no cached copy is there yet: once, with time for it.
fn prepared(python: &Path) -> Result<(), Unavailable> {
    static DONE: std::sync::OnceLock<Result<(), Unavailable>> = std::sync::OnceLock::new();
    DONE.get_or_init(|| {
        let fail = |e: std::io::Error| Unavailable(e.to_string());
        let work = work_dir().map_err(fail)?;
        std::fs::write(work.0.join("main.py"), "").map_err(fail)?;
        run_in(python, &work.0, "", PREPARE).map(|_| ())
    })
    .clone()
}

/// `code` as main.py in an empty folder, with `files` beside it, fed `stdin`.
pub fn run_python(
    code: &str,
    files: &[(&str, &str)],
    stdin: &str,
    timeout: Duration,
) -> Result<Run, Unavailable> {
    let fail = |e: std::io::Error| Unavailable(e.to_string());
    let python = python_dir(&kestrel_files()).ok_or_else(|| {
        Unavailable("no python.wasm among Kestrel's files: scripts/fetch_python_wasi.sh".into())
    })?;
    prepared(&python)?;
    let work = work_dir().map_err(fail)?;
    for (name, text) in files.iter().chain([&("main.py", code)]) {
        std::fs::write(work.0.join(name), text).map_err(fail)?;
    }
    run_in(&python, &work.0, stdin, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_secs(15);

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
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
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn no_home_and_files_beside_the_code() {
        let code = "import os\n\
                    print(os.path.exists('/home'))\n\
                    import solution\nprint(solution.f())";
        let r = run_python(code, &[("solution.py", "def f():\n    return 7\n")], "", T).unwrap();
        assert_eq!(r.stdout, "False\n7\n", "{r:?}");
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn an_endless_loop_and_a_long_sleep_are_stopped() {
        // Python's first compile is not on the clock below.
        run_python("", &[], "", T).unwrap();
        for code in ["while True:\n    pass", "import time\ntime.sleep(1000)"] {
            let t0 = Instant::now();
            let r = run_python(code, &[], "", Duration::from_secs(2)).unwrap();
            assert!(r.timed_out && !r.ok, "{code}: {r:?}");
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "{code}: {:?}",
                t0.elapsed()
            );
        }
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn much_output_does_not_hold_the_code_up() {
        let r = run_python("print('x' * 300000)\nprint('end')", &[], "", T).unwrap();
        assert!(r.ok && r.stdout.ends_with("end\n"), "{:?}", r.stderr);
        assert_eq!(r.stdout.len(), 300005);
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn writing_too_much_to_disk_is_stopped() {
        let code =
            "f = open('big.bin', 'wb')\nwhile True:\n    f.write(b'x' * 1048576)\n    f.flush()";
        let r = run_python(code, &[], "", T).unwrap();
        assert!(!r.ok && !r.timed_out, "{r:?}");
        assert!(r.stderr.contains("limit"), "{r:?}");
    }

    #[test]
    fn python_is_found_where_the_installers_or_the_fetch_script_put_it() {
        let d = std::env::temp_dir().join(format!("kestrel_python_dir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("third_party/python-wasi")).unwrap();
        assert_eq!(python_dir(&d), None);
        std::fs::write(d.join("third_party/python-wasi/python.wasm"), "").unwrap();
        assert_eq!(python_dir(&d), Some(d.join("third_party/python-wasi")));
        std::fs::create_dir_all(d.join("python-wasi")).unwrap();
        std::fs::write(d.join("python-wasi/python.wasm"), "").unwrap();
        assert_eq!(python_dir(&d), Some(d.join("python-wasi")));
        let _ = std::fs::remove_dir_all(&d);
    }
}
