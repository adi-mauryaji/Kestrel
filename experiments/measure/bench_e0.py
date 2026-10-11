#!/usr/bin/env python3
"""E0 hardware benchmark (blueprint Ch. 34.2, 37, Week 2): llama.cpp CLI on this machine.

Per model in models/MANIFEST.toml (role slm/encoder, file present):
  * SLM:     llama-bench pp<P> (prefill) and tg<G> (decode) tok/s, r repetitions after warm-up
  * encoder: llama-bench -embd 1 pp<E> (one forward pass over E tokens)
  * startup: one process with -p 1 -n 1 --no-warmup -r 1 (process start + load + 1 token),
             measured with a WARM page cache (dropping caches needs root; recorded as such)
  * peak RSS of every llama-bench process (os.wait4), CPU temperature before/after,
    mean/min CPU frequency sampled while it runs (Ch. 37.3: log frequency AND temperature)
  * background CPU busy % measured during the cooldown before each process, i.e. while no
    benchmark runs (load average would include the previous benchmark's own load)

Models are interleaved per round (rotated order) to cancel drift (Ch. 34.5). One run of this
script = one session; run it in 3 separate sessions for the repeatability CV (Week-2 test).

Warm-up rounds (default 2) are recorded with phase=warmup and excluded from analysis. The
2026-10-11 pilot showed why: on this Ryzen 5500U, prefill tok/s fell 26-39% after the first
round (short boost window, then a lower sustained power limit) while decode stayed within 3%.
In session 1 one warm-up round was not enough (round 1 still ran ~4% faster). ADR-011.

Threads are pinned one per physical core (llama-bench -C <mask> --cpu-strict 1), the way the
Kestrel runtime should run them; --no-pin disables it. Unpinned, two threads can share one
core's SMT siblings: session 1's encoder latency was bimodal (~6.5 vs ~9.6 ms). ADR-011.

Preconditions (Ch. 37.3) are recorded in every row; on battery or with turbo boost on the script
refuses unless --allow-battery / --allow-boost, and then the rows say so. Boost is off by protocol
(ADR-012): with boost the CPU switches between a ~2.6 GHz and a ~2.0 GHz sustained regime
depending on the last tens of minutes of load, which moves all throughput by 20-35%. Energy is NOT measured here (Ch. 37.4).

Usage: .venv/bin/python experiments/measure/bench_e0.py [--rounds 3] [--no-pin] [--only NAME ...]
"""

import argparse
import csv
import json
import os
import platform
import subprocess
import sys
import threading
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


def cpu_times() -> tuple[int, int]:
    """(busy, total) jiffies from the aggregate `cpu` line of /proc/stat."""
    fields = [int(x) for x in read("/proc/stat").splitlines()[0].split()[1:]]
    idle = fields[3] + fields[4]  # idle + iowait
    return sum(fields) - idle, sum(fields)


def busy_pct(before: tuple[int, int], after: tuple[int, int]) -> float:
    """Percent of CPU time spent busy between two cpu_times() samples."""
    total = after[1] - before[1]
    return 100.0 * (after[0] - before[0]) / total if total else 0.0


def mean_cpu_mhz(cpus: list[int] | None = None) -> float | None:
    """Mean current frequency over the given logical CPUs (all CPUs if None)."""
    paths = ([Path(f"/sys/devices/system/cpu/cpu{c}/cpufreq/scaling_cur_freq") for c in cpus] if cpus
             else Path("/sys/devices/system/cpu").glob("cpu[0-9]*/cpufreq/scaling_cur_freq"))
    vals = [int(v) for v in (read(str(p)) for p in paths) if v]
    return sum(vals) / len(vals) / 1000 if vals else None


def freq_summary(samples: list[float]) -> tuple[float | None, float | None]:
    """(mean, min) of frequency samples in MHz; (None, None) if nothing was sampled."""
    return (sum(samples) / len(samples), min(samples)) if samples else (None, None)


def read_topology() -> dict[int, tuple[str, str]]:
    """Logical CPU -> (package id, core id) from sysfs."""
    topo = {}
    for d in Path("/sys/devices/system/cpu").glob("cpu[0-9]*"):
        core = read(f"{d}/topology/core_id")
        if core:
            topo[int(d.name[3:])] = (read(f"{d}/topology/physical_package_id"), core)
    return topo


def one_cpu_per_core(topology: dict[int, tuple[str, str]]) -> list[int]:
    """The lowest logical CPU of each physical core, so no two threads share a core's SMT siblings."""
    first: dict[tuple[str, str], int] = {}
    for cpu in sorted(topology):
        first.setdefault(topology[cpu], cpu)
    return sorted(first.values())


def cpu_mask(cpus: list[int]) -> str:
    """llama.cpp --cpu-mask value: hex bitmask of logical CPUs, e.g. [0, 2, 4] -> '15'."""
    return format(sum(1 << c for c in cpus), "x")


def protocol_problems(pre: dict, allow_battery: bool, allow_boost: bool) -> list[str]:
    """Ch. 37.3 preconditions that stop a session unless explicitly allowed (rows record them anyway)."""
    problems = []
    if not pre["ac_online"] and not allow_battery:
        problems.append("On battery: plug in (Ch. 37.3) or pass --allow-battery.")
    if pre["boost"] != "0" and not allow_boost:
        problems.append("Turbo boost is on: echo 0 | sudo tee /sys/devices/system/cpu/cpufreq/boost "
                        "(ADR-012) or pass --allow-boost.")
    return problems


def run_bench(args: list[str], pinned: list[int] | None = None):
    """Run llama-bench; return (json rows, wall s, peak RSS MiB, MHz samples over all CPUs,
    MHz samples over the pinned CPUs only) for that process. With pinning, the all-CPU mean
    includes idle SMT siblings and understates the working cores' clock."""
    samples: list[float] = []
    samples_pinned: list[float] = []
    done = threading.Event()

    def sample() -> None:
        while not done.wait(0.25):
            if (mhz := mean_cpu_mhz()) is not None:
                samples.append(mhz)
            if pinned and (mhz := mean_cpu_mhz(pinned)) is not None:
                samples_pinned.append(mhz)

    t0 = time.perf_counter()
    proc = subprocess.Popen([str(BENCH), *args, "-o", "json"],
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    sampler = threading.Thread(target=sample, daemon=True)
    sampler.start()
    stdout = proc.stdout.read()
    _, status, usage = os.wait4(proc.pid, 0)
    wall = time.perf_counter() - t0
    done.set()
    sampler.join()
    if os.waitstatus_to_exitcode(status) != 0:
        raise RuntimeError(f"llama-bench failed: {' '.join(args)}")
    return json.loads(stdout), wall, usage.ru_maxrss / 1024, samples, samples_pinned  # ru_maxrss: KiB


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rounds", type=int, default=3, help="measured rounds")
    ap.add_argument("--warmup-rounds", type=int, default=2, help="recorded as phase=warmup")
    ap.add_argument("--reps", type=int, default=5, help="llama-bench repetitions per test")
    ap.add_argument("--threads", type=int, help="default = physical cores")
    ap.add_argument("--no-pin", action="store_true", help="don't pin threads to one CPU per core")
    ap.add_argument("--prompt", type=int, default=128, help="prefill tokens (SLM)")
    ap.add_argument("--gen", type=int, default=64, help="decode tokens (SLM)")
    ap.add_argument("--embd-tokens", type=int, default=32, help="tokens per encoder pass")
    ap.add_argument("--cooldown", type=float, default=10.0, help="seconds between processes")
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--allow-battery", action="store_true")
    ap.add_argument("--allow-boost", action="store_true", help="run with turbo boost on (not protocol)")
    args = ap.parse_args()
    cores = one_cpu_per_core(read_topology())
    args.threads = args.threads or len(cores)
    if not args.no_pin and args.threads > len(cores):
        sys.exit(f"--threads {args.threads} > {len(cores)} physical cores: can't pin one per core")
    pinned = None if args.no_pin else cores[: args.threads]
    mask = None if pinned is None else cpu_mask(pinned)

    pre = conditions()
    problems = protocol_problems(pre, args.allow_battery, args.allow_boost)
    if problems:
        sys.exit("\n".join(problems))

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
        "warmup_rounds": args.warmup_rounds, "cpu_mask": mask, "pinned": mask is not None,
        "page_cache": "warm (not dropped; needs root)", "energy": "NOT measured (Ch. 37.4)",
        "pre_conditions": pre, "args": vars(args),
    }, indent=2))

    fields = ["session", "phase", "round", "model", "role", "quant", "sha256", "test", "n_tokens",
              "threads", "reps", "avg_tok_s", "stddev_tok_s", "median_ms", "wall_s",
              "peak_rss_mib", "temp_before_c", "temp_after_c", "freq_mean_mhz", "freq_min_mhz",
              "freq_pinned_mean_mhz", "freq_pinned_min_mhz",
              "bg_busy_pct", "ac_online", "governor", "boost", "load1", "llama_commit", "cpu_mask"]
    out_csv = OUT / f"{session}.csv"
    with out_csv.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        for rnd in range(args.warmup_rounds + args.rounds):
            phase = "warmup" if rnd < args.warmup_rounds else "measure"
            order = models[rnd % len(models):] + models[:rnd % len(models)]
            for m in order:
                path = str(ROOT / "models" / m["file"])
                common = ["-m", path, "-t", str(args.threads)]
                if mask:
                    common += ["-C", mask, "--cpu-strict", "1"]
                if m["role"] == "slm":
                    tests = [("throughput", common + ["-p", str(args.prompt), "-n", str(args.gen),
                                                      "-r", str(args.reps)])]
                else:
                    tests = [("throughput", common + ["-embd", "1", "-p", str(args.embd_tokens),
                                                      "-n", "0", "-r", str(args.reps)])]
                tests.append(("startup", common + ["-p", "1", "-n", "0", "-r", "1", "--no-warmup"]
                              + (["-embd", "1"] if m["role"] == "encoder" else [])))
                for kind, cli in tests:
                    # The cooldown doubles as the background-load window: nothing of ours runs.
                    before = cpu_times()
                    time.sleep(args.cooldown)
                    bg = busy_pct(before, cpu_times())
                    c = conditions()
                    problems = protocol_problems(c, args.allow_battery, args.allow_boost)
                    if problems:  # e.g. charger unplugged mid-session: stop, don't mix regimes
                        sys.exit(f"Session stopped before {m['name']} {kind}, round {rnd}: "
                                 + " ".join(problems) + f" Partial CSV: {out_csv}")
                    rows, wall, rss, mhz, mhz_pinned = run_bench(cli, pinned)
                    t_after = cpu_temp_c()
                    f_mean, f_min = freq_summary(mhz)
                    fp_mean, fp_min = freq_summary(mhz_pinned)
                    for r in rows:
                        test = (f"pp{r['n_prompt']}" if r["n_gen"] == 0 else f"tg{r['n_gen']}")
                        if kind == "startup":
                            test = "startup"
                        samples = sorted(r["samples_ns"])
                        w.writerow({
                            "session": session, "phase": phase, "round": rnd, "model": m["name"],
                            "role": m["role"], "quant": m["quant"], "sha256": m["sha256"],
                            "test": test, "n_tokens": r["n_prompt"] or r["n_gen"],
                            "threads": args.threads, "reps": len(samples),
                            "avg_tok_s": round(r["avg_ts"], 3),
                            "stddev_tok_s": round(r["stddev_ts"], 3),
                            "median_ms": round(samples[len(samples) // 2] / 1e6, 3),
                            "wall_s": round(wall, 3), "peak_rss_mib": round(rss, 1),
                            "temp_before_c": c["temp_c"], "temp_after_c": t_after,
                            "freq_mean_mhz": None if f_mean is None else round(f_mean),
                            "freq_min_mhz": None if f_min is None else round(f_min),
                            "freq_pinned_mean_mhz": None if fp_mean is None else round(fp_mean),
                            "freq_pinned_min_mhz": None if fp_min is None else round(fp_min),
                            "bg_busy_pct": round(bg, 1),
                            "ac_online": c["ac_online"], "governor": c["governor"],
                            "boost": c["boost"], "load1": round(c["load1"], 2),
                            "llama_commit": commit[:9], "cpu_mask": mask or "",
                        })
                    f.flush()
                    print(f"{phase} round {rnd} {m['name']:28s} {kind:10s} wall={wall:6.1f}s "
                          f"rss={rss:7.1f}MiB temp={c['temp_c']}->{t_after} "
                          f"mhz={f_mean and round(f_mean)} bg={bg:.1f}%", flush=True)
    print(f"wrote {out_csv.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
