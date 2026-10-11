import json
from pathlib import Path

import pytest

from prep import dataset_fingerprint, dedupe, group_split4, norm_text, stratified_split4

CONTRACTS = Path(__file__).resolve().parents[2] / "datasets" / "contracts"
NAMES = ("train", "cal", "val", "test")


def test_norm_text_matches_the_rust_normalizer():
    cases = json.loads((CONTRACTS / "normalize_cases.json").read_text(encoding="utf-8"))
    wrong = [(c["input"], norm_text(c["input"]), c["expected"]) for c in cases
             if norm_text(c["input"]) != c["expected"]]
    assert wrong == []


def test_dedupe_drops_duplicates_after_normalization_and_reports_conflicts():
    rows = [{"text": "Call Asha", "label": "a"}, {"text": "call  asha!", "label": "a"},
            {"text": "x", "label": "a"}, {"text": "X.", "label": "b"}, {"text": "y", "label": "b"}]
    kept, conflicts = dedupe(rows)
    assert [r["text"] for r in kept] == ["Call Asha", "y"]
    assert conflicts == [("x", ["a", "b"])]


def _check_partition(split, n):
    assert set(split) == set(NAMES)
    seen = [i for name in NAMES for i in split[name]]
    assert sorted(seen) == list(range(n)), "every row in exactly one set"


def test_stratified_split4_is_disjoint_stratified_and_deterministic():
    rows = [{"text": f"t{i}", "label": "ab"[i % 2]} for i in range(100)]
    s = stratified_split4(rows, seed=1)
    _check_partition(s, 100)
    for label in "ab":
        sizes = [sum(rows[i]["label"] == label for i in s[name]) for name in NAMES]
        assert sizes == [30, 5, 5, 10]
    assert s == stratified_split4(rows, seed=1)
    assert s != stratified_split4(rows, seed=2)


def _grouped_rows():
    # 20 groups per label, 3 rows each (one English template + its Hinglish variants).
    return [{"text": f"{label}{g}-{k}", "label": label, "family": f"{label}{g}"}
            for label in "ab" for g in range(20) for k in range(3)]


def test_group_split4_never_splits_a_group_and_stratifies_groups():
    rows = _grouped_rows()
    s = group_split4(rows, "family", "label", seed=3)
    _check_partition(s, len(rows))
    where = {}
    for name in NAMES:
        for i in s[name]:
            assert where.setdefault(rows[i]["family"], name) == name, "group straddles sets"
    for label in "ab":
        groups = [{rows[i]["family"] for i in s[name] if rows[i]["label"] == label} for name in NAMES]
        assert [len(g) for g in groups] == [12, 2, 2, 4]
    assert s == group_split4(rows, "family", "label", seed=3)
    assert s != group_split4(rows, "family", "label", seed=4)


def test_group_split4_zero_fractions_leave_sets_empty():
    s = group_split4(_grouped_rows(), "family", "label", fracs=(0, 0, 1 / 3, 2 / 3), seed=0)
    assert s["train"] == [] and s["cal"] == []
    assert len(s["val"]) and len(s["test"])


def test_group_split4_rejects_a_group_with_two_strata():
    rows = [{"text": "p", "label": "a", "family": "f"}, {"text": "q", "label": "b", "family": "f"}]
    with pytest.raises(ValueError, match="f"):
        group_split4(rows, "family", "label")


def test_fingerprint_is_order_independent_and_normalization_aware():
    a = [{"text": "Hello  World!", "label": "x"}, {"text": "foo", "label": "y"}]
    b = [{"text": "foo", "label": "y"}, {"text": "hello world", "label": "x"}]
    assert dataset_fingerprint(a) == dataset_fingerprint(b)
    assert dataset_fingerprint(a) != dataset_fingerprint([a[0], {"text": "foo", "label": "x"}])
