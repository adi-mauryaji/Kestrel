import copy
import gzip
import sys
from pathlib import Path

import pytest

from make_config import build, config_hash, outputs_sha256, verify

POLICIES = {
    "n_val": 128,
    "calibration": {"rule_precision_cal": 1.0, "t3": 0.69, "t4": 0.85},
    "cost_ms_mean": {"t1": 0.003, "t3": 5.8, "t4": 812.1},
    "R0": {"acc": 0.984},
    "R0_tuned": {"acc": 0.98, "cost": 812.0, "tau": [None, None, 0.58]},
    "R1": {"tuned": {"acc": 0.91, "cost": 5.6, "tau": [None, 0.78, None]}},
    "reference": {"system": "R0_tuned", "acc": 0.984},
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
RUNTIME = {"encoder_file": "e5.gguf", "n_ctx": 2048, "labels": "intents.txt",
           "shots": [{"text": "add a task", "label": "create_task"}]}


def test_build_turns_the_chosen_point_into_router_thresholds():
    cfg = build(POLICIES, META, "tier_outputs_sha", RUNTIME, signal="conjunction", delta=0.0)
    assert cfg["accept"] == [0.0, 1.0, 0.96, 0.0]  # index = Tier::index(); Tier 2 is not built
    assert cfg["tiers"] == [1, 3, 4]
    assert cfg["multi_intent_to_generative"] is True
    assert cfg["calibration"] == {"tier1_confidence": 1.0, "tier3_temperature": 0.69, "tier4_temperature": 0.85}
    assert cfg["chosen_on"]["delta"] == 0.0 and cfg["chosen_on"]["split"] == "val"
    assert cfg["chosen_on"]["reference"] == {"system": "R0_tuned", "acc": 0.984}  # ADR-019
    assert cfg["status"].startswith("CANDIDATE")
    assert cfg["est_cost_ms"] == [1, 0, 6, 813]  # measured dev means, rounded up; Tier 2 not built
    assert cfg["models"]["tier3"]["encoder_file"] == "e5.gguf"
    assert cfg["models"]["tier4"]["n_ctx"] == 2048 and cfg["models"]["tier4"]["shots"] == RUNTIME["shots"]
    assert cfg["models"]["tier4"]["labels"] == "intents.txt"  # Tier 4's answers, ADR-019


@pytest.mark.parametrize("policy, tiers, accept", [("R0", [4], [1.0, 1.0, 1.0, 0.58]),
                                                   ("R1", [3], [1.0, 1.0, 0.78, 1.0])])
def test_r0_and_r1_configs_are_one_tier_with_its_tuned_threshold(policy, tiers, accept):
    # ADR-019/020: E1 runs the baselines from their own configs, sharing every model field with R3.
    cfg = build(POLICIES, META, "s", RUNTIME, signal="conjunction", delta=0.0, policy=policy)
    r3 = build(POLICIES, META, "s", RUNTIME, signal="conjunction", delta=0.0)
    assert cfg["policy"] == policy and cfg["tiers"] == tiers and cfg["accept"] == accept
    assert cfg["multi_intent_to_generative"] is False  # one tier: there is nothing to skip
    assert cfg["models"] == r3["models"] and cfg["calibration"] == r3["calibration"]
    assert cfg["est_cost_ms"] == r3["est_cost_ms"]
    assert cfg["chosen_on"]["rule"].startswith("tuned_single") and "delta" not in cfg["chosen_on"]
    assert verify(cfg)


def test_build_refuses_an_unknown_policy():
    with pytest.raises(ValueError, match="policy"):
        build(POLICIES, META, "s", RUNTIME, signal="conjunction", delta=0.0, policy="R2")


def test_the_outputs_hash_is_of_the_content_whether_plain_or_gzipped(tmp_path):
    data = b'{"id": "a"}\n'
    (tmp_path / "plain.jsonl").write_bytes(data)
    with gzip.open(tmp_path / "packed.jsonl.gz", "wb") as f:
        f.write(data)
    assert outputs_sha256(tmp_path / "plain.jsonl") == outputs_sha256(tmp_path / "packed.jsonl.gz")


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


@pytest.mark.parametrize("dataset", ["intents_v0", "clinc150"])
def test_the_committed_config_is_current_and_unedited(dataset):
    import json

    from make_config import OUT_DIR, inputs, runtime_inputs

    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from freeze import freeze_config  # the committed configs are frozen (ADR-031): only status and hash differ

    committed = json.loads((OUT_DIR / f"router_{dataset}.json").read_text())
    assert verify(committed)
    policies, meta, outputs = inputs(dataset)
    rebuilt = build(policies, meta, outputs_sha256(outputs), runtime_inputs(meta),
                    signal="conjunction", delta=committed["chosen_on"]["delta"],
                    budget_ms=committed["latency_budget_ms"])
    assert freeze_config(rebuilt) == committed
    for policy in ("R0", "R1"):
        baseline = json.loads((OUT_DIR / f"router_{dataset}_{policy.lower()}.json").read_text())
        assert verify(baseline)
        assert freeze_config(build(policies, meta, outputs_sha256(outputs), runtime_inputs(meta),
                                   signal="conjunction", delta=committed["chosen_on"]["delta"],
                                   budget_ms=committed["latency_budget_ms"], policy=policy)) == baseline
