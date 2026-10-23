import math

import numpy as np
import pytest

from policies import (OFF, Answers, best_point, correct, label_classes, r_rand, reference_acc, route, rule_precision,
                      sweep_shared, tuned_single)

CLASSES = ["create_task", "search_notes", "unsupported"]


def _rec(gold, t1=None, t3=(0.0, 0.0, 0.0), t4="create_task", conj=False, lang="en", ms=(0.001, 10.0, 500.0),
         t4_logit=2.0):
    t1d = {"outcome": "match", "intent": t1} if t1 else {"outcome": "no_match"}
    t1d |= {"conjunction": conj, "us": ms[0] * 1e3}
    t4d = {"error": "x"} if t4 is None else {"label": t4, "steps": [{"allowed": [0, 1], "logits": [t4_logit, 0.0],
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
    assert r.tier.tolist() == [4] and r.cost == pytest.approx([500.0])  # nothing below Tier 4 is paid


def test_scoring_rules_for_in_scope_out_of_scope_and_compound_items():
    recs = [_rec("create_task"), _rec("create_task"), _rec("unsupported"), _rec("unsupported"),
            _rec("search_notes"), _rec("create_task|search_notes"), _rec("create_task|search_notes")]
    a = _answers(recs)
    pred = np.array([0, -1, -1, 2, 0, 0, 0])
    tier = np.array([3, 4, 4, 4, 3, 3, 4])
    # in scope: right / abstained; OOS: abstained / predicted unsupported; wrong label;
    # compound accepted below Tier 4 (wrong) / reaching Tier 4 (right, ADR-010).
    assert correct(a, pred, tier).tolist() == [True, False, True, True, False, False, True]


def test_a_single_tier_baseline_abstains_where_that_raises_accuracy():
    # Tier 4 is right and sure on two items, wrong and unsure on an out-of-scope one (ADR-019).
    recs = [_rec("create_task", t4_logit=3.0), _rec("create_task", t4_logit=2.0), _rec("unsupported", t4_logit=0.5)]
    a = _answers(recs)
    p = tuned_single(a, 4)
    assert p["acc"] == 1.0 and p["coverage"] == pytest.approx(2 / 3)
    assert p["tau"][:2] == [None, None] and a.c4[2] < p["tau"][2] < a.c4[1]


def test_a_single_tier_baseline_keeps_answering_when_abstaining_does_not_help():
    # The unsure in-scope item is wrong whether Tier 3 answers or abstains, so coverage decides.
    recs = [_rec("search_notes", t3=(0.0, 2.0, 0.0)), _rec("search_notes", t3=(0.0, 0.5, 0.0)),
            _rec("create_task", t3=(0.0, 0.1, 0.0))]
    p = tuned_single(_answers(recs), 3)
    assert p["acc"] == pytest.approx(2 / 3) and p["coverage"] == 1.0 and p["tau"] == [None, 0.0, None]


def test_the_operating_point_reference_is_the_better_tuned_single_tier():
    # Tier 3 is right on both items; Tier 4 is wrong on the second, in scope, so abstaining cannot help it.
    recs = [_rec("search_notes", t3=(0.0, 2.0, 0.0), t4="search_notes"),
            _rec("create_task", t3=(2.0, 0.0, 0.0), t4="search_notes")]
    a = _answers(recs)
    assert tuned_single(a, 4)["acc"] == 0.5 and tuned_single(a, 3)["acc"] == 1.0
    assert reference_acc(a) == 1.0


def test_label_classes_put_the_oos_label_after_the_head_classes_when_the_head_lacks_it():
    assert label_classes({"tier3": {"classes": ["a", "unsupported"]}}, "intents_v0") == ["a", "unsupported"]
    assert label_classes({"tier3": {"classes": ["a", "b"]}}, "clinc150") == ["a", "b", "oos"]


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


def test_candidate_thresholds_fall_between_observed_confidences():
    # Midpoints, so f32 vs f64 rounding cannot move any item across a chosen threshold.
    from policies import OFF, _grid

    assert _grid(np.array([0.9, 0.2, 0.5, 0.5])).tolist() == pytest.approx([0.0, 0.35, 0.7, OFF])
    assert _grid(np.array([0.4])).tolist() == [0.0, OFF]


def test_clinc_style_answers_index_oos_after_the_head_classes_and_ignore_foreign_rule_labels():
    head = ["a", "b"]  # CLINC150's head has no oos class; Tier 4 can still answer oos
    recs = [_rec("oos", t4="oos"), _rec("a", t1="create_task", t4="a")]
    for r in recs:
        r["t3"]["logits"] = [1.0, 0.0]
    a = Answers.build(recs, head + ["oos"], rule_conf=1.0, t3=1.0, t4=1.0, oos_label="oos")
    assert a.oos.tolist() == [True, False] and a.oos_idx == 2
    assert a.y4.tolist() == [2, 0]
    assert not a.m1.any()  # a Kestrel rule label is not a CLINC150 class: Tier 1 has no opinion
    r = route(a, (OFF, 0.99, 0.0), tiers=(3, 4))
    assert correct(a, r.pred, r.tier).tolist() == [True, True]


def test_families_come_from_the_record_else_the_id_prefix_of_older_logs():
    recs = [_rec("a", t4="a"), _rec("b", t4="b")]
    recs[0]["id"], recs[1]["id"] = "create-12/en/0/0", "clinc150/val/2"
    recs[1]["family"] = "clinc150/val/2"
    a = Answers.build(recs, ["a", "b"], rule_conf=1.0, t3=1.0, t4=1.0)
    assert a.family.tolist() == ["create-12", "clinc150/val/2"]


def test_thresholds_of_tiers_left_out_are_reported_off():
    from policies import summary, sweep_per_tier

    a = _answers([_rec("create_task", t3=(5.0, 0.0, 0.0)), _rec("search_notes")])
    s = summary(a, route(a, (0.0, 0.5, 0.0), tiers=(3, 4)), (0.0, 0.5, 0.0))
    assert s["tau"] == [None, 0.5, 0.0]
    assert all(p["tau"][0] is None for p in sweep_per_tier(a, tiers=(3, 4)))


def _random_answers(seed, n=70):
    rng = np.random.default_rng(seed)
    golds = ["create_task", "search_notes", "unsupported", "create_task|search_notes"]
    recs = []
    for i in range(n):
        gold = golds[rng.integers(4)]
        t4 = None if rng.random() < 0.05 else CLASSES[rng.integers(3)]
        r = _rec(gold, t1="create_task" if rng.random() < 0.2 else None, t3=tuple(rng.normal(size=3) * 2), t4=t4,
                 conj=bool(rng.random() < 0.15), ms=(0.001, 5 + rng.random(), 500 + 100 * rng.random()))
        if t4 is not None:
            r["t4"]["steps"][0]["logits"] = [float(rng.normal() * 2), 0.0]
        recs.append(r)
    return _answers(recs, rule_conf=0.9)


@pytest.mark.parametrize("seed", [0, 1, 2])
@pytest.mark.parametrize("tiers", [(1, 3, 4), (3, 4)])
@pytest.mark.parametrize("signal", [None, "conjunction"])
def test_fast_per_tier_search_chooses_what_the_full_grid_chooses(seed, tiers, signal):
    from policies import best_per_tier, risk_at_coverage, sweep_per_tier

    a = _random_answers(seed)
    pts = sweep_per_tier(a, signal, tiers)
    r0 = max(p["acc"] for p in pts)
    floors = {d: r0 - d for d in (0.0, 0.02, 0.05, 0.2)}
    fast, risk = best_per_tier(a, floors, signal, tiers)
    for d, floor in floors.items():
        slow = best_point(pts, floor)
        assert (fast[d] is None) == (slow is None)
        if slow is not None:
            assert fast[d]["tau"] == slow["tau"] and fast[d]["acc"] == slow["acc"]
            assert fast[d]["cost"] == pytest.approx(slow["cost"])
    assert risk == risk_at_coverage(pts)


def test_records_are_read_from_plain_or_gzipped_logs(tmp_path):
    import gzip
    import json

    from policies import read_tier_outputs

    rec = {"id": "x", "split": "val"}
    (tmp_path / "tier_outputs_a.jsonl").write_text(json.dumps(rec) + "\n")
    with gzip.open(tmp_path / "tier_outputs_b.jsonl.gz", "wt") as f:
        f.write(json.dumps(rec) + "\n")
    for name in ("a", "b"):
        (tmp_path / f"tier_outputs_{name}.meta.json").write_text('{"dataset": "%s"}' % name)
        recs, meta, path = read_tier_outputs(name, tmp_path)
        assert recs == [rec] and meta == {"dataset": name} and path.exists()
    with pytest.raises(FileNotFoundError):
        read_tier_outputs("c", tmp_path)


@pytest.mark.parametrize("seed", [0, 1])
@pytest.mark.parametrize("signal", [None, "conjunction"])
def test_a_tier_sets_point_is_the_full_grids_cheapest_point_above_the_floor(seed, signal):
    # E3 (H3): each tier set gets the operating-point rule of R3 (cheapest at accuracy >= floor).
    from policies import sweep_per_tier, tier_set_point

    a = _random_answers(seed)
    for tiers in [(4,), (1, 4), (3, 4), (1, 3, 4)]:
        pts = sweep_per_tier(a, signal, tiers)
        top = max(p["acc"] for p in pts)
        for floor in (top, top - 0.05, top + 0.01):
            slow = best_point(pts, floor)
            fast = tier_set_point(a, floor, signal, tiers)
            assert (fast is None) == (slow is None), (tiers, floor)
            if slow is not None:
                assert (fast["tau"], fast["acc"]) == (slow["tau"], slow["acc"]), tiers
                assert fast["cost"] == pytest.approx(slow["cost"])


def test_the_best_reachable_point_is_the_highest_floor_a_tier_set_meets():
    from policies import best_reachable, sweep_per_tier

    a = _random_answers(3)
    pts = sweep_per_tier(a, None, (1, 4))
    top = max(p["acc"] for p in pts)
    p = best_reachable(a, None, (1, 4), start=top + 0.05, step=0.005)
    assert p["acc"] >= top - 0.005 - 1e-12 and p == best_point(pts, p["floor"]) | {"floor": p["floor"]}
