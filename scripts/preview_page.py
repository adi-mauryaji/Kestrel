#!/usr/bin/env python3
"""Shows the v4 page (apps/cli/web/page.html, the human's design) in the browser with demo data,
without touching the installed service or your memory file.

A v4 `kestrel serve` runs from this repository with its own folders under ~/.cache/kestrel-preview
(memory, settings, socket), a few demo tasks and the name you give; Ctrl-C stops it.

    cargo build -p kestrel-cli --features llama     # the service, once
    python3 scripts/preview_page.py [--name Aditya | --welcome] [--no-open]
"""
import argparse
import os
import pathlib
import shutil
import sqlite3
import subprocess
import sys
import time

REPO = pathlib.Path(__file__).resolve().parent.parent
HOME = pathlib.Path.home() / ".cache" / "kestrel-preview"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--name", default="Aditya", help="the name the page greets")
    ap.add_argument("--welcome", action="store_true", help="no name saved: the first-open steps show")
    ap.add_argument("--no-open", action="store_true", help="print the address instead of opening it")
    args = ap.parse_args()

    builds = [REPO / "target" / b / "kestrel" for b in ("release", "debug")]
    builds = [b for b in builds if b.is_file()]
    if not builds:
        sys.exit("no kestrel build: cargo build -p kestrel-cli --features llama")
    kestrel = max(builds, key=lambda b: b.stat().st_mtime)

    shutil.rmtree(HOME, ignore_errors=True)
    # A Unix socket path must stay short: the runtime folder sits in this user's runtime dir.
    run = pathlib.Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp")) / "kestrel-preview"
    shutil.rmtree(run, ignore_errors=True)
    run.mkdir(mode=0o700)
    for d in ("data", "cache", "notes"):
        (HOME / d).mkdir(parents=True, mode=0o700)
    (HOME / "notes" / "2026-10-30-budget-plan.md").write_text("# Budget plan\nMeeting on Friday. Ravi shares the project plan for Q4.\n")
    (HOME / "notes" / "ideas.txt").write_text("A small desk lamp for the study.\n")
    env = dict(os.environ, XDG_RUNTIME_DIR=str(run), XDG_DATA_HOME=str(HOME / "data"),
               XDG_CACHE_HOME=str(HOME / "cache"))
    for k in ("KESTREL_NOTES", "KESTREL_MEMORY", "KESTREL_TELEMETRY", "KESTREL_WEB_LAN", "KESTREL_SLM"):
        env.pop(k, None)

    def kestrel_says(*a: str) -> subprocess.CompletedProcess:
        return subprocess.run([str(kestrel), *a], env=env, capture_output=True, text=True, timeout=60)

    kestrel_says("settings", "notes", str(HOME / "notes"))
    if not args.welcome:
        kestrel_says("settings", "name", args.name)
    log = open(HOME / "serve.log", "w")
    service = subprocess.Popen([str(kestrel), "serve", "--web-port", "0", "--app",
                                "experiments/configs/router_intents_v0.json"],
                               cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        end = time.time() + 90
        memory = HOME / "data" / "kestrel" / "memory.db"
        while not memory.exists() or kestrel_says("open", "--print", "--no-start").returncode:
            if service.poll() is not None or time.time() > end:
                print((HOME / "serve.log").read_text(), file=sys.stderr)
                return 1
            time.sleep(0.3)
        # Demo tasks: two later today, four without a date.
        now = time.localtime()
        today, hour = time.strftime("%Y-%m-%d", now), now.tm_hour
        at = lambda h: f"{min(h, 23):02d}:00"
        db = sqlite3.connect(memory)
        for title, due, time_ in [
            ("hiba ko call karna", today, at(hour + 1)),
            ("hiba ko call karna yaad dila dena", today, at(hour + 2)),
            ("send the q4 budget draft to finance", None, None),
            ("fix the flaky build", None, None),
            ("share the project plan with ravi", None, None),
            ("book the demo room", None, None),
        ]:
            db.execute("INSERT INTO tasks(title, due, due_time, created_at) VALUES(?, ?, ?, ?)",
                       (title, due, time_, int(time.time())))
        db.commit()
        db.close()
        url = kestrel_says("open", "--print", "--no-start").stdout.strip()
        if args.no_open:
            print(url, flush=True)
        else:
            subprocess.run(["xdg-open" if sys.platform.startswith("linux") else "open", url])
        print("The v4 page with demo data is open; Ctrl-C stops it.", file=sys.stderr)
        service.wait()
    except KeyboardInterrupt:
        pass
    finally:
        service.terminate()
        service.wait(10)
        shutil.rmtree(run, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
