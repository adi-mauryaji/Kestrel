"""experiments/v3/sandbox.py -- runs model-written Python where it can do no harm (v3 code loop, ADR-036).

bubblewrap: every namespace unshared (no network, own PIDs), the system's /usr read-only, an empty /tmp and
a fresh work folder as the only writable place, a clean environment; the user's home is not there at all.
prlimit inside: address space 512 MiB, 10 s of CPU, 10 MiB per file, 64 processes; and a wall-clock
timeout from here. `python3 -E -S`: PYTHON* variables ignored, no site-packages (the standard library
only; also keeps out Fedora's abrt hook), the script's folder on the path. The app's code loop (Rust) is
to run the same command line.
"""
from __future__ import annotations

import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path

PYTHON = "/usr/bin/python3"
MEMORY = 512 * 2**20
CPU_S = 10
FILE_BYTES = 10 * 2**20
PROCS = 64


@dataclass
class Run:
    ok: bool
    returncode: int | None
    stdout: str
    stderr: str
    timed_out: bool


def command(work: Path, script: str) -> list[str]:
    return ["bwrap", "--unshare-all", "--die-with-parent", "--new-session",
            "--ro-bind", "/usr", "/usr", "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64",
            "--symlink", "usr/bin", "/bin", "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
            "--bind", str(work), "/work", "--chdir", "/work",
            "--clearenv", "--setenv", "PATH", "/usr/bin", "--setenv", "HOME", "/work",
            "--setenv", "PYTHONDONTWRITEBYTECODE", "1",
            "prlimit", f"--as={MEMORY}", f"--cpu={CPU_S}", f"--fsize={FILE_BYTES}", f"--nproc={PROCS}", "--",
            PYTHON, "-E", "-S", script]


def run_python(code: str, stdin: str = "", timeout_s: float = 15, files: dict[str, str] | None = None) -> Run:
    """`code` as main.py in an empty folder (with `files` beside it), fed `stdin`."""
    with tempfile.TemporaryDirectory(prefix="kestrel-sandbox-") as d:
        work = Path(d)
        for name, text in {**(files or {}), "main.py": code}.items():
            (work / name).write_text(text, encoding="utf-8")
        try:
            p = subprocess.run(command(work, "main.py"), input=stdin, capture_output=True, text=True,
                               timeout=timeout_s)
        except subprocess.TimeoutExpired as e:
            out = e.stdout.decode() if isinstance(e.stdout, bytes) else (e.stdout or "")
            err = e.stderr.decode() if isinstance(e.stderr, bytes) else (e.stderr or "")
            return Run(False, None, out, err, True)
    # The CPU limit ends a run with SIGKILL or SIGXCPU: a time limit too.
    cpu_out = p.returncode in (128 + 9, 128 + 24)
    return Run(p.returncode == 0, p.returncode, p.stdout, p.stderr, cpu_out)
