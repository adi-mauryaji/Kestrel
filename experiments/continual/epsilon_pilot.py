"""experiments/continual/epsilon_pilot.py -- eps1 and eps3 for Ch. 23's head gates (ADR-022, ADR-027).

    python experiments/continual/epsilon_pilot.py     # ~minutes of CPU: 5 lbfgs retrains

The deployed Tier-3 head (lbfgs softmax regression with Week 5's C) has no seed, so its variance comes
from the data (ADR-022): retrain it on B bootstrap resamples of train (same C), fit T on cal, score val
in-scope (macro-F1 and ECE after T, 15 bins as in Week 5). eps1 = 2 x SD(macro-F1) and eps3 = 2 x SD(ECE)
over the B retrains, the rule delta uses (PREREG 5). eps2 (forgetting) comes from E5's naive runs
(e5_sweep.py). Out: experiments/logs/E5/epsilon_pilot.json.
"""
from __future__ import annotations

import json
import statistics
import sys
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "tier3"))
sys.path.insert(0, str(ROOT / "experiments" / "analysis"))
from head import fit_temperature, logits, softmax, train_head  # noqa: E402
from metrics import ece, macro_f1  # noqa: E402

B = 5
ECE_BINS = 15
WEEK5 = ROOT / "experiments" / "logs" / "tier3" / "week5_dev.json"
OUT = ROOT / "experiments" / "logs" / "E5" / "epsilon_pilot.json"
ENCODER = "multilingual-e5-small-q8_0"


def week5_c(log) -> float:
    """The C Week 5 chose for CLINC150 with the deployed encoder (ADR-014)."""
    for run in log:
        if run["dataset"] == "clinc150" and run["encoder"] == ENCODER:
            return float(run["c"])
    raise LookupError(f"no clinc150 x {ENCODER} run in the Week-5 log")


def bootstrap_scores(x_tr, y_tr, x_cal, y_cal, x_val, y_val, *, n_classes: int, c: float, b: int = B,
                     seed: int = 0) -> list[dict]:
    """One retrain per bootstrap resample of train (seeded): val macro-F1, accuracy, ECE after T fitted on cal."""
    rng = np.random.default_rng(seed)
    out = []
    for _ in range(b):
        idx = rng.integers(0, len(y_tr), len(y_tr))
        w, bias = train_head(x_tr[idx], y_tr[idx], n_classes, c)
        t = fit_temperature(logits(x_cal, w, bias), y_cal)
        p = softmax(logits(x_val, w, bias), t)
        pred = p.argmax(axis=1)
        out.append({"macro_f1": macro_f1(y_val.tolist(), pred.tolist()),
                    "accuracy": float((pred == y_val).mean()),
                    "ece": ece(p.max(axis=1).tolist(), (pred == y_val).tolist(), n_bins=ECE_BINS),
                    "temperature": float(t)})
    return out


def epsilons(scores: list[dict]) -> dict:
    f1 = [s["macro_f1"] for s in scores]
    e = [s["ece"] for s in scores]
    return {"n": len(scores), "eps1_macro_f1": 2 * statistics.stdev(f1), "eps3_ece": 2 * statistics.stdev(e),
            "macro_f1": {"mean": statistics.fmean(f1), "sd": statistics.stdev(f1)},
            "ece": {"mean": statistics.fmean(e), "sd": statistics.stdev(e)}}


def main():
    from e5_data import load
    data = load()
    c = week5_c(json.loads(WEEK5.read_text()))
    t0 = time.perf_counter()
    scores = bootstrap_scores(data["x"]["train"], data["y"]["train"], data["x"]["cal"], data["y"]["cal"],
                              data["x"]["val"], data["y"]["val"], n_classes=len(data["classes"]), c=c)
    report = {"pilot": "eps1/eps3 from bootstrap retrains of the Tier-3 head (ADR-022, ADR-027)",
              "dataset": "clinc150 dev (train -> cal T -> val in-scope)", "encoder": ENCODER, "c": c, "b": B,
              "seed": 0, "ece_bins": ECE_BINS, "seconds": time.perf_counter() - t0, "retrains": scores,
              **epsilons(scores)}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(report, indent=1) + "\n")
    print(json.dumps({k: report[k] for k in ("eps1_macro_f1", "eps3_ece", "macro_f1", "ece", "seconds")}, indent=1))


if __name__ == "__main__":
    main()
