"""experiments/analysis/kestrel_stats.py -- statistical helpers for the Kestrel study (Ch. 38, A.15).

Every function takes PER-ITEM (or per-seed) paired arrays, never pre-aggregated means.

Adapted from A.15:
- Bootstrap resamples are drawn BLOCK rows at a time. A.15 builds one n_boot x n index matrix, which
  for 10 000 resamples of CLINC150's 5500 test items is 440 MB of int64; a block is 22 MB.
- One resampling scheme serves every statistic (`bootstrap_means` for means, `bootstrap` for
  anything else), so a CI and a p-value of the same comparison, same seed, share their resamples.
- Directional bootstrap p-values (`p_above`, `p_below`, `p_noninferior`) and the intersection-union
  p of a claim made of several parts, because Holm needs a p for every hypothesis in the secondary
  family and A.15 has only CIs for the non-inferiority ones. They are on the two-sided scale: p <
  alpha <=> the (1 - alpha) percentile CI excludes the bound (up to one resample at the boundary).
- A cluster (family) bootstrap (ADR-029): Kestrel-Intents-v0 items come from template families, so
  a resample draws families, each bringing all its items, and the statistic is taken over the
  drawn items. Singleton clusters give exactly the item bootstrap.
- `outcome` / `combine`: ADR-029's supported / not supported / inconclusive.
- Shape checks raise ValueError (A.15's assert disappears under python -O).
- `forgetting` lives in metrics.py; `user_level_split` is not needed (Personas-v0 splits by persona).
"""
from __future__ import annotations

import numpy as np
from scipy import stats

N_BOOT = 10_000  # PREREG §6
BLOCK = 500  # resample rows per draw


def _vector(x) -> np.ndarray:
    v = np.asarray(x, float)
    if v.ndim != 1 or v.size == 0:
        raise ValueError(f"expected a non-empty 1-D array, got shape {v.shape}")
    return v


def _paired(a, b) -> np.ndarray:
    """a - b for paired per-item measurements."""
    a, b = _vector(a), _vector(b)
    if a.shape != b.shape:
        raise ValueError(f"paired arrays differ in length: {a.size} vs {b.size}")
    return a - b


def _resamples(n: int, n_boot: int, seed: int):
    """Index resamples (with replacement) of n units, BLOCK rows at a time, from one seeded generator."""
    rng = np.random.default_rng(seed)
    for start in range(0, n_boot, BLOCK):
        yield rng.integers(0, n, size=(min(BLOCK, n_boot - start), n))


def _cluster_index(clusters, n: int) -> tuple[np.ndarray, int]:
    """(cluster number of each item, number of clusters); clusters are numbered in sorted label order."""
    labels = np.asarray(clusters)
    if labels.shape != (n,):
        raise ValueError(f"{labels.size} cluster labels for {n} items")
    uniq, inv = np.unique(labels, return_inverse=True)
    return inv, uniq.size


def bootstrap_means(d, n_boot: int = N_BOOT, seed: int = 0, clusters=None) -> np.ndarray:
    """Mean of d over each of n_boot resamples of the items, or, with `clusters` (one label per item),
    of the clusters: a resample draws as many clusters as there are, and its mean is over their items."""
    d = _vector(d)
    if clusters is None:
        return np.concatenate([d[idx].mean(axis=1) for idx in _resamples(d.size, n_boot, seed)])
    inv, g = _cluster_index(clusters, d.size)
    sums, sizes = np.bincount(inv, weights=d, minlength=g), np.bincount(inv, minlength=g)
    return np.concatenate([sums[idx].sum(axis=1) / sizes[idx].sum(axis=1) for idx in _resamples(g, n_boot, seed)])


def bootstrap(stat, *arrays, n_boot: int = N_BOOT, seed: int = 0, clusters=None) -> np.ndarray:
    """stat(*resampled arrays) for each of n_boot resamples of the ITEMS (or clusters, as in
    bootstrap_means): row i of every array is item i, and a resample takes the same rows from all of
    them (e.g. confidences and correctness for a selective-risk difference). Same resamples as
    bootstrap_means for the same items, clusters and seed."""
    arrays = [np.asarray(a) for a in arrays]
    if not arrays or arrays[0].ndim == 0 or len(arrays[0]) == 0:
        raise ValueError("bootstrap needs at least one non-empty array")
    n = len(arrays[0])
    if any(len(a) != n for a in arrays):
        raise ValueError(f"arrays differ in length: {[len(a) for a in arrays]}")
    if clusters is None:
        rows = (row for idx in _resamples(n, n_boot, seed) for row in idx)
    else:
        inv, g = _cluster_index(clusters, n)
        members = [np.flatnonzero(inv == k) for k in range(g)]
        rows = (np.concatenate([members[k] for k in row]) for idx in _resamples(g, n_boot, seed) for row in idx)
    return np.array([stat(*(a[row] for a in arrays)) for row in rows], float)


def percentile_ci(boots, alpha: float = 0.05) -> tuple[float, float]:
    lo, hi = np.quantile(np.asarray(boots, float), [alpha / 2, 1 - alpha / 2])
    return float(lo), float(hi)


def paired_bootstrap_ci(a, b, n_boot: int = N_BOOT, alpha: float = 0.05, seed: int = 0, clusters=None):
    """(mean(a - b), lo, hi): percentile bootstrap CI over paired items (resample ITEMS, or their
    clusters, never systems)."""
    d = _paired(a, b)
    lo, hi = percentile_ci(bootstrap_means(d, n_boot, seed, clusters), alpha)
    return float(d.mean()), lo, hi


def p_above(boots, bound: float = 0.0) -> float:
    """Bootstrap p of "the difference is > bound": twice the share of resamples at or below `bound`,
    capped at 1. p < alpha <=> the (1 - alpha) percentile CI's lower bound is > bound. 0 means fewer
    than 1 in n_boot resamples."""
    return min(1.0, 2 * float(np.mean(np.asarray(boots, float) <= bound)))


def p_below(boots, bound: float = 0.0) -> float:
    """Bootstrap p of "the difference is < bound" (e.g. faster): the mirror of p_above."""
    return min(1.0, 2 * float(np.mean(np.asarray(boots, float) >= bound)))


def p_noninferior(boots, margin: float) -> float:
    """Bootstrap p of non-inferiority, "the difference is > -margin" (H1's accuracy rule)."""
    return p_above(boots, -margin)


def intersection_union_p(*pvals) -> float:
    """A claim that needs every part to hold (H1: non-inferior AND faster) is rejected at alpha iff
    each part is; its p is the largest part's (Berger 1982). No alpha-splitting."""
    if not pvals:
        raise ValueError("intersection_union_p of no parts")
    return float(max(pvals))


def mcnemar_exact(correct_a, correct_b) -> tuple[int, int, float]:
    """Exact two-sided McNemar test on paired binary correctness: (n01, n10, p) with n10 = A right and
    B wrong, n01 = A wrong and B right; p = binomial(n01 + n10, 1/2) on the discordant pairs."""
    a, b = _vector(correct_a).astype(bool), _vector(correct_b).astype(bool)
    if a.shape != b.shape:
        raise ValueError(f"paired arrays differ in length: {a.size} vs {b.size}")
    n10 = int(np.sum(a & ~b))
    n01 = int(np.sum(~a & b))
    if n01 + n10 == 0:
        return n01, n10, 1.0
    return n01, n10, float(stats.binomtest(n10, n01 + n10, 0.5, alternative="two-sided").pvalue)


def wilcoxon_paired(a, b) -> float:
    """Two-sided Wilcoxon signed-rank p on paired continuous measurements (per-item median latency);
    zero differences are dropped (zero_method "wilcox"), and all-zero differences give 1."""
    d = _paired(a, b)
    if np.allclose(d, 0):
        return 1.0
    return float(stats.wilcoxon(d, zero_method="wilcox", alternative="two-sided").pvalue)


def cohens_dz(a, b) -> float:
    """Paired effect size mean(d) / sd(d). Report it next to the raw difference and its CI."""
    d = _paired(a, b)
    sd = d.std(ddof=1)
    return float(d.mean() / sd) if sd > 0 else float("nan")


def holm(pvals, alpha: float = 0.05) -> tuple[np.ndarray, np.ndarray]:
    """Holm-Bonferroni step-down: (adjusted p, reject) in the INPUT order; ties keep input order."""
    p = np.asarray(pvals, float)
    m = p.size
    adj = np.empty(m)
    running = 0.0
    for rank, i in enumerate(np.argsort(p, kind="stable")):
        running = max(running, (m - rank) * p[i])
        adj[i] = min(1.0, running)
    return adj, adj <= alpha


OUTCOMES = ("supported", "not supported", "inconclusive")


def outcome(significant: bool, diff: float, ci, *, above: bool, bound: float = 0.0) -> str:
    """ADR-029 for one claim, "diff > bound" (above) or "diff < bound": supported iff the test is
    significant (Holm-adjusted for the secondary family) with the effect on the claimed side; not
    supported iff the 95% CI lies wholly on the other side (non-inferiority, bound -delta: upper bound
    < -delta); inconclusive otherwise."""
    if significant and (diff > bound if above else diff < bound):
        return "supported"
    wrong_side = ci[1] < bound if above else ci[0] > bound
    return "not supported" if wrong_side else "inconclusive"


def combine(*parts: str) -> str:
    """A claim with parts: supported iff every part is, not supported iff any part is."""
    if not parts or any(p not in OUTCOMES for p in parts):
        raise ValueError(f"parts must be outcomes {OUTCOMES}, got {parts}")
    if all(p == "supported" for p in parts):
        return "supported"
    return "not supported" if "not supported" in parts else "inconclusive"
