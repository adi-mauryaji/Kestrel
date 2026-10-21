"""experiments/router/check_runtime.py -- does the Rust cascade decide what the simulation decided?

    cargo build --release -p kestrel-cli --features llama
    python experiments/router/check_runtime.py                     # ~2 min: Intents-v0 val (128)
    python experiments/router/check_runtime.py --dataset clinc150  # ~6 min: CLINC150 val (1700)

Runs `kestrel route-items` with the generated config (experiments/configs/router_<dataset>.json) on
the val items, and simulates the same config on the logged tier outputs (policies.route). For every
item, the label and the tier where routing ended must agree. Confidences are compared too. Runtime
Tier 3 uses f32 and Tier 4 decodes live, so small gaps are expected; midpoint thresholds keep them
away from the decisions. Wall times are dev timings (as found, boost and AC state recorded), the
first look at E1's numbers. Out: experiments/logs/router/runtime_check_<dataset>.json, or --out.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from policies import DATASETS, LOG_DIR, OFF, Answers, correct, label_classes, read_tier_outputs, route  # noqa: E402

KESTREL = ROOT / "target" / "release" / "kestrel"
CONFIGS = ROOT / "experiments" / "configs"


def sim_settings(cfg: dict):
    """(tau, tiers, signal) for policies.route from a router config."""
    tiers = tuple(cfg["tiers"])
    acc = cfg["accept"]
    tau = tuple(acc[i] if t in tiers else OFF for t, i in ((1, 0), (3, 2), (4, 3)))
    return tau, tiers, ("conjunction" if cfg["multi_intent_to_generative"] else None)


def compare(ids, classes, sim_pred, sim_tier, sim_conf, rows) -> dict:
    """Item-by-item agreement of (label, tier where routing ended), and confidence gaps per tier."""
    if [r["id"] for r in rows] != list(ids):
        raise ValueError("runtime rows are not the simulated items, in order")
    mism, gaps = [], {}
    for i, r in enumerate(rows):
        sim = [classes[sim_pred[i]] if sim_pred[i] >= 0 else None, int(sim_tier[i])]
        run = [r["intent"], r["tier"]]
        if sim != run:
            mism.append({"id": r["id"], "sim": sim, "runtime": run})
        elif r["confidence"] is not None:
            key = str(r["tier"])
            gaps[key] = max(gaps.get(key, 0.0), abs(r["confidence"] - float(sim_conf[i])))
    return {"n": len(rows), "agree": len(rows) - len(mism), "mismatches": mism, "conf_max_abs_diff": gaps}


def machine_state() -> str:
    """Turbo boost and AC power as found (sysfs), for the timing note."""
    def read(path):
        try:
            return Path(path).read_text().strip()
        except OSError:
            return "?"
    ac = next((read(p) for p in sorted(Path("/sys/class/power_supply").glob("AC*/online"))), "?")
    return f"boost {read('/sys/devices/system/cpu/cpufreq/boost')}, AC {ac}"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", default="intents_v0", choices=sorted(DATASETS))
    ap.add_argument("--out", type=Path, help="default: experiments/logs/router/runtime_check_<dataset>.json")
    args = ap.parse_args(argv)
    config = CONFIGS / f"router_{args.dataset}.json"
    out_path = args.out or LOG_DIR / f"runtime_check_{args.dataset}.json"
    cfg = json.loads(config.read_text())
    recs, meta, _ = read_tier_outputs(args.dataset)
    val = [r for r in recs if r["split"] == "val"]
    texts = {r["id"]: r["text"] for r in _rows(args.dataset, "val")}
    classes = label_classes(meta, args.dataset)

    cal = cfg["calibration"]
    rule_conf = cal["tier1_confidence"] if cal["tier1_confidence"] is not None else 1.0  # unused without Tier 1
    a = Answers.build(val, classes, rule_conf=rule_conf, t3=cal["tier3_temperature"],
                      t4=cal["tier4_temperature"], oos_label=DATASETS[args.dataset][1])
    tau, tiers, signal = sim_settings(cfg)
    sim = route(a, tau, tiers=tiers, signal=signal)
    conf = np.select([sim.tier == 1, sim.tier == 3, sim.tier == 4], [a.c1, a.c3, a.c4], 0.0)

    if not KESTREL.exists():
        raise FileNotFoundError(f"{KESTREL}: cargo build --release -p kestrel-cli --features llama")
    with tempfile.TemporaryDirectory() as d:
        items, out = Path(d) / "items.jsonl", Path(d) / "out.jsonl"
        items.write_text("".join(json.dumps({"id": r["id"], "text": texts[r["id"]]}) + "\n" for r in val))
        subprocess.run([str(KESTREL), "route-items", str(config), str(items), str(out)], check=True, cwd=ROOT)
        rows = [json.loads(l) for l in out.read_text().splitlines()]

    res = compare([r["id"] for r in val], classes, sim.pred, sim.tier, conf, rows)
    idx = {c: i for i, c in enumerate(classes)}
    run_pred = np.array([idx[r["intent"]] if r["intent"] else -1 for r in rows])
    run_tier = np.array([r["tier"] for r in rows])
    ms = np.array([r["ms"] for r in rows])
    res |= {"config_sha256": cfg["config_sha256"],
            "accuracy": {"runtime": float(correct(a, run_pred, run_tier).mean()),
                         "simulation": float(correct(a, sim.pred, sim.tier).mean())},
            "wall_ms": {"mean": float(ms.mean()), "p50": float(np.percentile(ms, 50)),
                        "p95": float(np.percentile(ms, 95)), "simulated_mean_cost": float(sim.cost.mean()),
                        "by_end_tier": {str(t): {"n": int((run_tier == t).sum()), "mean": float(ms[run_tier == t].mean())}
                                        for t in sorted(set(run_tier.tolist()))}},
            "note": f"dev timing on the laptop as found ({machine_state()}); not an E1 measurement"}
    out_path.write_text(json.dumps(res, indent=1) + "\n")
    print(json.dumps({k: v for k, v in res.items() if k != "mismatches"}, indent=1))
    print("mismatches:", res["mismatches"])


def _rows(dataset, split):
    for sub in ("data", "tier3"):
        sys.path.insert(0, str(ROOT / "experiments" / sub))
    from splits import load_splits
    return load_splits(dataset)[split]


if __name__ == "__main__":
    main()
