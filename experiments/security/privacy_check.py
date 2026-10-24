"""experiments/security/privacy_check.py -- Ch. 42's privacy and security checks, run on this machine (Week 15).

    python experiments/security/privacy_check.py      # ~2 min (loads the models twice)

1  No network: (a) no network-client crate in the runtime's dependency tree (`cargo tree -e normal`, kestrel-cli
   with the llama feature); (b, c) every scenario run in a fresh network namespace (`unshare -rn`, no sudo) under
   `strace -f -e trace=network,openat`: it must succeed offline with zero AF_INET/AF_INET6 connect() calls.
2  No cloud path: no http(s):// in runtime source code (comments allowed).
3  File access: the files the scenarios opened, outside the repository, the scratch directory and the system roots.
4  No shell: no std::process::Command in the runtime crates.
7  Canary: a canary string goes into requests and a taught alias; the default outputs (telemetry, route-items rows)
   must not contain it.
8  Deletion: after `kestrel memory delete` and `vacuum`, the canary is gone from the database file and its WAL/SHM.
Scenarios: route-items on 3 canary requests (Intents-v0 config); a scripted `ask` session (teach a canary alias,
use it, a task, a preference) with KESTREL_TELEMETRY on; `kestrel memory list / delete / vacuum`.
What this cannot show (Ch. 42): code paths outside these scenarios, other OSes, debug logging, swap, backups.
Out: experiments/logs/privacy/<UTC>.json.
"""
from __future__ import annotations

import datetime as dt
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
KESTREL = ROOT / "target" / "release" / "kestrel"
CONFIG = ROOT / "experiments" / "configs" / "router_intents_v0.json"
OUT_DIR = ROOT / "experiments" / "logs" / "privacy"
CANARY = "zqcanary7f3a"  # lowercase letters and digits: survives the normalizer unchanged
NET_CRATES = re.compile(r"\b(reqwest|hyper|ureq|curl|openssl|rustls|native-tls|tokio)\b v", re.I)
RUNTIME_SRC = ["crates/kestrel-core", "crates/kestrel-nlu", "crates/kestrel-inference", "crates/kestrel-router",
               "crates/kestrel-memory", "crates/kestrel-learning", "crates/kestrel-tools", "apps/cli"]
SYSTEM_ROOTS = ["/proc", "/sys", "/dev", "/usr", "/lib", "/lib64", "/etc"]


def network_crates(tree: str) -> list[str]:
    """The dependency-tree entries that are network clients."""
    out = []
    for line in tree.splitlines():
        m = NET_CRATES.search(line)
        if m:
            out.append(line[m.start():].split(" (")[0].strip())
    return out


def connects(trace: str) -> dict:
    """connect() calls in an strace log: AF_INET/AF_INET6 (network) vs AF_UNIX (local)."""
    lines = [x for x in trace.splitlines() if "connect(" in x]
    return {"inet": sum("AF_INET" in x for x in lines), "unix": sum("AF_UNIX" in x for x in lines),
            "inet_lines": [x for x in lines if "AF_INET" in x][:5]}


def code_lines(src: str, pattern: str) -> list[tuple[int, str]]:
    """(line number, line) matching pattern outside // comments."""
    rx = re.compile(pattern)
    out = []
    for n, line in enumerate(src.splitlines(), 1):
        code = line.split("//", 1)[0] if not re.search(r'"[^"]*//', line) else line
        if line.lstrip().startswith("//"):
            continue
        if rx.search(code):
            out.append((n, line.strip()))
    return out


def opened_outside(trace: str, allowed: list[str], cwd: str = str(ROOT)) -> list[str]:
    """Paths successfully opened (openat >= 0) outside the allowed roots; relative paths resolve against cwd."""
    out = set()
    for m in re.finditer(r'openat\([^,]*, "([^"]+)", [^)]*\)\s*=\s*(-?\d+)', trace):
        path = os.path.normpath(os.path.join(cwd, m.group(1)))
        if int(m.group(2)) >= 0 and not any(path == a or path.startswith(a.rstrip("/") + "/") for a in allowed):
            out.add(path)
    return sorted(out)


def canary_hits(paths, canary: str = CANARY) -> list[str]:
    """The files whose bytes contain the canary (any case)."""
    needle = canary.lower().encode()
    return [str(p) for p in paths if Path(p).exists() and needle in Path(p).read_bytes().lower()]


def traced(cmd: list[str], trace: Path, *, stdin: str | None = None, env=None) -> subprocess.CompletedProcess:
    """cmd in a fresh network namespace under strace (network and openat calls, all threads)."""
    full = ["unshare", "-rn", "strace", "-f", "-qq", "-e", "trace=network,openat", "-o", str(trace), "--", *cmd]
    return subprocess.run(full, input=stdin, capture_output=True, text=True, cwd=ROOT, env=env, timeout=900)


def opened(trace: str) -> list[str]:
    """Every path successfully opened in an strace log."""
    return [m.group(1) for m in re.finditer(r'openat\([^,]*, "([^"]+)", [^)]*\)\s*=\s*(\d+)', trace)]


def probe(tmp: Path) -> dict:
    """Positive control (Ch. 42 [T]): a deliberate connect() to a documentation address, under the same namespace
    and strace setup, must show up in the trace, or a zero count would mean nothing."""
    code = "import socket; s = socket.socket(); s.settimeout(1); print(s.connect_ex(('203.0.113.1', 80)))"
    p = traced([sys.executable, "-c", code], tmp / "probe.trace")
    return {"exit": p.returncode, "connects": connects((tmp / "probe.trace").read_text())}


def static_checks() -> dict:
    tree = subprocess.run(["cargo", "tree", "-e", "normal", "-p", "kestrel-cli", "--features", "llama"],
                          capture_output=True, text=True, cwd=ROOT).stdout
    urls, shells = [], []
    for rel in RUNTIME_SRC:
        for f in sorted((ROOT / rel / "src").rglob("*.rs")):
            src = f.read_text(encoding="utf-8").split("#[cfg(test)]")[0]
            urls += [f"{f.relative_to(ROOT)}:{n}: {x}" for n, x in code_lines(src, r"https?://")]
            shells += [f"{f.relative_to(ROOT)}:{n}: {x}" for n, x in code_lines(src, r"process::Command|Command::new")]
    return {"1a_network_crates": network_crates(tree), "1a_tree_lines": len(tree.splitlines()),
            "2_urls_in_code": urls, "4_shell_in_runtime": shells}


def scenarios(tmp: Path) -> dict:
    env = dict(os.environ, KESTREL_MEMORY=str(tmp / "memory.db"), KESTREL_TELEMETRY=str(tmp / "telemetry.jsonl"))
    items = tmp / "items.jsonl"
    items.write_text("".join(json.dumps({"id": f"canary-{k}", "text": t}) + "\n" for k, t in enumerate(
        [f"search my notes for {CANARY}", f"remind me to email {CANARY} tomorrow", f"open {CANARY}.txt"])))
    script = "\n".join([f"when I say {CANARY} doc, I mean team sync", "y", f"open my {CANARY} doc",
                        f"remind me to call {CANARY} tomorrow", "I prefer short answers", "quit"]) + "\n"
    out = {}
    runs = {"route_items": ([str(KESTREL), "route-items", str(CONFIG), str(items), str(tmp / "rows.jsonl")], None),
            "ask": ([str(KESTREL), "ask", str(CONFIG)], script),
            "memory_list": ([str(KESTREL), "memory", "--db", str(tmp / "memory.db"), "list"], None)}
    for name, (cmd, stdin) in runs.items():
        p = traced(cmd, tmp / f"{name}.trace", stdin=stdin, env=env)
        out[name] = {"exit": p.returncode, "trace": (tmp / f"{name}.trace").read_text(), "stdout": p.stdout,
                     "stderr_tail": p.stderr[-400:]}
    ids = [m.group(1) for line in out["memory_list"]["stdout"].splitlines() if CANARY in line.lower()
           for m in [re.match(r"#(\d+)", line)] if m]
    for i in ids:
        p = traced([str(KESTREL), "memory", "--db", str(tmp / "memory.db"), "delete", i], tmp / f"delete_{i}.trace",
                   env=env)
        out[f"delete_{i}"] = {"exit": p.returncode, "trace": (tmp / f"delete_{i}.trace").read_text()}
    p = traced([str(KESTREL), "memory", "--db", str(tmp / "memory.db"), "vacuum"], tmp / "vacuum.trace", env=env)
    out["vacuum"] = {"exit": p.returncode, "trace": (tmp / "vacuum.trace").read_text()}
    out["_canary_rows_in_memory"] = ids
    return out


def main() -> int:
    if not KESTREL.exists():
        sys.exit("build the binary first: cargo build --release -p kestrel-cli --features llama")
    report = {"date": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
              "git_head": subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True,
                                         cwd=ROOT).stdout.strip(), "canary": CANARY} | static_checks()
    with tempfile.TemporaryDirectory(prefix="kestrel-privacy-") as d:
        tmp = Path(d)
        report["probe"] = probe(tmp)
        runs = scenarios(tmp)
        allowed = [str(ROOT), str(tmp)] + SYSTEM_ROOTS
        report["runs"] = {name: {"exit": r["exit"], "connects": connects(r["trace"]),
                                 "opened": len(opened(r["trace"])),
                                 "opened_models": sorted({Path(x).name for x in opened(r["trace"])
                                                          if Path(x).parent.name == "models"}),
                                 "opened_outside": opened_outside(r["trace"], allowed)}
                          for name, r in runs.items() if not name.startswith("_")}
        report["memory_rows_with_canary"] = len(runs["_canary_rows_in_memory"])
        report["7_canary_in_outputs"] = canary_hits([tmp / "telemetry.jsonl", tmp / "rows.jsonl",
                                                     tmp / "rows.meta.json"])
        report["7_telemetry_records"] = len((tmp / "telemetry.jsonl").read_text().splitlines()) \
            if (tmp / "telemetry.jsonl").exists() else 0
        db = [tmp / f"memory.db{s}" for s in ("", "-wal", "-shm")]
        report["8_canary_in_db_after_delete"] = [Path(p).name for p in canary_hits(db)]
    ok = {"0 the trace sees a deliberate connect() (positive control)": report["probe"]["connects"]["inet"] >= 1,
          "0 the scenarios' traces saw the model files opened": all(report["runs"][k]["opened_models"]
                                                                    for k in ("route_items", "ask")),
          "1a no network crates": not report["1a_network_crates"],
          "1b every scenario succeeded offline": all(r["exit"] == 0 for r in report["runs"].values()),
          "1c zero inet connect()": all(r["connects"]["inet"] == 0 for r in report["runs"].values()),
          "2 no URL in runtime code": not report["2_urls_in_code"],
          "3 nothing opened outside the allowed roots": not any(r["opened_outside"] for r in report["runs"].values()),
          "4 no shell in runtime crates": not report["4_shell_in_runtime"],
          "7 canary taught and stored": report["memory_rows_with_canary"] > 0,
          "7 no canary in telemetry or output rows": not report["7_canary_in_outputs"]
          and report["7_telemetry_records"] > 0,
          "8 canary gone from the database after delete + vacuum": not report["8_canary_in_db_after_delete"]}
    report["checks"] = ok
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    stamp = report["date"].replace(":", "").replace("-", "").replace("+0000", "Z")
    path = OUT_DIR / f"{stamp}.json"
    path.write_text(json.dumps(report, indent=1) + "\n")
    for k, v in ok.items():
        print(f"{'PASS' if v else 'FAIL'}  {k}")
    print(f"-> {path.relative_to(ROOT)}")
    return 0 if all(ok.values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
