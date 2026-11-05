#!/usr/bin/env python3
"""A first install from nothing, for real: the built rpm unpacked in a scratch folder with no model, the real
kestrel-net bringing the default model down from its real address as the page's Download button does, then a
question answered by the file that came down. It continues the download when it stops, as a person would.

    python3 experiments/v4/download_test.py OUT [--keep]

Costs the model's size in data (2.84 GB). Touches nothing of the real install. Linux only.
"""
import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request

REPO = pathlib.Path(__file__).resolve().parents[2]
RPM = REPO / "apps/desktop/target/release/bundle/rpm"
RESUMES = 20


def sha256_of(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("--keep", action="store_true", help="keep the scratch folder and its model")
    args = ap.parse_args()
    out = args.out.resolve()
    root = out / "root"
    shutil.rmtree(root, ignore_errors=True)
    (root / "unpacked").mkdir(parents=True)
    log_file = open(out / "download.log", "a", buffering=1)

    def say(line: str) -> None:
        stamped = f"{time.strftime('%H:%M:%S')} {line}"
        print(stamped, flush=True)
        log_file.write(stamped + "\n")

    rpm = next(RPM.glob("*.rpm"))
    unpack = subprocess.Popen(["rpm2cpio", str(rpm)], stdout=subprocess.PIPE)
    subprocess.run(["cpio", "-idm", "--quiet"], stdin=unpack.stdout, cwd=root / "unpacked", check=True)
    unpack.wait()
    usr = root / "unpacked/usr"
    kestrel, home = usr / "bin/kestrel", usr / "lib/Kestrel"
    catalog = json.loads((home / "models/catalog.json").read_text())
    model = next(m for m in catalog["models"] if m.get("url"))
    say(f"rpm {rpm.name}, built {time.ctime(rpm.stat().st_mtime)}; model {model['id']}; "
        f"model files in the install: {[p.name for p in (home / 'models').glob('gemma*')]}")

    run = pathlib.Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp")) / f"kd-{os.getpid()}"
    run.mkdir(mode=0o700)
    for d in ("data", "cache", "notes"):
        (root / d).mkdir(mode=0o700)
    env = dict(os.environ, XDG_RUNTIME_DIR=str(run), XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
               KESTREL_HOME=str(home))
    for k in ("KESTREL_NOTES", "KESTREL_MEMORY", "KESTREL_TELEMETRY", "KESTREL_WEB_LAN", "KESTREL_SLM"):
        env.pop(k, None)
    kestrel_says = lambda *a: subprocess.run([str(kestrel), *a], env=env, cwd=home, capture_output=True, text=True, timeout=60)
    kestrel_says("settings", "notes", str(root / "notes"))
    kestrel_says("settings", "name", "Test User")

    def start():
        log = open(out / "serve.log", "a")
        service = subprocess.Popen([str(kestrel), "serve", "--app", "--web-port", "0", "experiments/configs/router_intents_v0.json"],
                                   cwd=home, env=env, stdout=log, stderr=subprocess.STDOUT)
        end = time.time() + 90
        while True:
            opened = kestrel_says("open", "--print", "--no-start")
            if opened.returncode == 0 and opened.stdout.strip():
                url = opened.stdout.strip()
                return service, re.match(r"http://[^/]+", url).group(0), re.search(r"#t=([0-9a-f]+)", url).group(1)
            if service.poll() is not None or time.time() > end:
                sys.exit((out / "serve.log").read_text()[-1500:])
            time.sleep(0.3)

    def stop(service):
        service.terminate()
        service.wait(20)
        time.sleep(2)

    def api(base, token, path, body=None, timeout=60):
        req = urllib.request.Request(base + path, data=json.dumps(body).encode() if body is not None else None,
                                     method="POST" if body is not None else "GET",
                                     headers={"X-Kestrel-Token": token, "Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.read().decode()

    service = None
    code = 1
    try:
        service, base, token = start()
        cards = lambda: {c["id"]: c["state"] for c in json.loads(api(base, token, "/api/capabilities"))["cards"]}
        say(f"before: chat is {cards().get('chat')}")
        api(base, token, "/api/outbound", {"internet": True})
        api(base, token, "/api/model/download", {"id": model["id"]})
        t0, tries, last_done, last_print = time.time(), 1, 0, 0.0
        while True:
            time.sleep(5)
            state = json.loads(api(base, token, "/api/model/download"))
            kind = state["state"]
            if kind == "getting":
                done, whole = state.get("done", 0), state.get("all", 0) or 1
                if time.time() - last_print >= 60:
                    rate = (done - last_done) / max(time.time() - last_print, 1) / (1 << 20)
                    say(f"{done / (1 << 20):7.0f} of {whole / (1 << 20):.0f} MiB, {rate:5.1f} MiB/s")
                    last_done, last_print = done, time.time()
            elif kind == "checking":
                if last_print >= 0:
                    say("the whole file is here; its hash is being checked")
                    last_print = -1.0
            elif kind == "done":
                break
            elif kind in ("stopped", "failed"):
                say(f"it {kind}: {state.get('why', '')[:160]}")
                if tries >= RESUMES:
                    say(f"giving up after {tries} tries")
                    return 1
                tries += 1
                time.sleep(20)
                say(f"continuing, try {tries}")
                api(base, token, "/api/model/download", {"id": model["id"]})
                last_print = 0.0
        took = time.time() - t0
        got = root / "data/kestrel/models" / model["file"]
        say(f"done in {took / 60:.1f} min on {tries} tries; kept file {got.stat().st_size} bytes")
        same = sha256_of(got) == model["sha256"]
        say(f"SHA-256 is the catalog's: {same}")
        if (REPO / "models" / model["file"]).exists():
            say(f"same bytes as the file already here: {sha256_of(REPO / 'models' / model['file']) == sha256_of(got)}")
        left = json.loads(api(base, token, "/api/outbound"))["log"]
        say(f"in the log: {[(r['kind'], r['host'], r['result']) for r in left]}")
        say(f"after: chat is {cards().get('chat')}; left over: {[p.name for p in got.parent.glob('*.part')]}")

        stop(service)
        service, base, token = start()
        t1 = time.time()
        raw = api(base, token, "/api/say", {"text": "explain gravity in one line"}, timeout=300)
        lines = [json.loads(line) for line in raw.splitlines()]
        words = "".join(m.get("part", "") for m in lines) + " ".join(m.get("line", "") for m in lines)
        say(f"asked after a restart, {time.time() - t1:.1f} s: {words[:200]!r}")
        worker = subprocess.run(["pgrep", "-P", str(service.pid)], capture_output=True, text=True).stdout.split()
        for pid in worker:
            mapped = [l.split()[-1] for l in pathlib.Path(f"/proc/{pid}/maps").read_text().splitlines() if l.endswith(".gguf")]
            say(f"the model in use: {sorted(set(mapped))}")
        code = 0 if same else 1
        return code
    finally:
        if service is not None:
            service.terminate()
            try:
                service.wait(20)
            except subprocess.TimeoutExpired:
                service.kill()
        shutil.rmtree(run, ignore_errors=True)
        if not args.keep and code == 0:
            shutil.rmtree(root, ignore_errors=True)
        say(f"ended, code {code}")


if __name__ == "__main__":
    sys.exit(main())
