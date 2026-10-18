import copy

import pytest

from make_config import build, config_hash, verify

POLICIES = {
    "n_val": 128,
    "calibration": {"rule_precision_cal": 1.0, "t3": 0.69, "t4": 0.85},
    "cost_ms_mean": {"t1": 0.003, "t3": 5.8, "t4": 812.1},
    "R0": {"acc": 0.984},
    "R3": {
        "conjunction": {"points": {"0.0": {"acc": 0.984, "cost": 478.0, "tau": [0.0, 0.96, 0.0],
                                           "crossfit": {"acc": 0.953, "cost": 412.0}},
                                   "0.05": None}},
        "None": {"points": {"0.0": {"acc": 0.984, "cost": 737.0, "tau": [None, 0.999, 0.0],
                                    "crossfit": {"acc": 0.922, "cost": 613.0}}}},
    },
}
META = {"dataset": "intents_v0", "git_head": "abc1234",
        "tier3": {"encoder": "e5", "encoder_sha256": "e", "head": "h", "head_bin_sha256": "hb"},
        "tier4": {"model": "gemma", "model_file": "g.gguf", "prompt": "p.txt", "prompt_sha256": "ps",
                  "shots_per_label_and_language": 2}}
RUNTIME = {"encoder_file": "e5.gguf", "n_ctx": 2048, "shots": [{"text": "add a task", "label": "create_task"}]}


def test_build_turns_the_chosen_point_into_router_thresholds():
    cfg = build(POLICIES, META, "tier_outputs_sha", RUNTIME, signal="conjunction", delta=0.0)
    assert cfg["accept"] == [0.0, 1.0, 0.96, 0.0]  # index = Tier::index(); Tier 2 is not built
    assert cfg["tiers"] == [1, 3, 4]
    assert cfg["multi_intent_to_generative"] is True
    assert cfg["calibration"] == {"tier1_confidence": 1.0, "tier3_temperature": 0.69, "tier4_temperature": 0.85}
    assert cfg["chosen_on"]["delta"] == 0.0 and cfg["chosen_on"]["split"] == "val"
    assert "DEV" in cfg["status"]
    assert cfg["est_cost_ms"] == [1, 0, 6, 813]  # measured dev means, rounded up; Tier 2 not built
    assert cfg["models"]["tier3"]["encoder_file"] == "e5.gguf"
    assert cfg["models"]["tier4"]["n_ctx"] == 2048 and cfg["models"]["tier4"]["shots"] == RUNTIME["shots"]


def test_a_tier_switched_off_is_left_out_of_the_cascade():
    cfg = build(POLICIES, META, "s", RUNTIME, signal=None, delta=0.0)
    assert cfg["tiers"] == [3, 4]
    assert cfg["multi_intent_to_generative"] is False


def test_the_hash_covers_everything_but_itself():
    cfg = build(POLICIES, META, "s", RUNTIME, signal="conjunction", delta=0.0)
    assert cfg["config_sha256"] == config_hash(cfg) and verify(cfg)
    edited = copy.deepcopy(cfg)
    edited["accept"][2] = 0.5
    assert not verify(edited)


def test_build_refuses_a_missing_point_or_a_signal_the_router_lacks():
    with pytest.raises(ValueError):
        build(POLICIES, META, "s", RUNTIME, signal="conjunction", delta=0.05)
    with pytest.raises(ValueError):
        build(POLICIES, META, "s", RUNTIME, signal="ambiguous", delta=0.0)


def test_the_committed_config_is_current_and_unedited():
    import hashlib
    import json

    from make_config import LOG_DIR, OUT_DIR

    committed = json.loads((OUT_DIR / "router_intents_v0.json").read_text())
    assert verify(committed)
    outputs = LOG_DIR / "tier_outputs_intents_v0.jsonl"
    policies = json.loads((LOG_DIR / "policies_intents_v0.json").read_text())
    meta = json.loads(outputs.with_suffix(".meta.json").read_text())
    from make_config import runtime_inputs

    rebuilt = build(policies, meta, hashlib.sha256(outputs.read_bytes()).hexdigest(), runtime_inputs(meta),
                    signal="conjunction", delta=committed["chosen_on"]["delta"],
                    budget_ms=committed["latency_budget_ms"])
    assert rebuilt == committed
