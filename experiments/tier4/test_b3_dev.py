import math

import numpy as np
import pytest

from b3_dev import fit_conf_temperature, step_confidence


def _step(logits, chosen):
    return {"allowed": list(range(len(logits))), "logits": logits, "chosen": chosen}


def test_step_confidence_is_the_product_of_restricted_softmaxes():
    # Same case as the Rust labels test: logits [2, 1] pick 0, then [0, 1] pick 1, then a forced step.
    steps = [_step([2.0, 1.0], 0), _step([0.0, 1.0], 1), _step([0.0], 0)]
    p1 = math.exp(2) / (math.exp(2) + math.exp(1))
    p2 = math.exp(1) / (1 + math.exp(1))
    assert step_confidence(steps) == pytest.approx(p1 * p2)
    assert step_confidence(steps, 3.0) < step_confidence(steps)
    assert step_confidence(steps, 1e6) == pytest.approx(0.25, abs=1e-3)


def test_fit_conf_temperature_recovers_the_generating_temperature():
    rng = np.random.default_rng(0)
    steps, correct = [], []
    for _ in range(20000):
        z = rng.normal(0, 3, size=2).tolist()
        chosen = int(np.argmax(z))
        p = step_confidence([_step(z, chosen)], 2.0)  # the truth: P(correct) = softmax(z / 2)
        steps.append([_step(z, chosen)])
        correct.append(bool(rng.random() < p))
    assert fit_conf_temperature(steps, correct) == pytest.approx(2.0, abs=0.15)


def test_fit_conf_temperature_needs_both_outcomes():
    steps = [[_step([3.0, 0.0], 0)]] * 10
    with pytest.raises(ValueError, match="both"):
        fit_conf_temperature(steps, [True] * 10)


def test_select_shots_takes_k_per_label_and_language_from_distinct_train_families():
    from b3_dev import select_shots
    from splits import load_splits

    train = load_splits("intents_v0")["train"]
    labels = sorted({r["label"] for r in train})
    shots = select_shots(train, labels, k=2)
    assert len(shots) == len(labels) * 2 * 2
    by_text = {r["text"]: r for r in train}
    fams = {}
    for s in shots:
        r = by_text[s["text"]]  # every shot is a training item, with its own label
        assert r["label"] == s["label"]
        fams.setdefault((s["label"], r["lang"]), []).append(r["family"])
    assert all(len(set(f)) == 2 for f in fams.values())
    # Round-robin: the first len(labels) shots already cover every label once.
    assert {s["label"] for s in shots[: len(labels)]} == set(labels)


def test_select_shots_works_without_language_or_family_fields():
    # CLINC150 rows carry neither: one language, every item its own "family".
    from b3_dev import select_shots

    rows = [{"id": f"x/{lab}/{i}", "text": f"{lab} {i}", "label": lab} for lab in ("b", "a") for i in range(3)]
    shots = select_shots(rows, ["a", "b"], k=2)
    assert [s["label"] for s in shots] == ["a", "b", "a", "b"]
    assert [s["text"] for s in shots] == ["a 0", "b 0", "a 1", "b 1"]


def test_subset_is_a_fixed_seeded_sample():
    from b3_dev import subset

    rows = [{"id": f"r{i}"} for i in range(1000)]
    a, b = subset(rows, 300, seed=0), subset(rows, 300, seed=0)
    assert a == b and len(a) == 300 and len({r["id"] for r in a}) == 300
    assert subset(rows, 300, seed=1) != a
    assert subset(rows, 5000, seed=0) == rows  # asking for more than exists keeps everything, in order


def test_log_name_records_subset_and_a_prompt_override():
    from b3_dev import TASKS, log_name

    default = TASKS["clinc150"][0]
    assert log_name("clinc150", "g", 1, 300, default) == "b3_dev_clinc150_g_k1_n300.json"
    other = default.with_name("tier4_clinc150_desc_prompt.txt")
    assert log_name("clinc150", "g", 1, 300, other) == "b3_dev_clinc150_g_k1_n300_tier4_clinc150_desc_prompt.json"
    assert log_name("intents_v0", "g", 2, None, TASKS["intents_v0"][0]) == "b3_dev_intents_v0_g_k2.json"
