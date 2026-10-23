"""experiments/analysis/analyze_e3.py -- E3 (H3; PREREG §9, ADR-031): Table R3 and H3, from E1's test rows.

    python experiments/analysis/analyze_e3.py --dataset intents_v0 --split test
    python experiments/analysis/analyze_e3.py --dataset intents_v0 --limit 20 --dev    # the dev dry runs

Each tier set's frozen point (experiments/configs/e3_candidates.json: tau per tier, the routing signal) is
applied to the per-tier answers in E1's rows (Tier 1 = R3's first step, Tier 3 = R1's, Tier 4 = R0's; the
frozen calibration's confidences) with policies.route. An item's cost is the sum of its consulted tiers'
step times measured in E1 (per-item medians; reported on the latency subset). The full set must give back
R3's logged decisions, or E3 is refused. H3 (Intents-v0): {1,3,4} vs {1,4}, accuracy non-inferior at delta
and cost lower, intersection-union; families resampled (ADR-029). CLINC150 (no Tier 1): {3,4} vs {4},
reported the same way, outside the family. Table R3's area under the cost-accuracy curve is not computed
(one frozen point per set, PREREG §9). Out: experiments/logs/E3/analysis_<run>.json.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np

from analyze_e1 import (CLUSTERS, DELTA_LOG, OOS, _clean, load_runs, nearest_rank, noninferiority, rep1, speed,
                        step_times, tier_answers)
from kestrel_stats import N_BOOT, intersection_union_p
from policies import DATASETS, OFF, correct, route

ROOT = Path(__file__).resolve().parents[2]
E3_CONFIG = ROOT / "experiments" / "configs" / "e3_candidates.json"
OUT_DIR = ROOT / "experiments" / "logs" / "E3"
COMPARE = {"intents_v0": ("{1,3,4}", "{1,4}", "secondary (Holm)"),
           "clinc150": ("{3,4}", "{4}", "reported, outside the family")}


def set_tiers(name: str) -> tuple[int, ...]:
    return tuple(int(t) for t in name.strip("{}").split(","))


def analyze_sets(rows, gold: dict, latency_ids, entry: dict, *, dataset: str, delta: float, n_boot: int = N_BOOT,
                 seed: int = 0) -> dict:
    oos = OOS[dataset]
    timed_rows = [r for r in rows if not r["warmup"]]
    ids, dec, gold_rows, _ = rep1(timed_rows, gold)
    a, idx = tier_answers(dec, gold_rows, step_times(timed_rows), oos)
    timed = np.array([i in latency_ids for i in ids])
    fam = np.array([g[CLUSTERS[dataset]] for g in gold_rows]) if dataset in CLUSTERS else None
    sub = lambda mask: None if fam is None else fam[mask]  # noqa: E731
    full = "{" + ",".join(map(str, DATASETS[dataset][0])) + "}"
    signal = entry["signal"]
    sets, ok, cost = {}, {}, {}
    for name, s in entry["sets"].items():
        tiers = set_tiers(name)
        kind = "point" if s.get("point") else "best_reachable"  # a set that misses the floor: its best (frozen rule)
        tau = s[kind]["tau"]
        r = route(a, tuple(OFF if t is None else t for t in tau), tiers, signal)
        if name == full:
            logged = np.array([-1 if x["intent"] is None else idx[x["intent"]] for x in dec["R3"]])
            if not (np.array_equal(r.pred, logged) and np.array_equal(r.tier, [x["tier"] for x in dec["R3"]])):
                raise ValueError(f"the full set {full} does not reproduce R3's logged decisions")
        ok[name], cost[name] = correct(a, r.pred, r.tier), r.cost
        c = r.cost[timed]
        sets[name] = {"tiers": list(tiers), "tau": tau, "chosen": kind, "accuracy": ok[name].mean(),
                      "coverage": (r.pred != -1).mean(), "tier_rate": {str(k): (r.tier == k).mean() for k in (1, 3, 4)},
                      "cost_ms": {"n": int(timed.sum()), "mean": c.mean(), "median": np.median(c),
                                  "p95": nearest_rank(c, 95)}}
    a_name, b_name, family = COMPARE[dataset]
    acc = noninferiority(ok[a_name], ok[b_name], delta, n_boot, seed, sub(np.ones(len(ids), bool)))
    lat = speed(cost[a_name][timed], cost[b_name][timed], n_boot, seed, sub(timed))
    h3 = {"a": a_name, "b": b_name, "claim": f"{a_name} is non-inferior in accuracy and cheaper than {b_name}",
          "accuracy": acc, "cost": lat, "p": intersection_union_p(acc["p_noninferior"], lat["p_below"]),
          "family": family, "outcome": "after Holm (secondary family)" if family.startswith("secondary") else None}
    return _clean({"dataset": dataset, "n_items": len(ids), "n_latency_items": int(timed.sum()), "delta": delta,
                   "signal": signal, "clusters": CLUSTERS.get(dataset), "full_set": full, "reproduces_r3": True,
                   "sets": sets, "h3": h3, "area_under_cost_accuracy_curve": "not computed: one frozen point per set"})


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", required=True, choices=sorted(OOS))
    ap.add_argument("--split", default="val", choices=["val", "test"])
    ap.add_argument("--limit", type=int)
    ap.add_argument("--dev", action="store_true")
    ap.add_argument("--n-boot", type=int, default=N_BOOT)
    args = ap.parse_args(argv)
    if args.split == "test" and (args.dev or args.limit):
        ap.error("--dev and --limit are for val dry runs")
    run = load_runs(args.dataset, args.split, args.limit, dev=args.dev)
    entry = json.loads(E3_CONFIG.read_text())["datasets"][args.dataset]
    if entry["router_config_sha256"] != run["configs"]["R3"]["config_sha256"]:
        print(f"note: E1's R3 ran config {run['configs']['R3']['config_sha256'][:12]}, e3_candidates points at "
              f"{entry['router_config_sha256'][:12]} (dev dry runs predate the freeze)")
        if args.split == "test":
            raise SystemExit("E3 on test needs E1 run with the frozen configs e3_candidates.json points at")
    out = {"experiment": "E3", "split": args.split, "dev": args.dev,
           "runs": [str(d.relative_to(ROOT)) for d in run["dirs"]],
           "e3_candidates": str(E3_CONFIG.relative_to(ROOT))} | analyze_sets(
        run["rows"], run["gold"], run["latency_ids"], entry, dataset=args.dataset,
        delta=json.loads(DELTA_LOG.read_text())["delta"], n_boot=args.n_boot)
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    path = OUT_DIR / f"analysis_{run['name']}.json"
    path.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
    if args.dev:
        print("DEV: dry runs (never valid), not E3 results.")
    for name, s in out["sets"].items():
        print(f"{name:8s} acc {s['accuracy']:.3f}  cost mean {s['cost_ms']['mean']:.1f} ms  "
              f"p95 {s['cost_ms']['p95']:.1f}  T1/T3/T4 "
              + "/".join(f"{s['tier_rate'][k]:.2f}" for k in ("1", "3", "4")))
    h = out["h3"]
    print(f"H3 {h['a']} vs {h['b']}: dacc {h['accuracy']['diff']:+.4f} {h['accuracy']['ci95']}, dcost "
          f"{h['cost']['diff_ms']:+.1f} ms {h['cost']['ci95']}, IUT p {h['p']:.4f} ({h['family']})")
    print(f"-> {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
