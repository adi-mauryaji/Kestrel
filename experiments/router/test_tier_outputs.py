import numpy as np
import pytest

from tier_outputs import RULES_ITEMS, assemble, run_rules

ROWS = [{"id": "a/en/0/0", "text": "remind me to call asha", "label": "create_task", "lang": "en"},
        {"id": "b/hi-en/0/0", "text": "notes dhundo aur summary do", "label": "search_notes|summarize_notes",
         "lang": "hi-en"}]
T1 = [{"id": "a/en/0/0", "outcome": "match", "intent": "create_task", "rule": "remind", "conjunction": False, "us": 3.0},
      {"id": "b/hi-en/0/0", "outcome": "no_match", "conjunction": True, "us": 2.0}]
T4 = [{"id": "a/en/0/0", "label": "create_task", "steps": [], "ms": 500.0},
      {"id": "b/hi-en/0/0", "label": "search_notes", "steps": [], "ms": 600.0}]


def test_assemble_keeps_row_order_and_every_tier():
    z3 = np.array([[1.0, 0.0], [0.0, 2.0]])
    recs = assemble("val", ROWS, T1, z3, [8.0, 9.0], T4)
    assert [r["id"] for r in recs] == ["a/en/0/0", "b/hi-en/0/0"]
    a, b = recs
    assert a["split"] == "val" and a["gold"] == "create_task" and a["lang"] == "en" and not a["compound"]
    assert b["compound"] and b["gold"] == "search_notes|summarize_notes"
    assert a["t1"] == {k: v for k, v in T1[0].items() if k != "id"}
    assert b["t3"] == {"logits": [0.0, 2.0], "ms": 9.0}
    assert b["t4"] == {"label": "search_notes", "steps": [], "ms": 600.0}


def test_assemble_refuses_tier_outputs_for_other_items():
    z3 = np.zeros((2, 2))
    with pytest.raises(ValueError):
        assemble("val", ROWS, T1[::-1], z3, [1.0, 1.0], T4)
    with pytest.raises(ValueError):
        assemble("val", ROWS, T1, z3, [1.0, 1.0], T4[:1])
    with pytest.raises(ValueError):
        assemble("val", ROWS, T1, z3[:1], [1.0, 1.0], T4)


@pytest.mark.skipif(not RULES_ITEMS.exists(), reason="cargo build --release -p kestrel-nlu --example rules_items")
def test_rules_runner_normalizes_like_a_request():
    out = run_rules([{"id": "x", "text": "  Remind me to call Asha tomorrow. "},
                     {"id": "y", "text": "Summarize my notes and create a task"}])
    assert out[0]["outcome"] == "match" and out[0]["intent"] == "create_task" and not out[0]["conjunction"]
    assert out[1]["outcome"] == "ambiguous" and out[1]["conjunction"]
    assert all(r["us"] >= 0 for r in out)


def test_signal_rates_count_by_kind_and_language():
    from signal_check import rates

    rows = [{"label": "a", "lang": "en"}, {"label": "a", "lang": "hi-en"}, {"label": "a|b", "lang": "en"},
            {"label": "oos"}]
    r = rates(rows, [True, False, True, False], "oos")
    assert r["single"] == {"n": 2, "flagged": 1, "rate": 0.5}
    assert r["single/hi-en"]["flagged"] == 0 and r["compound"]["rate"] == 1.0
    assert r["oos/en"] == {"n": 1, "flagged": 0, "rate": 0.0}  # no lang field: one language


def test_assemble_records_a_family_and_defaults_the_language():
    rows = [{"id": "clinc150/val/1", "text": "x", "label": "a"},
            {"id": "create-12/en/0/0", "text": "y", "label": "create_task", "lang": "en", "family": "create-12"}]
    t1 = [{"id": r["id"], "outcome": "no_match", "conjunction": False, "us": 1.0} for r in rows]
    t4 = [{"id": r["id"], "label": "a", "steps": [], "ms": 1.0} for r in rows]
    recs = assemble("val", rows, t1, np.zeros((2, 2)), [1.0, 1.0], t4)
    assert [r["family"] for r in recs] == ["clinc150/val/1", "create-12"]
    assert [r["lang"] for r in recs] == ["en", "en"]


def test_a_test_run_takes_tier4s_settings_from_the_frozen_config():
    from tier_outputs import CONTRACTS, frozen_tier4
    cfg = {"models": {"tier4": {"prompt": "tier4_intent_prompt.txt", "n_ctx": 2048,
                                "shots": [{"text": "open my notes", "label": "read_document"}]}}}
    prompt, n_ctx, shots = frozen_tier4(cfg)
    assert prompt == CONTRACTS / "tier4_intent_prompt.txt" and n_ctx == 2048
    assert shots == [{"text": "open my notes", "label": "read_document"}]
