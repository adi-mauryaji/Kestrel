import statistics

import numpy as np
import pytest

from epsilon_pilot import bootstrap_scores, epsilons, week5_c


def _toy(seed=0, n=300, d=5, k=3):
    rng = np.random.default_rng(seed)
    centers = rng.normal(0, 1.5, size=(k, d))
    def draw(m):
        y = rng.integers(0, k, m)
        return centers[y] + rng.normal(0, 1.0, size=(m, d)), y
    return draw(n), draw(120), draw(150)


def test_bootstrap_retrains_are_seeded_and_scored_on_val():
    (xt, yt), (xc, yc), (xv, yv) = _toy()
    a = bootstrap_scores(xt, yt, xc, yc, xv, yv, n_classes=3, c=1.0, b=3, seed=7)
    assert a == bootstrap_scores(xt, yt, xc, yc, xv, yv, n_classes=3, c=1.0, b=3, seed=7)
    assert len(a) == 3 and all(set(s) == {"macro_f1", "ece", "accuracy", "temperature"} for s in a)
    assert len({s["macro_f1"] for s in a} | {s["ece"] for s in a}) > 1, "resamples differ"


def test_epsilons_are_twice_the_sample_sd():
    scores = [{"macro_f1": f, "ece": e, "accuracy": f, "temperature": 1.0}
              for f, e in [(0.95, 0.010), (0.96, 0.014), (0.94, 0.012), (0.955, 0.011)]]
    out = epsilons(scores)
    assert out["eps1_macro_f1"] == pytest.approx(2 * statistics.stdev([0.95, 0.96, 0.94, 0.955]))
    assert out["eps3_ece"] == pytest.approx(2 * statistics.stdev([0.010, 0.014, 0.012, 0.011]))
    assert out["n"] == 4


def test_the_week5_c_comes_from_the_log():
    log = [{"dataset": "clinc150", "encoder": "multilingual-e5-small-q8_0", "c": 100.0},
           {"dataset": "clinc150", "encoder": "bge-small-en-v1.5-q8_0", "c": 10.0}]
    assert week5_c(log) == 100.0
    with pytest.raises(LookupError):
        week5_c(log[1:])
