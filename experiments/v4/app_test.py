#!/usr/bin/env python3
"""The whole app in a real browser, on made-up data: a release `kestrel` and the real `kestrel-net` from a scratch
folder, real Gemma from models/, and a Chrome window on the screen. It says a scenario's requests and writes each
reply to OUT/results.jsonl; reading them is the test (page_steps/conversation.mjs has the scenario's form).

    python3 experiments/v4/app_test.py SCENARIO.json OUT [--headless] [--welcome]

The memory file, notes and settings live in OUT/root and last between runs with the same OUT, so a later scenario
continues an earlier one. It never touches the real ones. No mail account is set, so no mail can leave. A stand-in
search provider runs on {SEARX}, which a scenario's text may name. Stops everything it started.
"""
import argparse
import json
import os
import pathlib
import shutil
import socket
import subprocess
import sys
import time
import urllib.request

REPO = pathlib.Path(__file__).resolve().parents[2]
CONFIG = "experiments/configs/router_intents_v0.json"
NOTES = {
    "2026-10-30-budget-plan.md": "# Budget plan\nMeeting on Friday. Ravi shares the project plan for Q4. The Q4 budget is 40 lakh.\n",
    "ideas.txt": "A small desk lamp for the study.\n",
    "passport.txt": "Passport No. Z1234567\nDate of Expiry 11/03/2029\n",
    "e-pan.txt": "Permanent Account Number ABCDE1234F\n",
    "todo.md": "renew my passport\n",
}


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("scenario", type=pathlib.Path)
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("--headless", action="store_true")
    ap.add_argument("--welcome", action="store_true", help="no name saved: the first-open steps show")
    args = ap.parse_args()
    out, root = args.out.resolve(), args.out.resolve() / "root"
    fresh = not root.exists()
    run = pathlib.Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp")) / f"kt-{os.getpid()}"
    run.mkdir(mode=0o700)
    (root / "bin").mkdir(parents=True, exist_ok=True)
    for d in ("data", "cache", "notes", "Downloads"):
        (root / d).mkdir(mode=0o700, exist_ok=True)
    for name in ("kestrel", "kestrel-net"):
        built = REPO / "target/release" / name
        if not built.is_file():
            sys.exit(f"{built}: build it first (scripts/build_desktop.sh builds both)")
        shutil.copy2(built, root / "bin" / name)
    env = dict(os.environ, XDG_RUNTIME_DIR=str(run), XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"))
    for k in ("KESTREL_NOTES", "KESTREL_MEMORY", "KESTREL_TELEMETRY", "KESTREL_WEB_LAN", "KESTREL_SLM"):
        env.pop(k, None)
    kestrel = lambda *a: subprocess.run([str(root / "bin/kestrel"), *a], env=env, cwd=REPO, capture_output=True, text=True, timeout=60)
    chrome = "google-chrome"
    if fresh:
        for name, text in NOTES.items():
            (root / "notes" / name).write_text(text)
        for ticket in (REPO / "experiments/v4/tickets").glob("*.html"):
            subprocess.run([chrome, "--headless=new", "--no-pdf-header-footer", f"--user-data-dir={root}/print",
                            f"--print-to-pdf={root}/Downloads/{ticket.stem}.pdf", ticket.as_uri()],
                           capture_output=True, timeout=120)
        kestrel("settings", "notes", str(root / "notes"))
        if not args.welcome:
            kestrel("settings", "name", "Test User")

    started = []
    try:
        searx_port = free_port()
        started.append(subprocess.Popen([sys.executable, "-I", str(REPO / "experiments/v4/fake_searxng.py"), str(searx_port)],
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        log = open(out / "serve.log", "a")
        service = subprocess.Popen([str(root / "bin/kestrel"), "serve", "--app", "--web-port", "0", CONFIG], cwd=REPO, env=env,
                                   stdout=log, stderr=subprocess.STDOUT)
        started.append(service)
        end = time.time() + 120
        while True:
            opened = kestrel("open", "--print", "--no-start")
            if opened.returncode == 0 and opened.stdout.strip():
                break
            if service.poll() is not None or time.time() > end:
                print((out / "serve.log").read_text()[-2000:], file=sys.stderr)
                return 1
            time.sleep(0.3)
        (out / "url.txt").write_text(opened.stdout.strip())

        port = free_port()
        flags = ["--headless=new"] if args.headless else ["--new-window", "--window-size=1440,900"]
        started.append(subprocess.Popen([chrome, *flags, f"--remote-debugging-port={port}", f"--user-data-dir={root}/chrome",
                                         "--no-first-run", "--no-default-browser-check", "about:blank"],
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        for _ in range(60):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{port}/json/version", timeout=1)
                break
            except OSError:
                time.sleep(0.5)

        text = args.scenario.read_text().replace("{SEARX}", f"http://127.0.0.1:{searx_port}").replace("{DOWNLOADS}", str(root / "Downloads"))
        (out / "scenario.now.json").write_text(text)
        return subprocess.run(["node", str(REPO / "experiments/v4/page_check.mjs"), str(port), str(out / "url.txt"),
                               str(REPO / "experiments/v4/page_steps/conversation.mjs")],
                              env=dict(os.environ, SCENARIO=str(out / "scenario.now.json"), RESULTS=str(out / "results.jsonl"), OUT=str(out))).returncode
    finally:
        for p in reversed(started):
            p.terminate()
        for p in reversed(started):
            try:
                p.wait(15)
            except subprocess.TimeoutExpired:
                p.kill()
        shutil.rmtree(run, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
