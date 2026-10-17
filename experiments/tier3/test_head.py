import json
import struct

import numpy as np
import pytest

from head import (
    export_head,
    fit_temperature,
    load_head,
    logits,
    nll,
    softmax,
    train_head,
    write_parity_fixture,
)


def test_softmax_rows_sum_to_one_and_temperature_keeps_the_argmax():
    z = np.array([[1.0, 2.0, 3.0], [0.5, -1.0, 0.0]])
    for t in (0.5, 1.0, 3.0):
        p = softmax(z, t)
        assert np.allclose(p.sum(axis=1), 1.0)
        assert (p.argmax(axis=1) == z.argmax(axis=1)).all()
    assert softmax(z, 3.0)[0].max() < softmax(z, 1.0)[0].max()  # T > 1 softens


def test_softmax_is_stable_for_huge_logits():
    p = softmax(np.array([[1000.0, 0.0]]), 1.0)
    assert np.isfinite(p).all() and p[0, 0] == pytest.approx(1.0)


def test_fit_temperature_recovers_the_temperature_that_generated_the_labels():
    rng = np.random.default_rng(0)
    z = rng.normal(0.0, 3.0, size=(20000, 5))
    p = softmax(z, 2.0)  # the "true" model is z / 2, so the raw logits are overconfident
    y = np.array([rng.choice(5, p=row) for row in p])
    t = fit_temperature(z, y)
    assert t == pytest.approx(2.0, abs=0.1)
    assert nll(z, y, t) < nll(z, y, 1.0)


def test_fit_temperature_refuses_a_calibration_set_without_errors():
    # Every item right: the NLL keeps falling as T -> 0, so T is not identifiable from these data.
    # (A real risk for Kestrel-Intents-v0: 100 cal items, easy synthetic text.)
    z = np.array([[50.0, 0.0], [0.0, 50.0]] * 50)
    y = np.array([0, 1] * 50)
    with pytest.raises(ValueError, match="no errors"):
        fit_temperature(z, y)


def test_fit_temperature_refuses_an_optimum_at_the_search_bound():
    # Confident logits that carry no information: the best T is "infinite" (uniform output).
    rng = np.random.default_rng(1)
    z = rng.normal(0.0, 50.0, size=(2000, 4))
    y = rng.integers(0, 4, size=2000)
    with pytest.raises(ValueError, match="bound"):
        fit_temperature(z, y)


def _clusters(k=4, d=8, n=40, seed=0):
    rng = np.random.default_rng(seed)
    centers = rng.normal(0, 5, size=(k, d))
    y = np.repeat(np.arange(k), n)
    x = centers[y] + rng.normal(0, 0.5, size=(k * n, d))
    order = rng.permutation(len(y))  # classes do not appear in label order
    return x[order].astype(np.float32), y[order]


def test_train_head_returns_one_row_per_class_in_label_order():
    x, y = _clusters()
    w, b = train_head(x, y, n_classes=4, c=10.0)
    assert w.shape == (4, 8) and b.shape == (4,)
    assert w.dtype == np.float32 and b.dtype == np.float32
    assert (logits(x, w, b).argmax(axis=1) == y).all()


def test_train_head_needs_every_class_in_the_training_data():
    x, y = _clusters(k=3)
    with pytest.raises(ValueError, match="class 3"):
        train_head(x, y, n_classes=4, c=1.0)


def test_export_writes_little_endian_row_major_f32(tmp_path):
    w = np.array([[1.0, 2.0], [3.0, 4.0]], dtype=np.float32)
    b = np.array([0.5, -0.5], dtype=np.float32)
    export_head(tmp_path, w, b, 1.5, ["create_task", "search_notes"], {"encoder": "toy"})
    assert (tmp_path / "head.bin").read_bytes() == struct.pack("<4f", 1, 2, 3, 4)
    assert (tmp_path / "head_bias.bin").read_bytes() == struct.pack("<2f", 0.5, -0.5)
    meta = json.loads((tmp_path / "head.json").read_text())
    assert meta["classes"] == ["create_task", "search_notes"]
    assert meta["dim"] == 2 and meta["temperature"] == 1.5 and meta["encoder"] == "toy"
    assert meta["format"] == 1


@pytest.mark.parametrize("bad", ["t_zero", "t_nan", "w_nan", "classes", "bias"])
def test_export_rejects_heads_rust_could_not_use(tmp_path, bad):
    w = np.ones((2, 3), dtype=np.float32)
    b = np.zeros(2, dtype=np.float32)
    t, classes = 1.0, ["a", "b"]
    if bad == "t_zero":
        t = 0.0
    elif bad == "t_nan":
        t = float("nan")
    elif bad == "w_nan":
        w[0, 0] = np.nan
    elif bad == "classes":
        classes = ["a"]
    elif bad == "bias":
        b = np.zeros(3, dtype=np.float32)
    with pytest.raises(ValueError):
        export_head(tmp_path, w, b, t, classes, {})


def test_load_head_round_trips(tmp_path):
    x, y = _clusters()
    w, b = train_head(x, y, n_classes=4, c=1.0)
    export_head(tmp_path, w, b, 1.25, list("abcd"), {"seed": 0})
    w2, b2, t2, meta = load_head(tmp_path)
    assert np.array_equal(w, w2) and np.array_equal(b, b2)
    assert t2 == 1.25 and meta["classes"] == list("abcd") and meta["seed"] == 0


def test_parity_fixture_is_deterministic(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    write_parity_fixture(a)
    write_parity_fixture(b)
    for name in ("head.json", "head.bin", "head_bias.bin", "cases.json"):
        assert (a / name).read_bytes() == (b / name).read_bytes()
    cases = json.loads((a / "cases.json").read_text())
    assert len(cases["inputs"]) == len(cases["probs"]) == 100


def test_committed_parity_fixture_is_current(tmp_path):
    # datasets/contracts/head_parity/ is what the Rust LinearHead test reads; regenerate it with
    # `python experiments/tier3/head.py` if this fails after an intended change.
    from head import PARITY_DIR

    write_parity_fixture(tmp_path)
    for name in ("head.json", "head.bin", "head_bias.bin", "cases.json"):
        assert (tmp_path / name).read_bytes() == (PARITY_DIR / name).read_bytes(), name
