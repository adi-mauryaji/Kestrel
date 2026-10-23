"""experiments/analysis/metrics.py -- accuracy, macro-F1, ECE (Ch. 36). Standard library only.

A prediction is a label or None (the cascade abstained). Scoring rule (to confirm in PREREG.md):
- in-scope item: correct iff the prediction equals the gold label; abstention is wrong (Ch. 36);
- out-of-scope item (gold == oos_label): abstaining, or predicting oos_label, is the correct
  response (Ch. 8.3: say it's unsupported, never improvise). CLINC150's OOS items have no head
  class (Ch. 32), so for them abstention is the only way to be right.

ECE uses equal-width, right-closed bins ((m-1)/M, m/M] (Guo et al. 2017). The blueprint's A.3
comment says (0,1] but its code bins left-closed; the test pins the convention used here.
"""
from __future__ import annotations

import math
from collections import Counter


def _check_lengths(a, b):
    if len(a) != len(b):
        raise ValueError(f"length mismatch: {len(a)} vs {len(b)}")


def per_item_correct(gold, pred, oos_label=None):
    """1[prediction is right] per item, in input order (the vector paired tests need)."""
    _check_lengths(gold, pred)
    out = []
    for y, p in zip(gold, pred):
        if oos_label is not None and y == oos_label:
            out.append(p is None or p == oos_label)
        else:
            out.append(p is not None and p == y)
    return out


def accuracy(gold, pred, oos_label=None):
    correct = per_item_correct(gold, pred, oos_label)
    return sum(correct) / len(correct)


def macro_f1(gold, pred, oos_label=None):
    """Mean over gold classes of F1 = 2PR/(P+R), 0 when a class is never predicted correctly.
    An abstention counts as predicting oos_label when one is given; otherwise it matches no class."""
    _check_lengths(gold, pred)
    pred = [oos_label if p is None else p for p in pred]
    tp = Counter(y for y, p in zip(gold, pred) if y == p)
    n_pred = Counter(p for p in pred if p is not None)
    n_gold = Counter(gold)
    f1 = []
    for c in sorted(n_gold, key=str):
        denom = n_pred[c] + n_gold[c]  # 2PR/(P+R) == 2TP/(n_pred + n_gold)
        f1.append(2 * tp[c] / denom if denom else 0.0)
    return sum(f1) / len(f1)


def ece(confidences, correct, *, n_bins):
    """Expected calibration error: sum over bins of (|B|/N) * |accuracy(B) - mean confidence(B)|."""
    _check_lengths(confidences, correct)
    if not confidences:
        raise ValueError("ece of no items")
    if any(not 0.0 <= c <= 1.0 for c in confidences):
        raise ValueError("confidences must lie in [0, 1]")
    count, conf_sum, hits = [0] * n_bins, [0.0] * n_bins, [0] * n_bins
    for c, ok in zip(confidences, correct):
        b = min(max(math.ceil(c * n_bins) - 1, 0), n_bins - 1)  # (b/M, (b+1)/M]; 0 goes to bin 0
        count[b] += 1
        conf_sum[b] += c
        hits[b] += bool(ok)
    n = len(confidences)
    return sum(count[b] / n * abs(hits[b] / count[b] - conf_sum[b] / count[b])
               for b in range(n_bins) if count[b])


def forgetting(r):
    """Continual-learning summary of an accuracy matrix (Ch. 22.3, A.15). r[i][j] = accuracy on
    task j after training stage i, T x T. avg_accuracy = mean of the last row; avg_forgetting = mean
    over j < T-1 of (max over l < T-1 of r[l][j]) - r[T-1][j] (Chaudhry et al. 2018); bwt = mean over
    j < T-1 of r[T-1][j] - r[j][j] (Lopez-Paz & Ranzato 2017). With one stage both are 0."""
    t = len(r)
    if t == 0 or any(len(row) != t for row in r):
        raise ValueError("r must be a non-empty square matrix (stages x tasks)")
    last = r[-1]
    f = [max(r[l][j] for l in range(t - 1)) - last[j] for j in range(t - 1)]
    b = [last[j] - r[j][j] for j in range(t - 1)]

    def mean(v):
        return sum(v) / len(v) if v else 0.0

    return {"avg_accuracy": mean(last), "avg_forgetting": mean(f), "bwt": mean(b)}
