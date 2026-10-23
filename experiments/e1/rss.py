"""experiments/e1/rss.py -- peak RSS per system (PREREG §4, D12): one short process per config.

    python experiments/e1/rss.py --dataset clinc150
    python experiments/e1/rss.py --dataset intents_v0 --systems R1    # some systems only

An E1 chunk loads every system's models in one process, so its VmHWM is the whole stack. Here each
config runs alone in `kestrel bench-items` on E1's 5 warm-up train items (no val or test item is
read), so only that system's models load (without mmap, D12) and VmHWM after the items is its peak
RSS, with RssAnon and RssFile next to it. Pinned to one CPU per physical core, as in E1. Memory does
not depend on power or clock settings, so E1's timing checks do not apply; the model, head and prompt
hashes are verified first (rule 5). analyze_e1 takes the latest summary whose config hashes match
the chunks' configs.

Out: experiments/logs/E1/rss/<dataset>_<UTC>/: <system>.meta.json and .stderr.txt from bench-items,
summary.json.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from run_e1 import (CHUNKS, CONFIGS, KESTREL, LOG_DIR, ROOT, SYSTEMS, environment, hash_problems,  # noqa: E402
                    one_cpu_per_core, read_topology, warmup_items)

RSS_DIR = LOG_DIR / "rss"
KEPT = ("config_sha256", "status")
MEASURED = ("peak_rss_mib", "rss_anon_mib_at_end", "rss_file_mib_at_end", "load_s", "tiers_loaded")


def configs(dataset: str) -> dict[str, dict]:
    """policy -> config (with its repository path), for R0, R1, R3."""
    out = {}
    for suffix in SYSTEMS:
        path = CONFIGS / f"router_{dataset}{suffix}.json"
        cfg = json.loads(path.read_text())
        out[cfg["policy"]] = cfg | {"_path": str(path.relative_to(ROOT))}
    return out


def command(cores, items_path, out_path, config_path) -> list[str]:
    return ["taskset", "-c", ",".join(map(str, cores)), str(KESTREL), "bench-items", str(items_path), str(out_path),
            str(config_path)]


def summarize(dataset: str, metas: dict[str, dict], cfgs: dict[str, dict]) -> dict:
    """Per system: its config and what its process measured; refuses a process that ran other configs."""
    systems = {}
    for policy, meta in metas.items():
        want = [{"policy": policy, "config_sha256": cfgs[policy]["config_sha256"]}]
        if meta["systems"] != want:
            raise ValueError(f"{policy}: the process ran {meta['systems']}, expected {want}")
        if meta.get("peak_rss_mib") is None:
            raise ValueError(f"{policy}: no peak RSS in the bench-items meta")
        systems[policy] = {k: cfgs[policy][k] for k in KEPT} | {k: meta[k] for k in MEASURED}
    return {"experiment": "E1 peak RSS per system", "dataset": dataset, "systems": systems}


def main(argv=None):
    from splits import load_splits

    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", required=True, choices=sorted(CHUNKS))
    ap.add_argument("--systems", nargs="*", default=["R0", "R1", "R3"], choices=["R0", "R1", "R3"])
    args = ap.parse_args(argv)
    cfgs = configs(args.dataset)
    problems = hash_problems(next(iter(cfgs.values())))  # the three configs share every model field
    if problems:
        raise SystemExit("not starting:\n- " + "\n- ".join(problems))
    cores = one_cpu_per_core(read_topology())
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out = RSS_DIR / f"{args.dataset}_{stamp}"
    out.mkdir(parents=True)
    items = warmup_items(load_splits(args.dataset)["train"])
    metas = {}
    with tempfile.TemporaryDirectory() as d:
        items_path = Path(d) / "items.jsonl"
        items_path.write_text("".join(json.dumps(it, ensure_ascii=False) + "\n" for it in items))
        for policy in args.systems:
            rows = Path(d) / f"{policy}.jsonl"
            with (out / f"{policy}.stderr.txt").open("w") as err:
                code = subprocess.run(command(cores, items_path, rows, cfgs[policy]["_path"]), cwd=ROOT,
                                      stderr=err).returncode
            if code != 0:
                raise SystemExit(f"{policy}: kestrel exited with {code}; see {out / (policy + '.stderr.txt')}")
            metas[policy] = json.loads(rows.with_suffix(".meta.json").read_text())
            (out / f"{policy}.meta.json").write_text(json.dumps(metas[policy], indent=1) + "\n")
            print(f"{policy}: peak RSS {metas[policy]['peak_rss_mib']:.0f} MiB, tiers {metas[policy]['tiers_loaded']}")
    summary = summarize(args.dataset, metas, cfgs) | {"pinned_cpus": cores, "items": [it["id"] for it in items],
                                                      "environment": environment()}
    (out / "summary.json").write_text(json.dumps(summary, indent=1, ensure_ascii=False) + "\n")
    print(f"-> {(out / 'summary.json').relative_to(ROOT)}")


if __name__ == "__main__":
    main()
