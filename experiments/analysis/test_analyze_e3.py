import pytest

from analyze_e3 import analyze_sets, set_tiers
from test_analyze_e1 import scenario

# Thresholds that reproduce the scenario's R3: Tier 3 accepts at 0.9 (in scope 0.95, out of scope 0.5),
# Tier 4 at 0.6 (in scope 0.9; out of scope 0.3, so it abstains); Tier 1 never matches.
SETS = {"{4}": [None, None, 0.6], "{1,4}": [0.0, None, 0.6], "{3,4}": [None, 0.9, 0.6], "{1,3,4}": [0.0, 0.9, 0.6]}
ENTRY = {"signal": "conjunction", "sets": {k: {"point": {"tau": v}} for k, v in SETS.items()}}


def run(rows, gold, lat, entry=ENTRY):
    return analyze_sets(rows, gold, lat, entry, dataset="intents_v0", delta=0.0263, n_boot=300)


def test_set_names():
    assert set_tiers("{1,3,4}") == (1, 3, 4) and set_tiers("{4}") == (4,)


def test_each_tier_set_on_the_logged_answers():
    rows, gold, lat = scenario()
    out = run(rows, gold, lat)
    s = out["sets"]
    assert all(v["accuracy"] == 1.0 for v in s.values())
    # {4}: every item pays Tier 4 (800 ms); {3,4}: 31 items stop at Tier 3 (5 ms), 4 out-of-scope pay 5 + 800,
    # 5 flagged requests go straight to Tier 4 (the routing signal).
    assert s["{4}"]["cost_ms"]["mean"] == pytest.approx(800.0)
    assert s["{3,4}"]["cost_ms"]["mean"] == pytest.approx((31 * 5 + 4 * 805 + 5 * 800) / 40)
    assert s["{1,3,4}"]["tier_rate"]["3"] == pytest.approx(31 / 40)
    assert out["full_set"] == "{1,3,4}" and out["reproduces_r3"]


def test_h3_compares_the_full_set_with_the_set_without_tier_3():
    rows, gold, lat = scenario()
    h3 = run(rows, gold, lat)["h3"]
    assert (h3["a"], h3["b"]) == ("{1,3,4}", "{1,4}") and h3["family"] == "secondary (Holm)"
    assert h3["accuracy"]["noninferior"] and h3["cost"]["faster"]
    assert h3["p"] == max(h3["accuracy"]["p_noninferior"], h3["cost"]["p_below"])
    assert "item_level" in h3["accuracy"]  # Intents-v0: families, item level alongside


def test_e3_is_refused_when_the_full_set_does_not_reproduce_r3():
    rows, gold, lat = scenario()
    wrong = {"signal": "conjunction", "sets": dict(ENTRY["sets"]) | {"{1,3,4}": {"point": {"tau": [0.0, 0.99, 0.6]}}}}
    with pytest.raises(ValueError, match="reproduce"):
        run(rows, gold, lat, wrong)


def test_a_set_that_missed_the_floor_uses_its_best_reachable_point():
    rows, gold, lat = scenario()
    missed = {"point": None, "best_reachable": {"tau": [None, None, 0.6]}}
    entry = {"signal": "conjunction", "sets": dict(ENTRY["sets"]) | {"{4}": missed}}
    s = run(rows, gold, lat, entry)["sets"]["{4}"]
    assert s["chosen"] == "best_reachable" and s["accuracy"] == 1.0
