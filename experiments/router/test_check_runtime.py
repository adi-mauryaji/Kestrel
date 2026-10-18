import numpy as np

from check_runtime import compare, sim_settings


def test_sim_settings_follow_the_config():
    cfg = {"tiers": [1, 3, 4], "accept": [0.0, 1.0, 0.96, 0.0], "multi_intent_to_generative": True}
    tau, tiers, signal = sim_settings(cfg)
    assert tau == (0.0, 0.96, 0.0) and tiers == (1, 3, 4) and signal == "conjunction"
    tau, tiers, signal = sim_settings({**cfg, "tiers": [3, 4], "multi_intent_to_generative": False})
    assert tiers == (3, 4) and signal is None


def test_compare_counts_agreement_and_confidence_gaps():
    classes = ["a", "b"]
    sim_pred, sim_tier = np.array([0, -1, 1]), np.array([3, 4, 4])
    sim_conf = np.array([0.9, 0.0, 0.7])
    rows = [{"id": "x", "intent": "a", "tier": 3, "confidence": 0.9000004, "ms": 6.0},
            {"id": "y", "intent": None, "tier": 4, "confidence": None, "ms": 800.0},
            {"id": "z", "intent": "a", "tier": 4, "confidence": 0.7, "ms": 900.0}]
    r = compare(["x", "y", "z"], classes, sim_pred, sim_tier, sim_conf, rows)
    assert r["n"] == 3 and r["agree"] == 2
    assert r["mismatches"] == [{"id": "z", "sim": ["b", 4], "runtime": ["a", 4]}]
    assert abs(r["conf_max_abs_diff"]["3"] - 4e-7) < 1e-9
