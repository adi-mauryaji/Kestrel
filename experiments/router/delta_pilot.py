"""experiments/router/delta_pilot.py -- the Week-8 pilot that sets the non-inferiority margin δ (ADR-020).

    cargo build --release -p kestrel-inference --features llama --example classify_items
    python experiments/router/delta_pilot.py --dataset intents_v0   # ~1 min per seed
    python experiments/router/delta_pilot.py --dataset clinc150     # ~15 min per seed (boost on)
    python experiments/router/delta_pilot.py --summary              # δ from the seeds logged so far

δ = max(2 × the seed-to-seed SD of tuned-R0 accuracy, the smallest effect of interest 0.02), from
CLINC150, the primary dataset; Intents-v0's SD is reported next to it (Ch. 34.1, PREREG §5).

R0 decodes greedily, so a "seed" cannot be a sampling seed. It draws R0's few-shot examples from
train instead (`select_shots` with a seed): the one arbitrary choice inside R0. Item-sampling noise
is left to the test's bootstrap CI. Pilot items = a fixed 25% of cal and of val (seeded hash order).
Per seed, Tier 4 runs on the pilot items with the logged prompt, model and context; T is fitted on
the pilot cal items (BCE) and τ chosen on the pilot val items (`tuned_single`), as for the deployed
R0. The other tiers' outputs are the logged ones; R0 does not read them. The deployed shots' scores
on the same items come from the logged outputs and are a reference, not a seed.

Accuracy only, so the laptop may run with boost on. Seeds already logged are not rerun.
Out: experiments/logs/router/delta_pilot/<dataset>_seed<s>.jsonl.gz (+ .meta.json) and
experiments/logs/router/delta_pilot.json.
"""
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
for sub in ("data", "analysis", "tier3", "tier4"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from b3_dev import CONTRACTS, TASKS, classify, fit_conf_temperature, model_path, select_shots  # noqa: E402
from policies import (DATASETS, LOG_DIR, OFF, Answers, correct, label_classes, read_tier_outputs,  # noqa: E402
                      route, tuned_single)
from prep import seeded_order  # noqa: E402

PILOT_FRAC = 0.25  # Ch. 34.1: 20-30% of the development data
PILOT_KEY = "delta-pilot"
SEEDS = (1, 2, 3, 4, 5)
SESOI = 0.02  # smallest effect of interest (ADR-020; to be confirmed with the mentor)
PRIMARY = "clinc150"  # D8
PILOT_DIR = LOG_DIR / "delta_pilot"
SUMMARY = LOG_DIR / "delta_pilot.json"


def pilot_records(recs, frac: float = PILOT_FRAC) -> list[dict]:
    """A fixed share of each dev split (seeded hash order of the ids), in the original order."""
    keep = set()
    for split in ("cal", "val"):
        ids = [r["id"] for r in recs if r["split"] == split]
        keep |= {(split, i) for i in seeded_order(ids, PILOT_KEY)[: round(frac * len(ids))]}
    return [r for r in recs if (r["split"], r["id"]) in keep]


def with_t4(recs, t4_rows) -> list[dict]:
    """Copies of `recs` carrying a seed's Tier-4 outputs (classify rows, in the same order)."""
    if [o["id"] for o in t4_rows] != [r["id"] for r in recs]:
        raise ValueError("Tier-4 outputs are not aligned with the pilot records")
    return [{**r, "t4": {k: v for k, v in o.items() if k != "id"}} for r, o in zip(recs, t4_rows)]


def r0_scores(recs, classes, oos_label: str) -> dict:
    """R0 built as the deployed one is: T by BCE on the cal records, one abstention τ on the val
    records (ADR-019), scored on val by PREREG §3. The untuned R0 always answers."""
    cal = [r for r in recs if r["split"] == "cal" and not r["compound"] and "steps" in r["t4"]]
    try:
        t = fit_conf_temperature([r["t4"]["steps"] for r in cal], [r["t4"]["label"] == r["gold"] for r in cal])
        note = "fitted on the pilot cal items (BCE)"
    except ValueError as e:
        t, note = 1.0, f"NOT FITTED ({e}); T = 1 kept"
    val = [r for r in recs if r["split"] == "val"]
    a = Answers.build(val, classes, rule_conf=1.0, t3=1.0, t4=t, oos_label=oos_label)
    untuned = route(a, (OFF, OFF, 0.0), tiers=(4,))
    tuned = tuned_single(a, 4)
    return {"n_cal": len(cal), "n_val": len(val), "temperature": t, "temperature_note": note,
            "untuned_acc": float(correct(a, untuned.pred, untuned.tier).mean()),
            "tuned_acc": tuned["acc"], "tuned_coverage": tuned["coverage"], "tuned_tau": tuned["tau"][2]}


def delta(accs, sesoi: float = SESOI) -> dict:
    """δ = max(2 × sample SD across seeds, SESOI) (Ch. 34.1); the blueprint asks for >= 3 seeds."""
    if len(accs) < 3:
        raise ValueError(f"δ needs >= 3 seeds, got {len(accs)}")
    sd = float(np.std(accs, ddof=1))
    return {"n_seeds": len(accs), "mean": float(np.mean(accs)), "sd": sd, "two_sd": 2 * sd, "sesoi": sesoi,
            "delta": max(2 * sd, sesoi), "binding": "2sd" if 2 * sd > sesoi else "sesoi"}


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _paths(dataset: str, seed: int) -> tuple[Path, Path]:
    out = PILOT_DIR / f"{dataset}_seed{seed}.jsonl.gz"
    return out, out.with_name(out.name.removesuffix(".jsonl.gz") + ".meta.json")


def _setup(dataset: str):
    """(pilot records, class list, Tier-4 meta) from the logged dev tier outputs."""
    recs, meta, _ = read_tier_outputs(dataset)
    return pilot_records(recs), label_classes(meta, dataset), meta["tier4"]


def run_seed(dataset: str, seed: int) -> Path:
    """Tier 4 with seed `seed`'s shots on the pilot items; writes the outputs and their meta."""
    from splits import load_splits

    pilot, _, t4 = _setup(dataset)
    _, labels_file, task_ctx = TASKS[dataset]
    prompt = CONTRACTS / t4["prompt"]
    if _sha256(prompt) != t4["prompt_sha256"]:
        raise ValueError(f"{prompt.name} differs from the prompt the dev outputs were logged with")
    splits = load_splits(dataset)
    labels = [l for l in labels_file.read_text().split("\n") if l and not l.startswith("#")]
    k = t4["shots_per_label_and_language"]
    shots = select_shots(splits["train"], [l for l in labels if l != "oos"], k, seed=seed)  # train has no OOS
    canonical = {s["text"] for s in select_shots(splits["train"], [l for l in labels if l != "oos"], k)}
    rows = {(s, r["id"]): r for s in ("cal", "val") for r in splits[s]}
    t0 = time.monotonic()
    t4_rows = classify(model_path(t4["model"]), [rows[(r["split"], r["id"])] for r in pilot], shots, dataset,
                       prompt, t4.get("n_ctx", task_ctx))
    seconds = time.monotonic() - t0
    with_t4(pilot, t4_rows)  # alignment check before anything is written
    out, meta_path = _paths(dataset, seed)
    out.parent.mkdir(parents=True, exist_ok=True)
    with gzip.open(out, "wt") as f:
        f.writelines(json.dumps(o) + "\n" for o in t4_rows)
    boost = Path("/sys/devices/system/cpu/cpufreq/boost")
    meta_path.write_text(json.dumps({
        "dataset": dataset, "seed": seed, "n": len(pilot), "model": t4["model"], "prompt": prompt.name,
        "prompt_sha256": t4["prompt_sha256"], "n_ctx": t4.get("n_ctx", task_ctx),
        "shots_per_label_and_language": k, "shots": shots,
        "shots_shared_with_deployed": sum(s["text"] in canonical for s in shots),
        "seconds": round(seconds, 1),
        "date": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
        "git_head": subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True,
                                   cwd=ROOT).stdout.strip(),
        "boost": boost.read_text().strip() if boost.exists() else None,
        "note": "accuracy pilot; timings are not measurements"}, indent=1) + "\n")
    return out


def summarize() -> dict:
    """Scores of the deployed shots and of every logged seed per dataset, and δ from the primary."""
    out = {"pilot_frac": PILOT_FRAC, "pilot_key": PILOT_KEY, "sesoi": SESOI, "primary": PRIMARY, "datasets": {}}
    for dataset in sorted(DATASETS):
        seeds = sorted(int(p.name.removesuffix(".jsonl.gz").rsplit("seed", 1)[1])
                       for p in PILOT_DIR.glob(f"{dataset}_seed*.jsonl.gz"))
        if not seeds:
            continue
        pilot, classes, _ = _setup(dataset)
        oos_label = DATASETS[dataset][1]
        per_seed = {}
        for s in seeds:
            path, meta_path = _paths(dataset, s)
            with gzip.open(path, "rt") as f:
                t4_rows = [json.loads(l) for l in f]
            m = json.loads(meta_path.read_text())
            per_seed[str(s)] = r0_scores(with_t4(pilot, t4_rows), classes, oos_label) | {
                "shots_shared_with_deployed": m["shots_shared_with_deployed"], "n_shots": len(m["shots"]),
                "seconds": m["seconds"], "boost": m["boost"]}
        res = {"n_cal": sum(r["split"] == "cal" for r in pilot), "n_val": sum(r["split"] == "val" for r in pilot),
               "deployed_shots": r0_scores(pilot, classes, oos_label), "seeds": per_seed}
        if len(seeds) >= 3:
            res["tuned"] = delta([v["tuned_acc"] for v in per_seed.values()])
            res["untuned"] = delta([v["untuned_acc"] for v in per_seed.values()])
        out["datasets"][dataset] = res
    primary = out["datasets"].get(PRIMARY, {})
    out["delta"] = primary["tuned"]["delta"] if "tuned" in primary else None
    out["delta_note"] = ("δ from the primary dataset's tuned-R0 seeds (ADR-020)" if out["delta"] is not None
                         else f"not set: {PRIMARY} needs >= 3 logged seeds")
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", choices=sorted(DATASETS), help="run this dataset's missing seeds")
    ap.add_argument("--seeds", type=int, nargs="+", default=list(SEEDS))
    ap.add_argument("--summary", action="store_true", help="only summarize the seeds logged so far")
    args = ap.parse_args(argv)
    if args.dataset and not args.summary:
        for s in args.seeds:
            if _paths(args.dataset, s)[0].exists():
                print(f"{args.dataset} seed {s}: logged, skipped", flush=True)
                continue
            print(f"{args.dataset} seed {s}: running", flush=True)
            print(f"wrote {run_seed(args.dataset, s).relative_to(ROOT)}", flush=True)
    res = summarize()
    SUMMARY.write_text(json.dumps(res, indent=1) + "\n")
    for name, d in res["datasets"].items():
        print(name, json.dumps({k: d[k] for k in ("n_cal", "n_val", "deployed_shots", "tuned", "untuned") if k in d},
                               indent=1))
    print("delta:", res["delta"], "--", res["delta_note"])


if __name__ == "__main__":
    main()
