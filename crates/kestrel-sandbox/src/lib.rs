//! Model-written Python in WebAssembly, the same on Linux, macOS and Windows (v4 A4, blueprint §4.9;
//! replaces v3's bubblewrap, ADR-044). CPython built for WASI runs in wasmtime with one writable
//! folder (its `/`, so relative paths work as in v3), its standard library read-only at `/lib`, no
//! other file of this device, no sockets (WASI preview 1 has none), no subprocesses or threads, and
//! capped memory and output.
//!
//! This runs in a process of its own (`kestrel sandbox-python`): the caller ends it at its time limit
//! whatever Python is doing (a `time.sleep` holds a thread in the host, past any interrupt), and a
//! crash here cannot reach the service.

use std::path::{Path, PathBuf};

use wasmtime::{
    Cache, CacheConfig, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::{FsPerms, I32Exit, WasiCtxBuilder};

/// What Python may allocate: v3's limit.
pub const MEMORY_BYTES: usize = 512 << 20;
/// What it may print, on stdout and on stderr each.
pub const OUTPUT_BYTES: usize = 10 << 20;
/// CPython's own recursion limit must end deep calls before WebAssembly's stack does (the figure
/// CPython's WASI runner uses); the host thread needs room for it.
const WASM_STACK_BYTES: usize = 16 << 20;
const THREAD_STACK_BYTES: usize = 32 << 20;

/// How a run ended.
#[derive(Debug, PartialEq)]
pub struct Ended {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs `script` (a file in `work`) with the Python in `python` (python.wasm and lib/), fed `stdin`.
/// `cache` keeps the compiled module: the first run compiles for some seconds, the next load it.
/// Err: the sandbox itself could not run; nothing was checked.
pub fn run(
    python: &Path,
    cache: Option<&Path>,
    work: &Path,
    script: &str,
    stdin: Vec<u8>,
) -> Result<Ended, String> {
    if !python.join("python.wasm").is_file() {
        return Err(format!("no python.wasm in {}", python.display()));
    }
    let (python, work, script) = (python.to_owned(), work.to_owned(), script.to_owned());
    let cache = cache.map(Path::to_owned);
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(move || run_here(&python, cache, &work, &script, stdin))
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the sandbox's thread ended early".to_string())?
}

struct Guest {
    wasi: WasiP1Ctx,
    limits: StoreLimits,
}

fn engine(cache: Option<PathBuf>) -> Result<Engine, String> {
    let mut config = Config::new();
    // Calls here are synchronous, on our thread; the setting must only allow the stack above.
    config.async_stack_size(THREAD_STACK_BYTES);
    config.max_wasm_stack(WASM_STACK_BYTES);
    if let Some(dir) = cache {
        let mut keep = CacheConfig::new();
        keep.with_directory(dir);
        // Without a cache the module only compiles each time: slower, nothing lost.
        config.cache(Cache::new(keep).ok());
    }
    Engine::new(&config).map_err(|e| e.to_string())
}

fn run_here(
    python: &Path,
    cache: Option<PathBuf>,
    work: &Path,
    script: &str,
    stdin: Vec<u8>,
) -> Result<Ended, String> {
    let fail = |e: wasmtime::Error| format!("{e:#}");
    let engine = engine(cache)?;
    let module = Module::from_file(&engine, python.join("python.wasm")).map_err(fail)?;
    let mut linker = Linker::<Guest>::new(&engine);
    p1::add_to_linker_sync(&mut linker, |g: &mut Guest| &mut g.wasi).map_err(fail)?;

    let (out, err) = (
        MemoryOutputPipe::new(OUTPUT_BYTES),
        MemoryOutputPipe::new(OUTPUT_BYTES),
    );
    let mut wasi = WasiCtxBuilder::new();
    // -E: no PYTHON* variables, -S: no site-packages (the standard library only), -B: no .pyc.
    wasi.args(&["python.wasm", "-E", "-S", "-B", script])
        .env("HOME", "/")
        .stdin(MemoryInputPipe::new(stdin))
        .stdout(out.clone())
        .stderr(err.clone());
    wasi.preopened_dir(work, "/", FsPerms::ReadWrite)
        .map_err(fail)?;
    wasi.preopened_dir(python.join("lib"), "/lib", FsPerms::ReadOnly)
        .map_err(fail)?;

    let guest = Guest {
        wasi: wasi.build_p1(),
        limits: StoreLimitsBuilder::new().memory_size(MEMORY_BYTES).build(),
    };
    let mut store = Store::new(&engine, guest);
    store.limiter(|g| &mut g.limits);
    let start = linker
        .instantiate(&mut store, &module)
        .and_then(|i| i.get_typed_func::<(), ()>(&mut store, "_start"))
        .map_err(fail)?;

    let mut trap = String::new();
    let code = match start.call(&mut store, ()) {
        Ok(()) => 0,
        Err(e) => match e.downcast_ref::<I32Exit>() {
            Some(exit) => exit.0,
            // Past the output cap, out of stack: the run failed, and says how.
            None => {
                trap = format!("\nthe sandbox stopped this run: {}", e.root_cause());
                1
            }
        },
    };
    let mut stderr = err.contents().to_vec();
    stderr.extend_from_slice(trap.as_bytes());
    Ok(Ended {
        code,
        stdout: out.contents().to_vec(),
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn python() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/python-wasi")
    }

    /// One compile for all the tests, beside the build's other output.
    fn cache() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/python-wasi-cache")
    }

    struct Work(PathBuf);

    impl Drop for Work {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn work(tag: &str, code: &str, files: &[(&str, &str)]) -> Work {
        let d = std::env::temp_dir().join(format!("kestrel_sandbox_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("main.py"), code).unwrap();
        for (name, text) in files {
            std::fs::write(d.join(name), text).unwrap();
        }
        Work(d)
    }

    fn go(w: &Work, stdin: &str) -> (i32, String, String) {
        let e = run(&python(), Some(&cache()), &w.0, "main.py", stdin.into()).unwrap();
        let text = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
        (e.code, text(e.stdout), text(e.stderr))
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn code_runs_with_its_input_and_an_error_comes_back() {
        let w = work("input", "print(sum(int(x) for x in input().split()))", &[]);
        assert_eq!(go(&w, "1 2 3\n"), (0, "6\n".into(), String::new()));
        let w = work("error", "1 / 0", &[]);
        let (code, _, err) = go(&w, "");
        assert_ne!(code, 0);
        assert!(err.contains("ZeroDivisionError"), "{err}");
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn files_lie_beside_the_code_and_the_exit_code_comes_back() {
        let code = "import solution, sys\nprint(solution.f())\nprint(open('data.txt').read())\nsys.exit(3)";
        let w = work(
            "files",
            code,
            &[
                ("solution.py", "def f():\n    return 7\n"),
                ("data.txt", "hi"),
            ],
        );
        assert_eq!(go(&w, ""), (3, "7\nhi\n".into(), String::new()));
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn only_the_work_folder_can_be_written_and_nothing_else_of_this_device_seen() {
        let code = "import os\n\
            open('made.txt', 'w').write('here')\n\
            print(sorted(os.listdir('.')))\n\
            print(os.path.exists('/home'), os.path.exists('/etc/passwd'), os.path.exists('/tmp'))\n\
            print(os.path.exists('..' + os.sep + 'made.txt'), os.path.exists('../../etc'))\n\
            print(os.environ.get('HOME'), 'PATH' in os.environ)\n\
            try:\n    open('/lib/python3.14/os.py', 'a')\n    print('wrote lib')\n\
            except OSError:\n    print('lib is read-only')\n";
        let w = work("walls", code, &[]);
        let (code, out, err) = go(&w, "");
        assert_eq!(code, 0, "{err}");
        assert_eq!(
            out,
            "['made.txt', 'main.py']\nFalse False False\nFalse False\n/ False\nlib is read-only\n"
        );
        assert_eq!(
            std::fs::read_to_string(w.0.join("made.txt")).unwrap(),
            "here"
        );
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn there_is_no_network_and_no_subprocess() {
        let code = "for what in ('socket', 'subprocess'):\n\
            \x20   try:\n\
            \x20       if what == 'socket':\n\
            \x20           import socket\n\
            \x20           socket.create_connection(('1.1.1.1', 80), timeout=2)\n\
            \x20       else:\n\
            \x20           import subprocess\n\
            \x20           subprocess.run(['ls'])\n\
            \x20       print(what, 'worked')\n\
            \x20   except Exception as e:\n\
            \x20       print(what, 'refused')\n";
        let w = work("net", code, &[]);
        assert_eq!(go(&w, "").1, "socket refused\nsubprocess refused\n");
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn memory_is_capped_and_deep_recursion_is_python_s_own_error() {
        let code = "try:\n    x = bytearray(600 * 1024 * 1024)\n    print('got it')\n\
            except MemoryError:\n    print('no memory')\n\
            def f(n):\n    return f(n + 1)\n\
            try:\n    f(0)\nexcept RecursionError:\n    print('too deep')\n\
            print(len(bytearray(100 * 1024 * 1024)))\n";
        let w = work("limits", code, &[]);
        let (code, out, err) = go(&w, "");
        assert_eq!(
            (code, out.as_str()),
            (0, "no memory\ntoo deep\n104857600\n"),
            "{err}"
        );
    }

    #[test]
    #[ignore = "needs scripts/fetch_python_wasi.sh"]
    fn endless_printing_stops_at_the_cap() {
        let w = work("flood", "while True:\n    print('x' * 1000000)", &[]);
        let (code, out, _) = go(&w, "");
        assert_ne!(code, 0);
        assert!(out.len() <= OUTPUT_BYTES, "{}", out.len());
    }

    #[test]
    fn no_python_is_an_error_not_a_failed_run() {
        let w = work("none", "print(1)", &[]);
        let e = run(Path::new("/no/such/python"), None, &w.0, "main.py", vec![]).unwrap_err();
        assert!(e.contains("python.wasm"), "{e}");
    }
}
