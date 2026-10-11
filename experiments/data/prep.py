"""experiments/data/prep.py -- cleaning, dedup, leakage-safe splits, fingerprints (Ch. 32, A.16).

Differences from A.16, each covered by a test in test_prep.py:
- norm_text mirrors the Rust normalizer that runs at inference, checked against the shared cases in
  datasets/contracts/normalize_cases.json. A.16 strips ".!?" and then spaces only once ("wow?! ."
  stays "wow?!"), applies NFKC, which Rust doesn't (yet), and splits on U+001C..U+001F, which Rust
  doesn't count as whitespace.
- Splits order items by a seeded SHA-256 instead of numpy's shuffle, so a frozen split can be
  regenerated on any numpy/Python version. Cut points come from cumulative fractions.
- group_split4 gives the four disjoint sets *and* keeps each group (template family, OOS domain)
  whole, stratified by label. A.16's group_split is two-way only; Ch. 32 needs both.
"""
from __future__ import annotations

import hashlib
import re
from collections import defaultdict

NAMES = ("train", "cal", "val", "test")

# Unicode White_Space, which is what Rust's char::is_whitespace uses. Python's str.split() also
# splits on U+001C..U+001F, so the set is spelled out.
_WS = "\t\n\x0b\x0c\r \x85\xa0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000"
_WS_RUN = re.compile(f"[{_WS}]+")


def norm_text(s: str) -> str:
    """Same output as kestrel_core::normalize: lowercase, single spaces, no trailing . ! ? or space."""
    collapsed = " ".join(t for t in _WS_RUN.split(s.lower()) if t)
    return collapsed.rstrip(".!? ")


def dedupe(rows):
    """rows: dicts with text and label. Keeps the first copy of each normalized text. If one text
    has conflicting labels, drops ALL its copies and reports (text, sorted labels) as label noise."""
    by_text = defaultdict(list)
    for r in rows:
        by_text[norm_text(r["text"])].append(r)
    kept, conflicts = [], []
    for text, group in by_text.items():
        labels = {r["label"] for r in group}
        if len(labels) > 1:
            conflicts.append((text, sorted(labels)))
        else:
            kept.append(group[0])
    return kept, conflicts


def seeded_order(keys, seed):
    """A seeded permutation that depends only on the keys, not on any RNG implementation."""
    return sorted(keys, key=lambda k: hashlib.sha256(f"{seed}\x00{k}".encode()).hexdigest())


def _cut(items, fracs):
    """Splits items into len(fracs) consecutive parts at the rounded cumulative boundaries."""
    assert abs(sum(fracs) - 1) < 1e-9, fracs
    n, total, bounds = len(items), 0.0, [0]
    for f in fracs:
        total += f
        bounds.append(round(total * n))
    bounds[-1] = n
    return [items[a:b] for a, b in zip(bounds, bounds[1:])]


def stratified_split4(rows, fracs=(0.6, 0.1, 0.1, 0.2), seed=0):
    """train / cal (fit temperature) / val (tune thresholds) / test (touched once per frozen
    config): disjoint, stratified by label. Returns {name: sorted row indices}."""
    by_label = defaultdict(list)
    for i, r in enumerate(rows):
        by_label[r["label"]].append(i)
    out = {name: [] for name in NAMES}
    for label in sorted(by_label):
        for name, part in zip(NAMES, _cut(seeded_order(by_label[label], f"{seed}:{label}"), fracs)):
            out[name].extend(part)
    return {name: sorted(v) for name, v in out.items()}


def group_split4(rows, group_key, strat_key, fracs=(0.6, 0.1, 0.1, 0.2), seed=0):
    """Like stratified_split4, but whole groups are assigned, so no group straddles two sets.
    Groups are stratified by strat_key; a group whose rows disagree on it is a ValueError."""
    stratum_of = {}
    for r in rows:
        g, s = r[group_key], r[strat_key]
        if stratum_of.setdefault(g, s) != s:
            raise ValueError(f"group {g!r} has rows in strata {stratum_of[g]!r} and {s!r}")
    groups_by_stratum = defaultdict(list)
    for g, s in stratum_of.items():
        groups_by_stratum[s].append(g)
    set_of = {}
    for s in sorted(groups_by_stratum):
        parts = _cut(seeded_order(sorted(groups_by_stratum[s]), f"{seed}:{s}"), fracs)
        for name, part in zip(NAMES, parts):
            set_of.update(dict.fromkeys(part, name))
    out = {name: [] for name in NAMES}
    for i, r in enumerate(rows):
        out[set_of[r[group_key]]].append(i)
    return out


def dataset_fingerprint(rows) -> str:
    """Order-independent SHA-256 of (normalized text, label): record it in every experiment manifest."""
    items = sorted(f"{norm_text(r['text'])}\t{r['label']}" for r in rows)
    return hashlib.sha256("\n".join(items).encode()).hexdigest()
