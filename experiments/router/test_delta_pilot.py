import pytest

from delta_pilot import SESOI, delta, pilot_records, r0_scores, with_t4

CLASSES = ["create_task", "search_notes", "unsupported"]


def _t4(label, logit):
    return {"label": label, "steps": [{"allowed": [0, 1], "logits": [logit, 0.0], "chosen": 0}], "ms": 500.0}


def _rec(split, i, gold, t4):
    return {"split": split, "id": f"{split}{i}", "gold": gold, "lang": "en", "compound": False,
            "t1": {"outcome": "no_match", "conjunction": False, "us": 1.0},
            "t3": {"logits": [0.0, 0.0, 0.0], "ms": 5.0}, "t4": t4}


def test_pilot_records_keep_a_fixed_quarter_of_each_split_in_order():
    recs = [_rec("cal", i, "create_task", {}) for i in range(40)] + [_rec("val", i, "create_task", {})
                                                                      for i in range(100)]
    p = pilot_records(recs)
    assert sum(r["split"] == "cal" for r in p) == 10 and sum(r["split"] == "val" for r in p) == 25
    assert p == pilot_records(recs)
    pos = {r["id"]: i for i, r in enumerate(recs)}
    assert [pos[r["id"]] for r in p] == sorted(pos[r["id"]] for r in p)  # original order kept
    assert {r["id"] for r in p} != {r["id"] for r in recs[:10] + recs[40:65]}  # not just the first ones


def test_with_t4_swaps_in_a_seeds_tier4_outputs_and_checks_alignment():
    recs = [_rec("val", 0, "create_task", _t4("create_task", 2.0)), _rec("val", 1, "search_notes", {})]
    new = [{"id": "val0", **_t4("search_notes", 1.0)}, {"id": "val1", "error": "control token"}]
    out = with_t4(recs, new)
    assert out[0]["t4"]["label"] == "search_notes" and out[1]["t4"] == {"error": "control token"}
    assert recs[0]["t4"]["label"] == "create_task"  # the logged records are not modified
    with pytest.raises(ValueError, match="aligned"):
        with_t4(recs, new[::-1])


def test_r0_scores_tune_one_abstention_threshold_on_val():
    # All of cal right: no temperature can be fitted, so T = 1 is kept and noted.
    cal = [_rec("cal", i, "create_task", _t4("create_task", 3.0)) for i in range(4)]
    # val: three confident right answers, and one unsure wrong answer on an out-of-scope request.
    val = [_rec("val", i, "create_task", _t4("create_task", 3.0)) for i in range(3)]
    val.append(_rec("val", 3, "unsupported", _t4("search_notes", -1.0)))
    s = r0_scores(cal + val, CLASSES, "unsupported")
    assert s["temperature"] == 1.0 and "NOT FITTED" in s["temperature_note"]
    assert s["untuned_acc"] == 0.75  # always answering gets the OOS item wrong
    assert s["tuned_acc"] == 1.0 and s["tuned_coverage"] == 0.75  # abstaining on it is right (D3)
    assert 0.27 < s["tuned_tau"] < 0.95


def test_delta_is_twice_the_seed_sd_or_the_sesoi_whichever_is_larger():
    wide = delta([0.84, 0.87, 0.90])
    assert wide["sd"] == pytest.approx(0.03) and wide["delta"] == pytest.approx(0.06)
    assert wide["n_seeds"] == 3 and wide["mean"] == pytest.approx(0.87)
    narrow = delta([0.870, 0.871, 0.872])
    assert narrow["delta"] == SESOI == 0.02 and narrow["binding"] == "sesoi"
    assert wide["binding"] == "2sd"
    with pytest.raises(ValueError, match="seeds"):
        delta([0.9, 0.9])  # the blueprint asks for >= 3 seeds
