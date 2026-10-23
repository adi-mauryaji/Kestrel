"""experiments/router/e3_points.py -- E3 (H3) candidate operating points per tier set (Week 12).

    python experiments/router/e3_points.py     # both datasets, from the logged tier outputs; no model

H3: adding Tier 3 to {1, 4} is a Pareto improvement (PREREG §1). Each tier set gets R3's rule
(ADR-019 / ADR-021): the cheapest point on val with accuracy >= the better tuned single tier
(tolerance 0), with the deployed router's calibration and signal (experiments/configs/
router_<dataset>.json). A set that cannot reach that floor also gets its best reachable point (the
highest floor it meets, in steps of 0.005), so its frontier position is still on record. CLINC150 has
no Tier 1 (the rules speak Kestrel's intents), so its sets are {4} and {3, 4}. The full set equals the
deployed R3 router (tested). Out: experiments/configs/e3_candidates.json.
"""
from __future__ import annotations

import json
from pathlib import Path

from make_config import outputs_sha256
from policies import (DATASETS, best_reachable, calibrated, label_classes, read_tier_outputs, reference_acc,
                      tier_set_point)

ROOT = Path(__file__).resolve().parents[2]
CONFIGS = ROOT / "experiments" / "configs"
OUT = CONFIGS / "e3_candidates.json"
SETS = {"intents_v0": [(4,), (1, 4), (3, 4), (1, 3, 4)], "clinc150": [(4,), (3, 4)]}


def name(tiers) -> str:
    return "{" + ",".join(map(str, tiers)) + "}"


def points(dataset: str) -> dict:
    tiers, oos = DATASETS[dataset]
    recs, meta, outputs = read_tier_outputs(dataset)
    _, cal_, *_ = calibrated(recs, label_classes(meta, dataset), meta["tier3"]["temperature_cal"], oos, tiers)
    cfg = json.loads((CONFIGS / f"router_{dataset}.json").read_text())
    signal = "conjunction" if cfg["multi_intent_to_generative"] else None
    ref = reference_acc(cal_)
    out = {"n_val": len(cal_.gold), "reference_acc": ref, "signal": signal,
           "router_config_sha256": cfg["config_sha256"], "tier_outputs_sha256": outputs_sha256(outputs), "sets": {}}
    for s in SETS[dataset]:
        p = tier_set_point(cal_, ref, signal, s)
        entry = {"point": p}
        if p is None:
            entry["best_reachable"] = best_reachable(cal_, signal, s, start=ref)
        out["sets"][name(s)] = entry
    return out


def main():
    report = {"experiment": "E3 (H3) candidate points, chosen on val at tolerance 0 (Week 12)",
              "rule": "cheapest point with accuracy >= max(tuned R0, tuned R1) on val; best reachable if none",
              "datasets": {d: points(d) for d in SETS}}
    OUT.write_text(json.dumps(report, indent=1) + "\n")
    for d, v in report["datasets"].items():
        print(f"{d} (reference {v['reference_acc']:.3f}, signal {v['signal']})")
        for s, e in v["sets"].items():
            p = e["point"] or e.get("best_reachable")
            tag = "rule" if e["point"] else f"best reachable (floor {p['floor']:.3f})" if p else "none"
            if p:
                print(f"  {s:8s} acc {p['acc']:.3f} cost {p['cost']:7.1f} ms tau {p['tau']}  [{tag}]")


if __name__ == "__main__":
    main()
