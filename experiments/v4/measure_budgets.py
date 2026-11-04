#!/usr/bin/env python3
"""Blueprint §7's performance budgets, measured on this device with a release build (Linux only: /proc).

A scratch service with its own folders, so the real memory file is not touched; it asks real Gemma, so it
takes a few minutes of CPU. Close other heavy work first, and never run it beside a research run.

    python3 experiments/v4/measure_budgets.py [--idle-seconds 60] [--out FILE.json]
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request

REPO = pathlib.Path(__file__).resolve().parents[2]
CONFIG = "experiments/configs/router_intents_v0.json"
CHAT = ["explain recursion in one line", "what is a noun, in one line", "explain gravity in one line",
        "what is a prime number, in one line"]
MAIL = ("write a detailed email to my landlord about the leaking roof in the bedroom, the broken geyser, "
        "and the rent receipt for October that I still have not received")
ROUTED = ("mere tasks btao", "search my notes for plan", "jot down that the plants need water",
          "please could you make a note of the meeting", "kal doodh lana hai")
NEW = "start a new conversation"


def status(pid: int) -> dict:
    """Memory of a process in MiB, from /proc: its whole RSS, private part and mapped files."""
    text = pathlib.Path(f"/proc/{pid}/status").read_text()
    kib = lambda name: int(re.search(rf"^{name}:\s+(\d+) kB", text, re.M).group(1))
    return {"rss": kib("VmRSS") / 1024, "anon": kib("RssAnon") / 1024, "file": kib("RssFile") / 1024}


def cpu_seconds(pid: int) -> float:
    fields = pathlib.Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK")


def children(pid: int) -> list[int]:
    out = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout
    return [int(x) for x in out.split()]


def say(base: str, token: str, text: str) -> dict:
    """One request; the seconds to its first written word and to its end, and what came."""
    req = urllib.request.Request(f"{base}/api/say", data=json.dumps({"text": text}).encode(), method="POST",
                                 headers={"X-Kestrel-Token": token, "Content-Type": "application/json"})
    t0 = time.perf_counter()
    first, written, lines = None, "", []
    with urllib.request.urlopen(req, timeout=600) as r:
        for raw in r:
            msg = json.loads(raw)
            if "part" in msg:
                first = first if first is not None else time.perf_counter() - t0
                written += msg["part"]
            elif "line" in msg:
                lines.append(msg["line"])
            elif "ask" in msg:
                lines.append("(asked) " + msg["ask"])
    return {"first_s": first, "total_s": time.perf_counter() - t0, "chars": len(written), "lines": lines}


def verdict(value, target) -> str:
    if value is None:
        return "not measured"
    return "met" if value <= target else "MISSED"


def report(found: dict) -> None:
    """What a run measured, against §7. Routing's target is for rules and the encoder; a request that goes on
    to Gemma waits for it, so that is shown apart."""
    median = lambda runs: statistics.median(r["first_s"] for r in runs if r["first_s"] is not None)
    fast = [r["ms"] for r in found["routing_ms"] if r["end_tier"] < 4]
    tier4 = [r["ms"] for r in found["routing_ms"] if r["end_tier"] == 4]
    print(f"Service ready in {found['service_ready_s']:.1f} s; idle CPU {found['idle_cpu_percent']:.2f}% of a core.")
    fresh, within = median(found["warm_fresh"]), median(found["warm"])
    rows = [
        ("Routing, rules and encoder", max(fast) / 1000, 0.1, "s"),
        ("First words, warm, a new conversation", fresh, 2, "s"),
        ("First words, warm, in a conversation", within, 2, "s"),
        ("Worker ready (first words after a restart, less a new conversation's)",
         found["restart"]["first_s"] - fresh, 3, "s"),
        ("Mail draft", found["mail"]["total_s"] if found["mail"]["chars"] else None, 60, "s"),
        ("Idle memory", found["idle_memory_mib"]["rss"], 400, "MiB"),
    ]
    for name, value, target, unit in rows:
        shown = "?" if value is None else f"{value:.2f}"
        print(f"{name}: {shown} {unit} (target {target}): {verdict(value, target)}")
    print(f"Not in a target: a request routed on to Gemma {[round(ms / 1000, 2) for ms in tier4]} s (the first loads "
          f"the model); first words after a restart {found['restart']['first_s']:.1f} s.")
    print(f"Mail: {found['mail']['chars']} characters in {found['mail']['total_s']:.1f} s.")
    print(f"Unloaded {found['unloaded_after_s']} s after the last answer, with --idle-minutes 1.")
    worker = found["after_memory_mib"]["workers"]
    print("Memory, MiB: idle " + json.dumps(found["idle_memory_mib"]) + "; model worker " + json.dumps(worker)
          + "; after unload " + json.dumps(found["memory_after_unload_mib"]))
    for name in ("warm", "warm_fresh"):
        print(f"{name}: " + json.dumps([[round(r["first_s"], 2), round(r["total_s"], 2), r["chars"]] for r in found[name]]))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--kestrel", type=pathlib.Path, default=REPO / "target/release/kestrel")
    ap.add_argument("--idle-seconds", type=int, default=60)
    ap.add_argument("--out", type=pathlib.Path)
    ap.add_argument("--report", type=pathlib.Path, help="print what an earlier --out file measured, and stop")
    args = ap.parse_args()
    if args.report:
        report(json.loads(args.report.read_text()))
        return 0
    if not args.kestrel.is_file():
        sys.exit(f"{args.kestrel}: build it first (cargo build --release -p kestrel-cli --features llama)")

    root = pathlib.Path(tempfile.mkdtemp(prefix="kestrel-budgets-", dir=pathlib.Path.home() / ".cache"))
    run = pathlib.Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp")) / f"kb-{os.getpid()}"
    run.mkdir(mode=0o700)
    for d in ("data", "cache", "notes"):
        (root / d).mkdir(mode=0o700)
    (root / "notes/plan.md").write_text("# Plan\nMeeting on Friday. Ravi shares the plan.\n")
    telemetry = root / "telemetry.jsonl"
    env = dict(os.environ, XDG_RUNTIME_DIR=str(run), XDG_DATA_HOME=str(root / "data"),
               XDG_CACHE_HOME=str(root / "cache"), KESTREL_TELEMETRY=str(telemetry))
    for k in ("KESTREL_NOTES", "KESTREL_MEMORY", "KESTREL_WEB_LAN", "KESTREL_SLM"):
        env.pop(k, None)
    kestrel = lambda *a: subprocess.run([str(args.kestrel), *a], env=env, cwd=REPO, capture_output=True,
                                        text=True, timeout=60)
    kestrel("settings", "notes", str(root / "notes"))
    kestrel("settings", "name", "Budget")

    log = open(root / "serve.log", "w")

    def start(*extra):
        t0 = time.perf_counter()
        service = subprocess.Popen([str(args.kestrel), "serve", "--app", "--web-port", "0", *extra, CONFIG],
                                   cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
        while True:
            opened = kestrel("open", "--print", "--no-start")
            if opened.returncode == 0 and opened.stdout.strip():
                url = opened.stdout.strip()
                base = re.match(r"http://[^/]+", url).group(0)
                return service, base, re.search(r"#t=([0-9a-f]+)", url).group(1), time.perf_counter() - t0
            if service.poll() is not None or time.perf_counter() - t0 > 120:
                print((root / "serve.log").read_text()[-2000:], file=sys.stderr)
                sys.exit(1)
            time.sleep(0.2)

    def stop(service):
        service.terminate()
        try:
            service.wait(15)
        except subprocess.TimeoutExpired:
            service.kill()
        time.sleep(3)  # its worker goes when its stdin closes

    found: dict = {"binary": str(args.kestrel), "unix": int(time.time())}
    service = None
    try:
        service, base, token, found["service_ready_s"] = start()

        print(f"idle for {args.idle_seconds} s...", file=sys.stderr)
        time.sleep(5)
        cpu0, t1 = cpu_seconds(service.pid), time.perf_counter()
        time.sleep(args.idle_seconds)
        found["idle_cpu_percent"] = 100 * (cpu_seconds(service.pid) - cpu0) / (time.perf_counter() - t1)
        found["idle_memory_mib"] = status(service.pid)

        print("routing...", file=sys.stderr)
        for text in ROUTED:
            say(base, token, text)
        routed = [json.loads(line) for line in telemetry.read_text().splitlines()] if telemetry.exists() else []
        found["routing_ms"] = [{"end_tier": r["end_tier"], "ms": r["ms"]} for r in routed]

        print("the first answer: the model loads...", file=sys.stderr)
        found["cold"] = say(base, token, CHAT[0])
        workers = children(service.pid)
        print("warm answers...", file=sys.stderr)
        found["warm"] = [say(base, token, text) for text in CHAT[1:]]
        print("warm answers with no earlier turns...", file=sys.stderr)
        found["warm_fresh"] = []
        for text in CHAT[1:]:
            say(base, token, NEW)
            found["warm_fresh"].append(say(base, token, text))
        print("a mail...", file=sys.stderr)
        found["mail"] = say(base, token, MAIL)
        found["after_memory_mib"] = {"service": status(service.pid),
                                     "workers": [status(w) for w in children(service.pid)]}
        found["workers_seen"] = len(workers)
        stop(service)

        print("after a restart, its prefix on disk; then the model is left idle for a minute...", file=sys.stderr)
        service, base, token, _ = start("--idle-minutes", "1")
        found["restart"] = say(base, token, CHAT[1])
        t2, seen = time.perf_counter(), bool(children(service.pid))
        while children(service.pid) and time.perf_counter() - t2 < 180:
            time.sleep(1)
        found["unloaded_after_s"] = None if children(service.pid) else time.perf_counter() - t2
        found["worker_was_running"] = seen
        found["memory_after_unload_mib"] = status(service.pid)
    finally:
        if service is not None:
            stop(service)
        shutil.rmtree(run, ignore_errors=True)
        shutil.rmtree(root, ignore_errors=True)

    report(found)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(found, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
