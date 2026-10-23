"""experiments/continual/gates.py -- Ch. 23.2's rejection conditions for an updated head (Week 11 "T").

An updated head is rejected, and the previous version kept, if ANY condition holds. `check` returns the
failed conditions ("N name: detail"); empty = accept. A missing metric fails its condition: a gate that
cannot be evaluated does not pass. eps1-eps3 come from PREREG §5 (ADR-027); the other bounds have no
pre-registered values yet and must be passed explicitly (E4 sets them before it runs).
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np


# Metrics judged against the current head (the rest are judged on the candidate alone).
COMPARED = ("macro_f1", "ece", "temperature", "tier4_rate", "oos_rejection", "personal_accuracy")


@dataclass(frozen=True)
class Bounds:
    eps1_macro_f1: float        # 1: largest allowed macro-F1 drop on the frozen regression suite
    eps2_forgetting: float      # 2: largest allowed average forgetting on the stage matrix
    eps3_ece: float             # 3: largest allowed ECE rise
    temperature_ratio: float    # 3: largest allowed change of the re-fitted T, as a ratio either way
    tier4_rate_rise: float      # 4: largest allowed rise of the Tier-4 reach rate
    oos_rejection_drop: float   # 5: largest allowed drop of the OOS rejection rate
    update_seconds: float       # 7: resource limits of one update
    update_rss_mib: float


def check(ref: dict, cand: dict, weights: dict, b: Bounds) -> list[str]:
    """ref: the current head's metrics; cand: the candidate's (plus avg_forgetting, update_seconds,
    update_rss_mib); weights: W, b, the expected shape and whether the file hash matched."""
    failed = []

    def gate(name, keys, bad, detail):
        missing = [k for k in keys if k not in cand or (k in COMPARED and k not in ref)]
        if missing:
            failed.append(f"{name}: missing {', '.join(missing)}")
        elif bad():
            failed.append(f"{name}: {detail()}")

    gate("1 regression macro-F1", ["macro_f1"], lambda: ref["macro_f1"] - cand["macro_f1"] > b.eps1_macro_f1,
         lambda: f"drop {ref['macro_f1'] - cand['macro_f1']:.4f} > {b.eps1_macro_f1}")
    gate("2 forgetting", ["avg_forgetting"], lambda: cand["avg_forgetting"] > b.eps2_forgetting,
         lambda: f"{cand['avg_forgetting']:.4f} > {b.eps2_forgetting}")
    gate("3 calibration", ["ece"], lambda: cand["ece"] - ref["ece"] > b.eps3_ece,
         lambda: f"ECE rise {cand['ece'] - ref['ece']:.4f} > {b.eps3_ece}")
    ratio = lambda: max(cand["temperature"] / ref["temperature"], ref["temperature"] / cand["temperature"])  # noqa: E731
    gate("3 calibration", ["temperature"], lambda: ratio() > b.temperature_ratio,
         lambda: f"T changed by x{ratio():.3f} > x{b.temperature_ratio}")
    gate("4 routing mix", ["tier4_rate"], lambda: cand["tier4_rate"] - ref["tier4_rate"] > b.tier4_rate_rise,
         lambda: f"Tier-4 rate +{cand['tier4_rate'] - ref['tier4_rate']:.3f} > {b.tier4_rate_rise}")
    gate("5 OOS rejection", ["oos_rejection"],
         lambda: ref["oos_rejection"] - cand["oos_rejection"] > b.oos_rejection_drop,
         lambda: f"drop {ref['oos_rejection'] - cand['oos_rejection']:.3f} > {b.oos_rejection_drop}")
    gate("6 no personal gain", ["personal_accuracy"],
         lambda: cand["personal_accuracy"] <= ref["personal_accuracy"],
         lambda: f"{cand['personal_accuracy']:.3f} <= {ref['personal_accuracy']:.3f}")
    gate("7 resources", ["update_seconds", "update_rss_mib"],
         lambda: cand["update_seconds"] > b.update_seconds or cand["update_rss_mib"] > b.update_rss_mib,
         lambda: f"{cand['update_seconds']:.1f} s / {cand['update_rss_mib']:.0f} MiB over "
                 f"{b.update_seconds} s / {b.update_rss_mib} MiB")
    W, bias = np.asarray(weights["W"]), np.asarray(weights["b"])
    problems = [p for p, bad in [("non-finite weights", not (np.isfinite(W).all() and np.isfinite(bias).all())),
                                 (f"shape {W.shape} != {tuple(weights['shape'])}", W.shape != tuple(weights["shape"])),
                                 ("hash mismatch", not weights["sha256_ok"])] if bad]
    if problems:
        failed.append("8 integrity: " + "; ".join(problems))
    return failed
