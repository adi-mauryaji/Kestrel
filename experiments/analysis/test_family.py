import pytest

from family import MEMBERS, decide, members


def non_inf(diff, lo, hi, p):
    return {"diff": diff, "ci95": [lo, hi], "p_noninferior": p, "noninferior": lo > -0.0263, "margin": 0.0263}


def faster(diff, lo, hi, p):
    return {"diff_ms": diff, "ci95": [lo, hi], "p_below": p, "faster": hi < 0}


E1 = {"intents_v0": {"delta": 0.0263, "h1": {"accuracy": non_inf(0.0, -0.01, 0.01, 0.001),
                                           "latency": faster(-300, -400, -200, 0.0), "p_iut": 0.001},
                     "h1b": {"accuracy": {"diff": 0.05, "ci95": [0.02, 0.08], "p_above": 0.002}, "p": 0.002},
                     "h1c": {"accuracy": non_inf(-0.05, -0.08, -0.03, 1.0), "p": 1.0},
                     "h7a": {"accuracy": non_inf(0.0, -0.03, 0.02, 0.08)}},
      "clinc150": {"delta": 0.0263, "h1b": {"accuracy": {"diff": 0.1, "ci95": [0.08, 0.12], "p_above": 0.0}, "p": 0.0},
                   "h1c": {"accuracy": non_inf(0.0, -0.01, 0.01, 0.0), "p": 0.0}}}
E2 = {"intents_v0": {"h2": {"risk_at_080": {"diff": 0.005, "ci95": [-0.001, 0.02], "p_below": 1.0}, "p": 1.0},
                     "h7b": {"tier3": {"gap": 0.04, "ci95": [-0.002, 0.11], "p_above": 0.06, "p": 0.06}}},
      "clinc150": {"h2": {"risk_at_080": {"diff": -0.007, "ci95": [-0.016, -0.002], "p_below": 0.02}, "p": 0.02}}}
E3 = {"intents_v0": {"delta": 0.0263, "h3": {"accuracy": non_inf(0.0, -0.01, 0.01, 0.0),
                                           "cost": faster(-300, -400, -200, 0.0), "p": 0.0}}}
E5 = {"h5": {"diff": -0.95, "ci95": [-0.953, -0.946], "wilcoxon_p": 6.1e-5, "p": 6.1e-5, "direction_ok": True}}


def test_the_family_has_13_members_and_waits_for_h4_and_h6():
    assert len(MEMBERS) == 13
    m = members(E1, E2, E3, E5)
    assert [x["id"] for x in m] == list(MEMBERS)
    assert {x["id"] for x in m if x["p"] is None} == {"H4", "H6"}


def test_holm_with_pending_members_and_the_outcomes():
    out = {x["id"]: x for x in decide(members(E1, E2, E3, E5))}
    assert out["H4"]["outcome"] == "pending" and out["H4"]["p_holm"] is None
    # Three members have p = 0 and rank first, so H5 is 4th of 13: its multiplier is 10.
    assert out["H5"]["outcome"] == "supported" and out["H5"]["p_holm"] == pytest.approx(10 * 6.1e-5)
    assert out["H1c CLINC150"]["outcome"] == "supported"
    assert out["H1c Intents-v0"]["outcome"] == "not supported"  # the CI lies wholly below -delta
    assert out["H2 Intents-v0"]["outcome"] == "inconclusive (interim)"  # CI straddles 0; H4/H6 still pending
    assert out["H2 CLINC150"]["outcome"] == "inconclusive (interim)"  # p 0.02 x 13 > 0.05
    assert out["H1 Intents-v0"]["outcome"] == "supported" and out["H3"]["outcome"] == "supported"
