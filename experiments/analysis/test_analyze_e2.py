import numpy as np
import pytest

from analyze_e2 import LEVELS, curve_stats, ece_gap, outcomes, risk_levels, tier_risk_at
from policies import Answers, risk_at_coverage, sweep_shared


def toy(n=60, seed=0):
    """Answers for a 3-tier cascade on n items: random labels and confidences, a few out-of-scope items."""
    rng = np.random.default_rng(seed)
    gold = rng.integers(0, 3, n)
    oos = rng.random(n) < 0.1
    gold = np.where(oos, 3, gold)
    pick = lambda p: np.where(rng.random(n) < p, gold, rng.integers(0, 3, n))  # noqa: E731
    m1 = rng.random(n) < 0.3
    z = np.zeros(n)
    return Answers(gold=gold, compound=np.zeros(n, bool), oos=oos, conj=np.zeros(n, bool), amb=np.zeros(n, bool),
                   lang=np.array(["en"] * n), family=np.arange(n).astype(str), m1=m1, y1=np.where(m1, pick(0.9), -1),
                   c1=np.where(m1, 0.95, 0.0), y3=pick(0.7), c3=rng.random(n), m4=np.ones(n, bool), y4=pick(0.85),
                   c4=rng.random(n), k1=z, k3=z + 5, k4=z + 800, oos_idx=3)


def test_risk_levels_follow_d4_and_fall_back_to_the_largest_coverage():
    cov = np.array([[1.0, 0.85, 0.8, 0.6]])
    risk = np.array([[0.30, 0.20, 0.10, 0.05]])
    assert risk_levels(cov, risk, np.array([0.8, 0.82, 0.5]))[0].tolist() == [0.10, 0.20, 0.05]
    tie = risk_levels(np.array([[0.9, 0.9]]), np.array([[0.4, 0.2]]), np.array([0.85]))
    assert tie[0, 0] == pytest.approx(0.2)  # equal coverage: the lower risk, as policies.risk_at_coverage
    assert risk_levels(np.array([[0.7, 0.5]]), np.array([[0.3, 0.1]]), np.array([0.95]))[0, 0] == pytest.approx(0.3)


def test_the_full_data_curve_is_the_policies_sweep():
    for seed in range(5):
        a = toy(seed=seed)
        _, ans, err = outcomes(a, (1, 3, 4))
        r80, aurc = curve_stats(np.ones((1, len(a.gold)), np.float32), ans, err)
        want = risk_at_coverage(sweep_shared(a, tiers=(1, 3, 4)))["risk"]
        assert r80[0] == pytest.approx(want, abs=1e-6)
        assert aurc[0] == pytest.approx(np.mean(risk_levels(*_direct(a), LEVELS)), abs=1e-6)


def _direct(a):
    pts = sweep_shared(a, tiers=(1, 3, 4))
    cov = np.array([[p["coverage"] for p in pts]])
    risk = np.array([[np.nan if p["risk"] is None else p["risk"] for p in pts]])
    return cov, risk


def test_one_tiers_risk_depends_only_on_the_order_of_its_confidences():
    rng = np.random.default_rng(3)
    conf, ok = rng.random(200), rng.random(200) < 0.8
    assert tier_risk_at(conf, ok) == pytest.approx(tier_risk_at(conf ** 3, ok))  # same order, same risk


def test_ece_gap_is_hinglish_minus_english():
    conf = np.r_[np.full(50, 0.9), np.full(50, 0.9)]
    ok = np.r_[np.ones(45, bool), np.zeros(5, bool), np.ones(25, bool), np.zeros(25, bool)]
    lang = np.array(["en"] * 50 + ["hi-en"] * 50)
    assert ece_gap(conf, ok, lang) == pytest.approx(0.4 - 0.0)  # en: 0.9 vs 0.9; hi-en: 0.9 vs 0.5
