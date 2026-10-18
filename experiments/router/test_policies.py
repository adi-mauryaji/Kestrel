import math

import numpy as np
import pytest

from policies import Answers, best_point, correct, r_rand, route, rule_precision, sweep_shared

CLASSES = ["create_task", "search_notes", "unsupported"]


def _rec(gold, t1=None, t3=(0.0, 0.0, 0.0), t4="create_task", conj=False, lang="en", ms=(0.001, 10.0, 500.0)):
    t1d = {"outcome": "match", "intent": t1} if t1 else {"outcome": "no_match"}
    t1d |= {"conjunction": conj, "us": ms[0] * 1e3}
    t4d = {"error": "x"} if t4 is None else {"label": t4, "steps": [{"allowed": [0, 1], "logits": [2.0, 0.0],
                                                                    "chosen": 0}], "ms": ms[2]}
    return {"split": "val", "id": f"i{np.random.randint(1e9)}", "gold": gold, "lang": lang, "compound": "|" in gold,
            "t1": t1d, "t3": {"logits": list(t3), "ms": ms[1]}, "t4": t4d}


def _answers(recs, rule_conf=0.9, t3=1.0, t4=1.0):
    return Answers.build(recs, CLASSES, rule_conf=rule_conf, t3=t3, t4=t4)


def test_answers_take_each_tiers_top_label_and_confidence():
    a = _answers([_rec("create_task", t1="create_task", t3=(0.0, math.log(3), 0.0))])
    assert a.y1[0] == 0 and a.c1[0] == 0.9
    assert a.y3[0] == 1 and a.c3[0] == pytest.approx(0.6)  # softmax(0, ln 3, 0) = (.2, .6, .2)
    assert a.y4[0] == 0 and a.c4[0] == pytest.approx(math.exp(2) / (math.exp(2) + 1))
    assert (a.k1[0], a.k3[0], a.k4[0]) == pytest.approx((0.001, 10.0, 500.0))


def test_route_accepts_at_the_first_confident_tier_and_pays_for_each_consulted_one():
    recs = [_rec("create_task", t1="create_task"),            # Tier 1 accepts
            _rec("search_notes", t3=(0.0, 5.0, 0.0)),         # Tier 3 accepts
            _rec("search_notes", t3=(0.0, 0.1, 0.0))]         # Tier 3 unsure -> Tier 4
    r = route(_answers(recs), (0.5, 0.8, 0.0))
    assert r.pred.tolist() == [0, 1, 0]
    assert r.tier.tolist() == [1, 3, 4]
    assert r.cost == pytest.approx([0.001, 10.001, 510.001])


def test_a_tier_left_out_is_skipped_and_costs_nothing():
    r = route(_answers([_rec("create_task", t1="create_task")]), (0.5, 0.8, 0.0), tiers=(3, 4))
    assert r.tier.tolist() == [4] and r.cost == pytest.approx([510.0])


def test_low_confidence_at_the_last_tier_abstains():
    r = route(_answers([_rec("search_notes")]), (0.5, 0.8, 0.99))
    assert r.pred.tolist() == [-1] and r.tier.tolist() == [4]


def test_a_failed_tier_4_abstains():
    r = route(_answers([_rec("search_notes", t4=None)]), (0.5, 0.8, 0.0))
    assert r.pred.tolist() == [-1]


def test_the_conjunction_signal_sends_a_request_straight_to_tier_4():
    recs = [_rec("create_task|search_notes", t1="create_task", t3=(9.0, 0.0, 0.0), conj=True)]
    a = _answers(recs)
    assert route(a, (0.5, 0.8, 0.0)).tier.tolist() == [1]
    r = route(a, (0.5, 0.8, 0.0), signal="conjunction")
    assert r.tier.tolist() == [4] and r.cost == pytest.approx([500.001])  # Tier 3 not paid


def test_scoring_rules_for_in_scope_out_of_scope_and_compound_items():
    recs = [_rec("create_task"), _rec("create_task"), _rec("unsupported"), _rec("unsupported"),
            _rec("search_notes"), _rec("create_task|search_notes"), _rec("create_task|search_notes")]
    a = _answers(recs)
    pred = np.array([0, -1, -1, 2, 0, 0, 0])
    tier = np.array([3, 4, 4, 4, 3, 3, 4])
    # in scope: right / abstained; OOS: abstained / predicted unsupported; wrong label;
    # compound accepted below Tier 4 (wrong) / reaching Tier 4 (right, ADR-010).
    assert correct(a, pred, tier).tolist() == [True, False, True, True, False, False, True]


def test_rule_precision_is_measured_on_matches_only():
    recs = [_rec("create_task", t1="create_task"), _rec("search_notes", t1="create_task"), _rec("search_notes")]
    assert rule_precision(recs) == (0.5, 2)
    assert rule_precision([_rec("search_notes")]) == (None, 0)


def test_best_point_is_the_cheapest_meeting_the_accuracy_floor():
    pts = [{"acc": 0.95, "cost": 300.0}, {"acc": 0.97, "cost": 400.0}, {"acc": 0.97, "cost": 350.0},
           {"acc": 0.99, "cost": 600.0}]
    assert best_point(pts, floor=0.96) == {"acc": 0.97, "cost": 350.0}
    assert best_point(pts, floor=0.995) is None


def test_random_escalation_matches_the_counts_and_is_exact_when_nothing_is_random():
    recs = [_rec("create_task", t3=(5.0, 0.0, 0.0)), _rec("search_notes", t3=(0.0, 0.2, 0.0)),
            _rec("search_notes", t3=(0.0, 0.1, 0.0)), _rec("create_task", t3=(0.0, 0.3, 0.0))]
    a = _answers(recs)
    det = route(a, (0.5, 0.8, 0.0))
    rr = r_rand(a, det, seeds=200)
    assert rr["tier4_rate"] == pytest.approx(np.mean(det.tier == 4))
    # Tier 3 accepts 1 of 4 at random: expected accuracy = mean over which item is accepted.
    acc_if = []
    for k in range(4):
        pred = np.where(np.arange(4) == k, a.y3, a.y4)
        tier = np.where(np.arange(4) == k, 3, 4)
        acc_if.append(correct(a, pred, tier).mean())
    assert rr["acc"] == pytest.approx(np.mean(acc_if), abs=0.05)
    all_t4 = route(a, (0.5, 1.01, 0.0))
    assert r_rand(a, all_t4, seeds=10)["acc"] == pytest.approx(correct(a, all_t4.pred, all_t4.tier).mean())


def test_random_escalation_keeps_the_routing_signal():
    recs = [_rec("create_task|search_notes", t3=(9.0, 0.0, 0.0), conj=True),
            _rec("create_task", t3=(9.0, 0.0, 0.0)), _rec("search_notes", t3=(0.0, 0.1, 0.0))]
    a = _answers(recs)
    det = route(a, (0.5, 0.8, 0.0), signal="conjunction")
    assert det.tier.tolist() == [4, 3, 4]
    rr = r_rand(a, det, seeds=50)
    # Tier 3 accepts item 1 (as det: 2/3 right) or item 2 (then all 3 are right); item 0 stays at Tier 4.
    assert rr["acc"] == pytest.approx((2 / 3 + 1) / 2, abs=0.1)
    assert rr["tier4_rate"] == pytest.approx(2 / 3)


def test_a_shared_threshold_either_binds_tier_4_too_or_leaves_it_answering():
    recs = [_rec("search_notes", t3=(0.0, 0.1, 0.0)), _rec("create_task", t3=(5.0, 0.0, 0.0))]
    a = _answers(recs)
    every = sweep_shared(a)
    assert any(p["coverage"] < 1 for p in every)  # high tau: Tier 4 abstains too
    last = sweep_shared(a, last_answers=True)
    assert all(p["coverage"] == 1 and p["tau"][2] == 0.0 for p in last)
