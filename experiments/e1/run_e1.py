"""experiments/e1/run_e1.py -- E1 (H1, H1b, H1c): R0, R1 and R3 end to end under protocol v3 (ADR-020).

    cargo build --release -p kestrel-cli --features llama
    python experiments/e1/run_e1.py --dataset clinc150 --split val --limit 30 --check   # dev dry run
    python experiments/e1/run_e1.py --dataset clinc150 --split test --chunk 1           # after the freeze

One call = one chunk = one session (PREREG §4). The harness checks the machine (AC with the battery
full, boost 0, governor `performance`, no running containers), cools down (≥ 5 min, background
≤ 10%, CPU ≤ 50 °C), verifies the model, head and prompt hashes, then runs `kestrel bench-items` on
one CPU per physical core (`taskset`, ADR-011) with the three configs, R0 / R1 / R3, which must
agree on every model field. Each chunk starts with 5 warm-up requests per system (fixed train items).
Every 10 s it samples AC, boost, the pinned CPUs' frequency, the temperature and the busy share of
the CPUs outside the mask (background load while the run goes on); it stops the run on AC loss or
boost on. Afterwards the invalidation rules are applied (PREREG §4): rule 3 compares chunks with
each other, so it is applied in the analysis (`freq_problems`).

Items: the split's ids in a seeded order (key `e1-order`), cut into CHUNKS[dataset] chunks; the
latency subset (key `e1-latency`: 20% of CLINC150, all of Intents-v0) gets 5 timed repetitions,
every other item 1 (ADR-020). The test split is refused until the PREREG is frozen: the tag
`exp-E1-frozen` must be an ancestor of HEAD, the tree clean and no config DEV (`guard_test`).
With `--check` on val, every system's first-repetition decisions are compared with the
simulation of its config on the logged dev tier outputs (as check_runtime.py does for R3).

Out: experiments/logs/E1/<dataset>_<split>[_n<limit>]/chunk<k>of<n>_<UTC>/: rows.jsonl (one row per
item x system x repetition), rows.meta.json (systems, load time, peak RSS), samples.jsonl,
stderr.txt, manifest.json (environment, hashes, checks, problems, `valid`).
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
for sub in ("data", "analysis", "tier3", "tier4", "router", "measure"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from bench_e0 import (conditions, cpu_temp_c, cpu_times, busy_pct, mean_cpu_mhz, one_cpu_per_core,  # noqa: E402
                      read, read_topology)
from prep import seeded_order  # noqa: E402

KESTREL = ROOT / "target" / "release" / "kestrel"
CONFIGS = ROOT / "experiments" / "configs"
LOG_DIR = ROOT / "experiments" / "logs" / "E1"
SYSTEMS = ("_r0", "_r1", "")  # config suffixes: R0, R1, R3 (router_<dataset><suffix>.json)
ORDER_KEY, LATENCY_KEY, WARMUP_KEY = "e1-order", "e1-latency", "e1-warmup"
REPS = 5
N_WARMUP = 5
LATENCY_FRAC = {"clinc150": 0.2, "intents_v0": 1.0}  # ADR-020
CHUNKS = {"clinc150": 4, "intents_v0": 1}  # about one hour each at the dev estimates
SAMPLE_S = 10
WINDOW = 6  # samples: one minute
BUSY_MAX = 10.0  # % (rule 2)
TEMP_MAX = 95.0  # °C (rule 4)
FREQ_TOL = 0.10  # rule 3
START_TEMP = 50.0
FREEZE_TAG = "exp-E1-frozen"
ALLOWABLE = ("battery", "boost", "governor", "containers")


def plan(ids, dataset: str, chunks: int | None = None) -> list[list[dict]]:
    """The items in seeded order, cut into chunks; latency-subset items get REPS repetitions."""
    ids = sorted(ids)
    order = seeded_order(ids, ORDER_KEY)
    latency = set(seeded_order(ids, LATENCY_KEY)[: round(LATENCY_FRAC[dataset] * len(ids))])
    n = chunks or CHUNKS[dataset]
    bounds = [round(k * len(order) / n) for k in range(n + 1)]
    return [[{"id": i, "reps": REPS if i in latency else 1} for i in order[a:b]] for a, b in zip(bounds, bounds[1:])]


def warmup_items(train_rows) -> list[dict]:
    """N_WARMUP fixed train items (seeded order), run once per system before a chunk's items."""
    by_id = {r["id"]: r for r in train_rows}
    return [{"id": i, "text": by_id[i]["text"], "reps": 1, "warmup": True}
            for i in seeded_order(sorted(by_id), WARMUP_KEY)[:N_WARMUP]]


def per_cpu_times(stat: str) -> dict[int, tuple[int, int]]:
    """{cpu: (busy, total)} jiffies from the per-CPU lines of /proc/stat (idle + iowait = not busy)."""
    out = {}
    for line in stat.splitlines():
        name, *vals = line.split()
        if name.startswith("cpu") and name[3:].isdigit():
            f = [int(v) for v in vals]
            out[int(name[3:])] = (sum(f) - f[3] - f[4], sum(f))
    return out


def busy_outside(before: dict, after: dict, cpus) -> float:
    """Busy % of `cpus` between two per_cpu_times samples."""
    busy = sum(after[c][0] - before[c][0] for c in cpus)
    total = sum(after[c][1] - before[c][1] for c in cpus)
    return 100.0 * busy / total if total else 0.0


def stop_reason(state: dict, allow: set) -> str | None:
    """Why a running chunk must stop now (rule 1), or None."""
    if not state["ac_online"] and "battery" not in allow:
        return "AC power lost (ADR-012)"
    if state["boost"] != "0" and "boost" not in allow:
        return "turbo boost on (ADR-012)"
    return None


def window_problems(samples: list[dict]) -> list[str]:
    """Rules 2 and 4 over a chunk's samples: background busy over any 1-min window, temperature."""
    problems = []
    busy = [s["busy_outside_pct"] for s in samples]
    for i in range(max(1, len(busy) - WINDOW + 1)):
        w = busy[i:i + WINDOW]
        if w and sum(w) / len(w) > BUSY_MAX:
            problems.append(f"background busy {sum(w) / len(w):.1f}% > {BUSY_MAX}% over 1 min from "
                            f"t = {samples[i]['t']} s (rule 2)")
            break
    hot = [s for s in samples if s["temp_c"] is not None and s["temp_c"] > TEMP_MAX]
    if hot:
        problems.append(f"CPU {hot[0]['temp_c']} °C > {TEMP_MAX} °C at t = {hot[0]['t']} s (rule 4)")
    return problems


def freq_problems(chunk_mhz: dict[str, float]) -> dict[str, str]:
    """Rule 3 across one dataset's chunks: a chunk whose mean pinned-CPU frequency is > 10% from
    the median of all its chunks is invalid."""
    med = statistics.median(chunk_mhz.values())
    return {k: f"mean {v:.0f} MHz is {abs(v - med) / med:.1%} from the median {med:.0f} MHz (rule 3)"
            for k, v in chunk_mhz.items() if abs(v - med) / med > FREQ_TOL}


def rep_disagreements(rows) -> list[tuple[str, str]]:
    """(id, system) pairs whose repetitions decided differently (label or end tier)."""
    seen = defaultdict(set)
    for r in rows:
        if not r["warmup"]:
            seen[(r["id"], r["system"])].add((r["intent"], r["tier"]))
    return sorted(k for k, v in seen.items() if len(v) > 1)


def item_latency(rows) -> dict[tuple[str, str], float]:
    """The latency unit (PREREG §3): per (id, system), the median wall ms over its repetitions."""
    ms = defaultdict(list)
    for r in rows:
        if not r["warmup"]:
            ms[(r["id"], r["system"])].append(r["ms"])
    return {k: statistics.median(v) for k, v in ms.items()}


def _git(*args) -> tuple[int, str]:
    p = subprocess.run(["git", *args], capture_output=True, text=True, cwd=ROOT)
    return p.returncode, p.stdout


def guard_test(configs: list[dict], git=_git) -> None:
    """The test split runs only on a frozen PREREG (Ch. 34.6): frozen configs, the freeze tag in
    HEAD's history, a clean tree."""
    if not all(c["status"].startswith("FROZEN") for c in configs):
        raise RuntimeError("test split refused: a config is not FROZEN (the PREREG is not frozen)")
    if git("tag", "-l", FREEZE_TAG)[1].strip() != FREEZE_TAG:
        raise RuntimeError(f"test split refused: no {FREEZE_TAG} tag")
    if git("merge-base", "--is-ancestor", FREEZE_TAG, "HEAD")[0] != 0:
        raise RuntimeError(f"test split refused: {FREEZE_TAG} is not an ancestor of HEAD")
    if git("status", "--porcelain")[1].strip():
        raise RuntimeError("test split refused: the working tree is not clean")


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def hash_problems(cfg: dict) -> list[str]:
    """Rule 5: the files a config names must be the ones it was built from."""
    t3, t4 = cfg["models"]["tier3"], cfg["models"]["tier4"]
    manifest = {m.get("file"): m.get("sha256") for m in tomllib.loads((ROOT / "models" / "MANIFEST.toml").read_text())["model"]}
    checks = [(ROOT / "models" / t3["encoder_file"], t3["encoder_sha256"]),
              (ROOT / "experiments" / "artifacts" / "tier3" / t3["head"] / "head.bin", t3["head_bin_sha256"]),
              (ROOT / "datasets" / "contracts" / t4["prompt"], t4["prompt_sha256"]),
              (ROOT / "models" / t4["model_file"], manifest.get(t4["model_file"]))]
    return [f"hash mismatch: {p.relative_to(ROOT)} (rule 5)" for p, want in checks
            if not p.exists() or _sha256(p) != want]


def running_containers() -> list[str]:
    out = []
    for tool in ("docker", "podman"):
        if shutil.which(tool):
            try:
                p = subprocess.run([tool, "ps", "-q"], capture_output=True, text=True, timeout=30)
            except subprocess.TimeoutExpired:
                continue
            if p.returncode == 0:
                out += [f"{tool}:{c}" for c in p.stdout.split()]
    return out


def machine_problems(pre: dict, allow: set) -> list[str]:
    problems = []
    if not pre["ac_online"] and "battery" not in allow:
        problems.append("on battery: plug in (Ch. 37.3)")
    if pre["boost"] != "0" and "boost" not in allow:
        problems.append("turbo boost on: echo 0 | sudo tee /sys/devices/system/cpu/cpufreq/boost (ADR-012)")
    if pre["governor"] != "performance" and "governor" not in allow:
        problems.append(f"governor {pre['governor']}: sudo cpupower frequency-set -g performance")
    if (c := running_containers()) and "containers" not in allow:
        problems.append(f"containers running: {' '.join(c)}")
    return problems


def cooldown(seconds: float) -> dict:
    """Idle wait before a chunk: background busy over the wait, then until CPU ≤ START_TEMP (≤ 10 min)."""
    before = cpu_times()
    time.sleep(seconds)
    busy = busy_pct(before, cpu_times())
    waited = 0
    while (t := cpu_temp_c()) is not None and t > START_TEMP and waited < 600:
        time.sleep(10)
        waited += 10
    return {"seconds": seconds, "busy_pct": busy, "extra_wait_s": waited, "temp_c": cpu_temp_c()}


class Sampler(threading.Thread):
    """Every SAMPLE_S: AC, boost, pinned-CPU MHz, temperature, busy % outside the mask; calls
    `stop(reason)` once on a rule-1 violation."""

    def __init__(self, pinned, outside, allow, stop):
        super().__init__(daemon=True)
        self.pinned, self.outside, self.allow, self.stop = pinned, outside, allow, stop
        self.samples, self.done = [], threading.Event()
        self.t0, self.prev = time.monotonic(), per_cpu_times(read("/proc/stat"))

    def run(self):
        while not self.done.wait(SAMPLE_S):
            cur = per_cpu_times(read("/proc/stat"))
            c = conditions()
            s = {"t": round(time.monotonic() - self.t0, 1), "ac_online": c["ac_online"], "boost": c["boost"],
                 "temp_c": c["temp_c"], "mhz_pinned": mean_cpu_mhz(self.pinned),
                 "busy_outside_pct": busy_outside(self.prev, cur, self.outside)}
            self.prev = cur
            self.samples.append(s)
            if reason := stop_reason(s, self.allow):
                self.stop(reason)
                return


def load_items(dataset: str, split: str) -> list[dict]:
    if split == "val":
        from splits import load_splits
        return load_splits(dataset)["val"]
    # Only reached after guard_test: the frozen PREREG is the licence to read the test set.
    if dataset == "intents_v0":
        path = ROOT / "datasets" / "kestrel_intents_v0" / "splits" / "test.jsonl"
        return [json.loads(l) for l in path.open(encoding="utf-8")]
    from clinc150 import kestrel_splits, load_official
    return kestrel_splits(load_official())["test"]


def dev_check(dataset: str, rows: list[dict], configs: dict[str, dict]) -> dict:
    """Per system: first-repetition decisions vs the simulation of its config on the logged dev
    tier outputs (label and end tier must agree), as check_runtime.py does for R3."""
    import numpy as np
    from check_runtime import compare, sim_settings
    from policies import DATASETS, Answers, label_classes, read_tier_outputs, route

    recs, meta, _ = read_tier_outputs(dataset)
    by_id = {r["id"]: r for r in recs if r["split"] == "val"}
    classes = label_classes(meta, dataset)
    out = {}
    for name, cfg in configs.items():
        mine = [r for r in rows if r["system"] == name and r["rep"] == 1 and not r["warmup"]]
        val = [by_id[r["id"]] for r in mine]
        cal = cfg["calibration"]
        rule_conf = cal["tier1_confidence"] if cal["tier1_confidence"] is not None else 1.0
        a = Answers.build(val, classes, rule_conf=rule_conf, t3=cal["tier3_temperature"],
                          t4=cal["tier4_temperature"], oos_label=DATASETS[dataset][1])
        tau, tiers, signal = sim_settings(cfg)
        sim = route(a, tau, tiers=tiers, signal=signal)
        conf = np.select([sim.tier == 1, sim.tier == 3, sim.tier == 4], [a.c1, a.c3, a.c4], 0.0)
        res = compare([r["id"] for r in val], classes, sim.pred, sim.tier, conf, mine)
        out[name] = {k: res[k] for k in ("n", "agree", "mismatches", "conf_max_abs_diff")}
    return out


def environment() -> dict:
    run = lambda *cmd: subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT).stdout.strip()  # noqa: E731
    return {"git_head": run("git", "rev-parse", "HEAD"), "git_dirty": bool(run("git", "status", "--porcelain")),
            "rustc": run("rustc", "-V"), "cargo_lock_sha256": _sha256(ROOT / "Cargo.lock"),
            "kestrel_sha256": _sha256(KESTREL), "python": platform.python_version(), "kernel": platform.release(),
            "cpu": next((l.split(":", 1)[1].strip() for l in read("/proc/cpuinfo").splitlines()
                         if l.startswith("model name")), None)}


def run_chunk(args, allow: set) -> dict:
    from splits import load_splits

    configs = {}
    for suffix in SYSTEMS:
        path = CONFIGS / f"router_{args.dataset}{suffix}.json"
        cfg = json.loads(path.read_text())
        configs[cfg["policy"]] = cfg | {"_path": str(path.relative_to(ROOT))}
    if args.split == "test":
        guard_test(list(configs.values()))
    rows_all = load_items(args.dataset, args.split)
    if args.limit:
        keep = set(seeded_order(sorted(r["id"] for r in rows_all), ORDER_KEY)[: args.limit])
        rows_all = [r for r in rows_all if r["id"] in keep]
    chunks = plan([r["id"] for r in rows_all], args.dataset, 1 if args.limit else None)
    chunk = chunks[args.chunk - 1]
    text = {r["id"]: r["text"] for r in rows_all}
    items = warmup_items(load_splits(args.dataset)["train"]) + [it | {"text": text[it["id"]]} for it in chunk]

    cores = one_cpu_per_core(read_topology())
    outside = sorted(set(read_topology()) - set(cores))
    pre = conditions()
    # One config suffices: bench-items refuses configs that differ in any model field.
    problems = machine_problems(pre, allow) + hash_problems(next(iter(configs.values())))
    if problems:
        raise SystemExit("not starting:\n- " + "\n- ".join(problems))
    cool = cooldown(args.cooldown)
    if cool["busy_pct"] > BUSY_MAX:
        raise SystemExit(f"not starting: background busy {cool['busy_pct']:.1f}% during the cooldown (rule 2)")

    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    name = f"{args.dataset}_{args.split}" + (f"_n{args.limit}" if args.limit else "")
    out = LOG_DIR / name / f"chunk{args.chunk}of{len(chunks)}_{stamp}"
    out.mkdir(parents=True)
    stopped = []
    with tempfile.TemporaryDirectory() as d:
        items_path = Path(d) / "items.jsonl"
        items_path.write_text("".join(json.dumps(it, ensure_ascii=False) + "\n" for it in items))
        cmd = ["taskset", "-c", ",".join(map(str, cores)), str(KESTREL), "bench-items", str(items_path),
               str(out / "rows.jsonl"), *[c["_path"] for c in configs.values()]]
        t0 = time.monotonic()
        with (out / "stderr.txt").open("w") as err:
            proc = subprocess.Popen(cmd, cwd=ROOT, stderr=err)
            sampler = Sampler(cores, outside, allow, lambda why: (stopped.append(why), proc.terminate()))
            sampler.start()
            _, status, usage = os.wait4(proc.pid, 0)
        wall = time.monotonic() - t0
        sampler.done.set()
        sampler.join()
    code = os.waitstatus_to_exitcode(status)
    (out / "samples.jsonl").write_text("".join(json.dumps(s) + "\n" for s in sampler.samples))
    rows = [json.loads(l) for l in (out / "rows.jsonl").read_text().splitlines()] if (out / "rows.jsonl").exists() else []

    problems = stopped + window_problems(sampler.samples)
    if code != 0 and not stopped:
        problems.append(f"kestrel exited with {code} (rule 5)")
    mhz = [s["mhz_pinned"] for s in sampler.samples if s["mhz_pinned"] is not None]
    manifest = {
        "experiment": "E1", "dataset": args.dataset, "split": args.split, "limit": args.limit,
        "chunk": args.chunk, "chunks": len(chunks), "items": len(chunk),
        "latency_items": sum(it["reps"] == REPS for it in chunk), "warmup_items": [it["id"] for it in items[:N_WARMUP]],
        "item_ids": [it["id"] for it in chunk],
        "systems": {k: {"config": c["_path"], "config_sha256": c["config_sha256"], "status": c["status"]}
                    for k, c in configs.items()},
        "command": cmd[:5] + ["items.jsonl (temporary)"] + cmd[6:], "pinned_cpus": cores, "outside_cpus": outside,
        "allowed": sorted(allow), "pre": pre, "cooldown": cool, "wall_s": round(wall, 1), "exit_code": code,
        "child_maxrss_mib": usage.ru_maxrss / 1024,
        "rows": len(rows), "rep_disagreements": rep_disagreements(rows),
        "sampling": {"n": len(sampler.samples), "mhz_pinned_mean": statistics.mean(mhz) if mhz else None,
                     "temp_c_max": max((s["temp_c"] for s in sampler.samples if s["temp_c"] is not None),
                                       default=None),
                     "busy_outside_pct_max": max((s["busy_outside_pct"] for s in sampler.samples), default=None)},
        "problems": problems, "valid": not problems and not allow,
        "valid_note": "a run with any --allow is a dry run, never valid for E1",
        "environment": environment(),
    }
    if args.check:
        manifest["dev_check"] = dev_check(args.dataset, rows, configs) if args.split == "val" else None
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1, ensure_ascii=False) + "\n")
    manifest["_out"] = str(out.relative_to(ROOT))
    return manifest


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", required=True, choices=sorted(CHUNKS))
    ap.add_argument("--split", default="val", choices=["val", "test"])
    ap.add_argument("--chunk", type=int, default=1, help="1-based chunk of the dataset's plan")
    ap.add_argument("--limit", type=int, help="val only: the first N items of the seeded order, one chunk")
    ap.add_argument("--allow", nargs="*", default=[], choices=ALLOWABLE,
                    help="dry runs only: run despite these machine conditions (recorded; never valid)")
    ap.add_argument("--cooldown", type=float, default=300.0, help="seconds of idle before the chunk (PREREG: 300)")
    ap.add_argument("--check", action="store_true", help="val: compare decisions with the simulation")
    args = ap.parse_args(argv)
    if args.limit and args.split != "val":
        ap.error("--limit is for val dry runs only")
    if not KESTREL.exists():
        raise FileNotFoundError(f"{KESTREL}: cargo build --release -p kestrel-cli --features llama")
    m = run_chunk(args, set(args.allow))
    print(json.dumps({k: m[k] for k in ("_out", "items", "latency_items", "rows", "wall_s", "exit_code",
                                         "rep_disagreements", "sampling", "problems", "valid")}, indent=1))
    if "dev_check" in m:
        print(json.dumps({k: {x: v[x] for x in ("n", "agree", "conf_max_abs_diff")} for k, v in m["dev_check"].items()},
                         indent=1))


if __name__ == "__main__":
    main()
