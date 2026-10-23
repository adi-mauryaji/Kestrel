import numpy as np
import pytest

from analyze_e5 import analyze, compare, paired

KEYS = [(o, s) for o in ("1", "2", "3") for s in (1, 2, 3, 4, 5)]


def runs(method, forgetting, acc=0.5):
    return [{"method": method, "order": o, "seed": s, "avg_forgetting": f, "avg_accuracy": acc, "bwt": -f,
             "seconds": 1.0, "buffer_bytes": 0} for (o, s), f in zip(KEYS, forgetting)]


def test_runs_are_paired_by_order_and_seed():
    rs = runs("replay", [0.01] * 15) + runs("naive", [0.9] * 15)
    a, b = paired(rs, "replay", "naive", "avg_forgetting")
    assert len(a) == len(b) == 15
    with pytest.raises(ValueError, match="pair"):
        paired(rs[:-1], "replay", "naive", "avg_forgetting")  # naive lost a run


def test_replay_vs_naive_on_forgetting():
    rng = np.random.default_rng(0)
    rs = runs("replay", 0.001 + rng.random(15) * 1e-3) + runs("naive", 0.95 + rng.random(15) * 1e-2)
    c = compare(rs, "replay", "naive", n_boot=500)
    assert c["n"] == 15 and c["diff"] < -0.9 and c["ci95"][1] < 0 and c["direction_ok"]
    assert c["wilcoxon_p"] == pytest.approx(2 / 2 ** 15)  # exact: every pair has replay lower


def test_analyze_reports_table_r6_h5_and_ewc_outside_the_family():
    rng = np.random.default_rng(1)
    rs = (runs("replay", 0.001 + rng.random(15) * 1e-3) + runs("naive", 0.95 + rng.random(15) * 1e-2)
          + runs("ewc", 0.94 + rng.random(15) * 1e-2))
    summary = {"naive": {"runs": 15}, "replay": {"runs": 15}, "ewc": {"runs": 15}}
    out = analyze({"runs": rs, "summary": summary}, n_boot=500)
    assert out["table_r6"] == summary
    assert out["h5"]["p"] == out["h5"]["wilcoxon_p"] and out["h5"]["family"] == "secondary (Holm)"
    assert out["ewc_vs_naive"]["family"] == "reported, outside the family"
