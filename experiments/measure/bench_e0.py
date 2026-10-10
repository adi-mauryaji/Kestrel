#!/usr/bin/env python3
"""E0 hardware benchmark (blueprint Ch. 34.2, 37, Week 2): llama.cpp CLI on this machine.

Per model in models/MANIFEST.toml (role slm/encoder, file present):
  * SLM:     llama-bench pp<P> (prefill) and tg<G> (decode) tok/s, r repetitions after warm-up
  * encoder: llama-bench -embd 1 pp<E> (one forward pass over E tokens)
  * startup: one process with -p 1 -n 1 --no-warmup -r 1 (process start + load + 1 token),
             measured with a WARM page cache (dropping caches needs root; recorded as such)
  * peak RSS of every llama-bench process (os.wait4), CPU temperature before/after

Models are interleaved per round (rotated order) to cancel drift (Ch. 34.5). One run of this
script = one session; run it in 3 separate sessions for the repeatability CV (Week-2 test).

Preconditions (Ch. 37.3) are recorded in every row; on battery the script refuses unless
--allow-battery, and then the rows say so. Energy is NOT measured here (Ch. 37.4).

Usage: .venv/bin/python experiments/measure/bench_e0.py [--rounds 3] [--threads 6] [--only NAME ...]
"""

import argparse
import csv
import json
import os
import platform
import subprocess
import sys
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BENCH = ROOT / "third_party/llama.cpp/build/bin/llama-bench"
OUT = ROOT / "experiments/logs/E0"
LLAMA_SRC = ROOT / "third_party/llama.cpp"


def read(path: str, default: str = "") -> str:
    try:
        return Path(path).read_text().strip()
    except OSError:
        return default


def cpu_temp_c() -> float | None:
    for h in Path("/sys/class/hwmon").glob("hwmon*"):
        if read(f"{h}/name") in ("k10temp", "coretemp"):
            raw = read(f"{h}/temp1_input")
            return int(raw) / 1000 if raw else None
    return None


def conditions() -> dict:
    return {
        "ac_online": read("/sys/class/power_supply/AC/online") == "1",
        "battery_pct": read("/sys/class/power_supply/BAT0/capacity"),
        "governor": read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        "boost": read("/sys/devices/system/cpu/cpufreq/boost"),
        "load1": os.getloadavg()[0],
        "temp_c": cpu_temp_c(),
    }


def run_bench(args: list[str]) -> tuple[list[dict], float, float]:
    """Run llama-bench; return (json rows, wall seconds, peak RSS MiB) for that process."""
    t0 = time.perf_counter()
    proc = subprocess.Popen([str(BENCH), *args, "-o", "json"],
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    stdout = proc.stdout.read()
    _, status, usage = os.wait4(proc.pid, 0)
    wall = time.perf_counter() - t0
    if os.waitstatus_to_exitcode(status) != 0:
        raise RuntimeError(f"llama-bench failed: {' '.join(args)}")
    return json.loads(stdout), wall, usage.ru_maxrss / 1024  # ru_maxrss is KiB on Linux


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--reps", type=int, default=5, help="llama-bench repetitions per test")
    ap.add_argument("--threads", type=int, default=6, help="default = physical cores")
    ap.add_argument("--prompt", type=int, default=128, help="prefill tokens (SLM)")
    ap.add_argument("--gen", type=int, default=64, help="decode tokens (SLM)")
    ap.add_argument("--embd-tokens", type=int, default=32, help="tokens per encoder pass")
    ap.add_argument("--cooldown", type=float, default=10.0, help="seconds between processes")
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--allow-battery", action="store_true")
    args = ap.parse_args()

    pre = conditions()
    if not pre["ac_online"] and not args.allow_battery:
        sys.exit("On battery: plug in (Ch. 37.3) or pass --allow-battery (rows will be flagged).")

    manifest = tomllib.loads((ROOT / "models/MANIFEST.toml").read_text())
    models = [m for m in manifest["model"]
              if m["role"] in ("slm", "encoder") and "file" in m
              and (ROOT / "models" / m["file"]).exists()
              and (not args.only or m["name"] in args.only)]
    if not models:
        sys.exit("No model files found; run scripts/fetch_models.py first.")

    session = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    commit = subprocess.run(["git", "-C", str(LLAMA_SRC), "rev-parse", "HEAD"],
                            capture_output=True, text=True).stdout.strip()
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{session}.env.json").write_text(json.dumps({
        "session": session, "llama_cpp_commit": commit, "kernel": platform.release(),
        "threads": args.threads, "reps": args.reps, "rounds": args.rounds,
        "page_cache": "warm (not dropped; needs root)", "energy": "NOT measured (Ch. 37.4)",
        "pre_conditions": pre, "args": vars(args),
    }, indent=2))

    fields = ["session", "round", "model", "role", "quant", "sha256", "test", "n_tokens",
              "threads", "reps", "avg_tok_s", "stddev_tok_s", "median_ms", "wall_s",
              "peak_rss_mib", "temp_before_c", "temp_after_c", "ac_online", "governor",
              "boost", "load1", "llama_commit"]
    out_csv = OUT / f"{session}.csv"
    with out_csv.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        for rnd in range(args.rounds):
            order = models[rnd % len(models):] + models[:rnd % len(models)]
            for m in order:
                path = str(ROOT / "models" / m["file"])
                common = ["-m", path, "-t", str(args.threads)]
                if m["role"] == "slm":
                    tests = [("throughput", common + ["-p", str(args.prompt), "-n", str(args.gen),
                                                      "-r", str(args.reps)])]
                else:
                    tests = [("throughput", common + ["-embd", "1", "-p", str(args.embd_tokens),
                                                      "-n", "0", "-r", str(args.reps)])]
                tests.append(("startup", common + ["-p", "1", "-n", "0", "-r", "1", "--no-warmup"]
                              + (["-embd", "1"] if m["role"] == "encoder" else [])))
                for kind, cli in tests:
                    c = conditions()
                    rows, wall, rss = run_bench(cli)
                    t_after = cpu_temp_c()
                    for r in rows:
                        test = (f"pp{r['n_prompt']}" if r["n_gen"] == 0 else f"tg{r['n_gen']}")
                        if kind == "startup":
                            test = "startup"
                        samples = sorted(r["samples_ns"])
                        w.writerow({
                            "session": session, "round": rnd, "model": m["name"],
                            "role": m["role"], "quant": m["quant"], "sha256": m["sha256"],
                            "test": test, "n_tokens": r["n_prompt"] or r["n_gen"],
                            "threads": args.threads, "reps": len(samples),
                            "avg_tok_s": round(r["avg_ts"], 3),
                            "stddev_tok_s": round(r["stddev_ts"], 3),
                            "median_ms": round(samples[len(samples) // 2] / 1e6, 3),
                            "wall_s": round(wall, 3), "peak_rss_mib": round(rss, 1),
                            "temp_before_c": c["temp_c"], "temp_after_c": t_after,
                            "ac_online": c["ac_online"], "governor": c["governor"],
                            "boost": c["boost"], "load1": round(c["load1"], 2),
                            "llama_commit": commit[:9],
                        })
                    f.flush()
                    print(f"round {rnd} {m['name']:28s} {kind:10s} wall={wall:6.1f}s "
                          f"rss={rss:7.1f}MiB temp={c['temp_c']}->{t_after}", flush=True)
                    time.sleep(args.cooldown)
    print(f"wrote {out_csv.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
