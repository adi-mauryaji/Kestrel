"""experiments/analysis/analyze_e5.py -- E5 analysis (H5; PREREG §4, §6, ADR-027, ADR-029): Table R6 and H5.

    python experiments/analysis/analyze_e5.py      # reads experiments/logs/E5/test.json

H5: replay's average forgetting is lower than naive's. Test (ADR-029): Wilcoxon signed-rank over the 15
runs paired by (order, seed), two-sided, counted only if the effect points the hypothesized way; its p
enters Holm's secondary family. Effect: the mean paired difference with a 95% bootstrap CI over the 15
pairs (10 000 resamples, seed 0), and d_z. EWC vs naive gets the same numbers, reported outside the
family. Table R6: per method, mean ± SD over its runs of average accuracy, average forgetting, BWT,
update seconds and buffer bytes (e5_test.py's summary). Out: experiments/logs/E5/analysis_test.json.
"""
from __future__ import annotations

import json
from pathlib import Path

import numpy as np

from kestrel_stats import N_BOOT, cohens_dz, paired_bootstrap_ci, wilcoxon_paired

ROOT = Path(__file__).resolve().parents[2]
LOG = ROOT / "experiments" / "logs" / "E5" / "test.json"
OUT = ROOT / "experiments" / "logs" / "E5" / "analysis_test.json"


def paired(runs, a: str, b: str, metric: str) -> tuple[np.ndarray, np.ndarray]:
    """metric of methods a and b, run by run, paired on (order, seed); refuses unpaired runs."""
    val = {m: {(r["order"], r["seed"]): r[metric] for r in runs if r["method"] == m and "error" not in r}
           for m in (a, b)}
    if set(val[a]) != set(val[b]):
        raise ValueError(f"{a} and {b} do not pair: {sorted(set(val[a]) ^ set(val[b]))[:5]}")
    keys = sorted(val[a])
    return np.array([val[a][k] for k in keys]), np.array([val[b][k] for k in keys])


def compare(runs, a: str, b: str, metric: str = "avg_forgetting", n_boot: int = N_BOOT, seed: int = 0) -> dict:
    """The claim "a's metric is lower than b's": Wilcoxon p, mean difference a - b with its CI, d_z."""
    xa, xb = paired(runs, a, b, metric)
    m, lo, hi = paired_bootstrap_ci(xa, xb, n_boot=n_boot, seed=seed)
    return {"metric": metric, "claim": f"{a} < {b}", "n": len(xa), "mean_a": float(xa.mean()),
            "mean_b": float(xb.mean()), "diff": m, "ci95": [lo, hi], "wilcoxon_p": wilcoxon_paired(xa, xb),
            "dz": cohens_dz(xa, xb), "direction_ok": m < 0}


def analyze(report: dict, n_boot: int = N_BOOT) -> dict:
    h5 = compare(report["runs"], "replay", "naive", n_boot=n_boot)
    ewc = compare(report["runs"], "ewc", "naive", n_boot=n_boot)
    return {"table_r6": report["summary"],
            "h5": h5 | {"p": h5["wilcoxon_p"], "family": "secondary (Holm)"},
            "ewc_vs_naive": ewc | {"family": "reported, outside the family"}}


def main() -> int:
    report = json.loads(LOG.read_text())
    out = {"experiment": "E5 test analysis", "source": str(LOG.relative_to(ROOT)),
           "e5_json_sha256": report.get("e5_json_sha256")} | analyze(report)
    OUT.write_text(json.dumps(out, indent=1) + "\n")
    for name, s in out["table_r6"].items():
        if s:
            print(f"{name:7s} acc {s['avg_accuracy']['mean']:.3f}±{s['avg_accuracy']['sd']:.3f}  forget "
                  f"{s['avg_forgetting']['mean']:.3f}±{s['avg_forgetting']['sd']:.3f}  bwt {s['bwt']['mean']:+.3f}  "
                  f"s {s['seconds']['mean']:.1f}  buffer {s['buffer_bytes']['mean']:.0f} B  (n {s['runs']})")
    for key in ("h5", "ewc_vs_naive"):
        c = out[key]
        print(f"{key}: {c['claim']} on {c['metric']}: diff {c['diff']:+.4f} [{c['ci95'][0]:+.4f}, {c['ci95'][1]:+.4f}]"
              f", Wilcoxon p {c['wilcoxon_p']:.2g}, d_z {c['dz']:.2f} ({c['family']})")
    print(f"-> {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
