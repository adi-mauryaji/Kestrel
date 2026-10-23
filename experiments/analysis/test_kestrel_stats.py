import numpy as np
import pytest

from kestrel_stats import (bootstrap, bootstrap_means, cohens_dz, combine, holm, intersection_union_p, mcnemar_exact,
                           outcome, p_above, p_below, p_noninferior, paired_bootstrap_ci, percentile_ci,
                           wilcoxon_paired)


def test_bootstrap_ci_matches_normal_theory_on_paired_data():
    # A.19a's case: a shared item effect cancels in the paired difference, sd(d) = sqrt(2).
    rng = np.random.default_rng(0)
    shared = rng.normal(0, 1, 400)
    a, b = rng.normal(0.2, 1, 400) + shared, rng.normal(0.0, 1, 400) + shared
    m, lo, hi = paired_bootstrap_ci(a, b, seed=3)
    d = a - b
    half = 1.96 * d.std(ddof=1) / np.sqrt(len(d))
    assert m == pytest.approx(d.mean())
    assert lo < m < hi
    assert (hi - lo) / 2 == pytest.approx(half, rel=0.1)


def test_bootstrap_ci_covers_the_true_difference_in_most_trials():
    # Ch. 51: the CI brackets the true simulated difference in >= 90% of trials (nominal 95%).
    rng = np.random.default_rng(1)
    hits = 0
    for t in range(100):
        a = rng.random(200) < 0.80
        b = rng.random(200) < 0.75
        _, lo, hi = paired_bootstrap_ci(a, b, n_boot=1000, seed=t)
        hits += lo < 0.05 < hi
    assert hits >= 90


def test_resamples_are_deterministic_and_shared_by_every_statistic():
    d = np.random.default_rng(2).normal(size=300)
    means = bootstrap_means(d, n_boot=1200, seed=7)  # more than one block
    assert means.shape == (1200,)
    assert np.array_equal(means, bootstrap_means(d, n_boot=1200, seed=7))
    assert not np.array_equal(means, bootstrap_means(d, n_boot=1200, seed=8))
    # The generic path draws the same item resamples: a CI and a p-value of one comparison agree.
    assert np.allclose(bootstrap(np.mean, d, n_boot=1200, seed=7), means)


def test_generic_bootstrap_keeps_arrays_paired():
    a = np.arange(50.0)
    # Resampled rows stay aligned across arrays, so a - a is always exactly 0.
    out = bootstrap(lambda x, y: float(np.abs(x - y).max()), a, a.copy(), n_boot=200, seed=0)
    assert np.all(out == 0)


def test_inputs_must_be_paired_vectors():
    with pytest.raises(ValueError):
        paired_bootstrap_ci([1, 0, 1], [1, 0])
    with pytest.raises(ValueError):
        paired_bootstrap_ci(np.ones((2, 2)), np.ones((2, 2)))
    with pytest.raises(ValueError):
        bootstrap(np.mean, np.ones(3), np.ones(4))
    with pytest.raises(ValueError):
        paired_bootstrap_ci([], [])


def test_percentile_ci():
    assert percentile_ci(np.arange(101.0), alpha=0.1) == pytest.approx((5.0, 95.0))


def test_p_noninferior_hand_computed():
    boots = np.array([-0.03] * 3 + [0.0] * 97)
    assert p_noninferior(boots, 0.02) == pytest.approx(0.06)  # 2 x 3/100
    assert p_noninferior(boots, 0.03) == pytest.approx(0.06)  # "at or below" the margin counts
    assert p_noninferior(boots, 0.04) == 0.0
    assert p_noninferior(np.full(10, -1.0), 0.02) == 1.0  # capped


def test_p_noninferior_agrees_with_the_ci_rule():
    # H1's rule: non-inferior iff the 95% CI's lower bound > -delta. On the two-sided scale the
    # bootstrap p is < 0.05 exactly then (away from the boundary), whatever the data.
    rng = np.random.default_rng(4)
    seen = set()
    for shift in np.linspace(-0.2, 0.1, 31):  # mean(a - b) ~ shift / 2: from -0.1 to 0.05
        b = rng.random(1000) < 0.85
        a = np.where(rng.random(1000) < 0.5, b, rng.random(1000) < 0.85 + shift)  # paired: half agree
        boots = bootstrap_means(a.astype(float) - b, n_boot=2000, seed=0)
        lo, _ = percentile_ci(boots)
        ci_says = lo > -0.0263
        assert (p_noninferior(boots, 0.0263) < 0.05) == ci_says
        seen.add(ci_says)
    assert seen == {True, False}


def test_directional_p_on_the_two_sided_scale():
    boots = np.array([-1.0, 1.0, 2.0, 3.0])
    assert p_above(boots) == pytest.approx(0.5)  # 2 x 1/4 at or below 0
    assert p_below(boots) == 1.0  # 2 x 3/4, capped
    assert p_below(-boots) == pytest.approx(0.5)
    assert p_above(boots, bound=2.0) == 1.0
    assert p_above(np.array([0.1, 0.2])) == 0.0
    assert p_noninferior(boots, 0.5) == p_above(boots, -0.5)


def test_p_below_agrees_with_the_ci_upper_bound():
    # H1's latency part: faster iff the 95% CI of mean(lat_R3 - lat_R0) has its upper bound < 0.
    rng = np.random.default_rng(5)
    seen = set()
    for shift in np.linspace(-20, 10, 31):
        d = rng.normal(shift, 50, 400)
        boots = bootstrap_means(d, n_boot=2000, seed=0)
        ci_says = percentile_ci(boots)[1] < 0
        assert (p_below(boots) < 0.05) == ci_says
        seen.add(ci_says)
    assert seen == {True, False}


def test_mcnemar_known_counts():
    # Ch. 38.3: 10 vs 5 discordant pairs, exact two-sided p = 0.3018.
    a = np.array([1] * 10 + [1] * 5 + [0] * 5, bool)
    b = np.array([0] * 10 + [1] * 5 + [1] * 5, bool)
    n01, n10, p = mcnemar_exact(a, b)
    assert (n01, n10) == (5, 10)
    assert p == pytest.approx(0.3018, abs=1e-3)
    assert mcnemar_exact(b, a)[2] == pytest.approx(p)


def test_mcnemar_no_discordant():
    assert mcnemar_exact([1, 0], [1, 0]) == (0, 0, 1.0)


def test_holm_matches_hand_computation():
    adj, rej = holm([0.001, 0.02, 0.04, 0.3])
    assert np.allclose(adj, [0.004, 0.06, 0.08, 0.3])
    assert list(rej) == [True, False, False, False]


def test_holm_keeps_input_order_and_is_monotone():
    p = [0.04, 0.001, 0.3, 0.02]
    adj, rej = holm(p)
    assert np.allclose(adj, [0.08, 0.004, 0.3, 0.06])
    assert list(rej) == [False, True, False, False]
    order = np.argsort(p)
    assert np.all(np.diff(adj[order]) >= 0)
    assert holm([])[0].size == 0


def test_wilcoxon_and_dz():
    a = np.arange(10, 30, dtype=float)
    b = a - 1 + np.linspace(-0.2, 0.2, 20)
    assert wilcoxon_paired(a, b) < 0.01
    assert cohens_dz(a, b) > 1
    assert wilcoxon_paired(a, a) == 1.0
    assert np.isnan(cohens_dz(a, a))
    with pytest.raises(ValueError):
        wilcoxon_paired(a, b[:-1])


def test_intersection_union_p_is_the_largest_component():
    assert intersection_union_p(0.01, 0.2, 0.03) == 0.2
    with pytest.raises(ValueError):
        intersection_union_p()


def test_singleton_clusters_are_the_item_bootstrap():
    d = np.random.default_rng(6).normal(size=120)
    assert np.allclose(bootstrap_means(d, n_boot=1200, seed=2, clusters=np.arange(120)),
                       bootstrap_means(d, n_boot=1200, seed=2))


def test_cluster_bootstrap_resamples_whole_families():
    # 20 families of 5 identical items: the family bootstrap is the bootstrap of the 20 family means,
    # and the item bootstrap, which treats 100 items as independent, is about sqrt(5) too narrow.
    fam_means = np.random.default_rng(7).normal(size=20)
    d = np.repeat(fam_means, 5)
    labels = np.repeat([f"f{k:02d}" for k in range(20)], 5)
    fam = bootstrap_means(d, seed=0, clusters=labels)
    assert np.allclose(fam, bootstrap_means(fam_means, seed=0))
    width = lambda b: np.subtract(*percentile_ci(b)[::-1])  # noqa: E731
    assert width(fam) / width(bootstrap_means(d, seed=0)) == pytest.approx(np.sqrt(5), rel=0.15)
    assert np.allclose(bootstrap(np.mean, d, n_boot=600, seed=1, clusters=labels),
                       bootstrap_means(d, n_boot=600, seed=1, clusters=labels))
    _, lo, hi = paired_bootstrap_ci(d, np.zeros(100), clusters=labels)
    assert (lo, hi) == pytest.approx(percentile_ci(fam))


def test_unequal_families_weight_items_not_families():
    # One resample = the drawn families' items pooled: with families of sizes 1 and 3, every draw's mean
    # is the item mean of what was drawn, never the mean of family means.
    d = np.array([1.0, 0.0, 0.0, 0.0])
    labels = np.array(["a", "b", "b", "b"])
    vals = set(np.round(bootstrap_means(d, n_boot=200, seed=0, clusters=labels), 6))
    assert vals <= {1.0, 0.0, 0.25}  # aa, bb, ab / ba


def test_clusters_must_label_every_item():
    with pytest.raises(ValueError):
        bootstrap_means(np.ones(4), clusters=["a", "b"])


def test_outcome_rule_adr_029():
    # Non-inferiority (diff > -delta), delta = 0.02.
    assert outcome(True, 0.0, (-0.01, 0.01), above=True, bound=-0.02) == "supported"
    assert outcome(False, -0.01, (-0.04, 0.02), above=True, bound=-0.02) == "inconclusive"
    assert outcome(False, -0.05, (-0.07, -0.03), above=True, bound=-0.02) == "not supported"
    # Faster (diff < 0).
    assert outcome(True, -300.0, (-400.0, -200.0), above=False) == "supported"
    assert outcome(False, 5.0, (1.0, 9.0), above=False) == "not supported"
    # A two-sided test can be significant the wrong way: never "supported".
    assert outcome(True, 5.0, (1.0, 9.0), above=False) == "not supported"
    assert outcome(True, 0.01, (-0.001, 0.02), above=False) == "inconclusive"


def test_a_claim_with_parts():
    assert combine("supported", "supported") == "supported"
    assert combine("supported", "inconclusive") == "inconclusive"
    assert combine("inconclusive", "not supported") == "not supported"
    with pytest.raises(ValueError):
        combine("maybe")
