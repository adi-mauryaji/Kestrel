import math

import numpy as np
import pytest

from gates import Bounds, check

BOUNDS = Bounds(eps1_macro_f1=0.0048, eps2_forgetting=0.05, eps3_ece=0.0029, temperature_ratio=1.25,
                tier4_rate_rise=0.05, oos_rejection_drop=0.02, update_seconds=60.0, update_rss_mib=512.0)
REF = {"macro_f1": 0.956, "ece": 0.012, "temperature": 0.66, "tier4_rate": 0.14, "oos_rejection": 0.80,
       "personal_accuracy": 0.70}
GOOD = REF | {"macro_f1": 0.954, "ece": 0.013, "temperature": 0.70, "tier4_rate": 0.15, "oos_rejection": 0.79,
              "personal_accuracy": 0.78, "avg_forgetting": 0.01, "update_seconds": 3.0, "update_rss_mib": 100.0}


def weights_ok():
    return {"W": np.zeros((4, 3)), "b": np.zeros(3), "shape": (4, 3), "sha256_ok": True}


def test_a_candidate_inside_every_bound_is_accepted():
    assert check(REF, GOOD, weights_ok(), BOUNDS) == []


@pytest.mark.parametrize("change, gate", [
    ({"macro_f1": 0.950}, "1 regression macro-F1"),
    ({"avg_forgetting": 0.06}, "2 forgetting"),
    ({"ece": 0.0152}, "3 calibration"),
    ({"temperature": 0.50}, "3 calibration"),          # 0.66 / 0.50 = 1.32 > 1.25
    ({"tier4_rate": 0.20}, "4 routing mix"),
    ({"oos_rejection": 0.77}, "5 OOS rejection"),
    ({"personal_accuracy": 0.70}, "6 no personal gain"),  # equal is not an improvement
    ({"update_seconds": 61.0}, "7 resources"),
    ({"update_rss_mib": 600.0}, "7 resources"),
])
def test_each_condition_rejects_on_its_own(change, gate):
    failed = check(REF, GOOD | change, weights_ok(), BOUNDS)
    assert [f.split(":")[0] for f in failed] == [gate], failed


@pytest.mark.parametrize("bad", [
    {"W": np.full((4, 3), np.nan)},
    {"b": np.array([0.0, math.inf, 0.0])},
    {"W": np.zeros((3, 3))},
    {"sha256_ok": False},
])
def test_integrity_failures_reject(bad):
    failed = check(REF, GOOD, weights_ok() | bad, BOUNDS)
    assert [f.split(":")[0] for f in failed] == ["8 integrity"], failed


def test_a_missing_metric_rejects_instead_of_passing():
    cand = {k: v for k, v in GOOD.items() if k != "ece"}
    assert any(f.startswith("3 calibration") for f in check(REF, cand, weights_ok(), BOUNDS))
