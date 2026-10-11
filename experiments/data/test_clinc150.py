import json
from pathlib import Path

import pytest

from clinc150 import MANIFEST, RAW, cross_split_duplicates, kestrel_splits, load_official, manifest


def _fake_full(n_intents=4, per=(10, 4, 6), oos=(3, 3, 5)):
    """A data_full.json-shaped dict: [text, label] pairs per official split."""
    d = {}
    for split, k in zip(("train", "val", "test"), per):
        d[split] = [[f"{split} {i} q{j}", f"intent{i}"] for i in range(n_intents) for j in range(k)]
    for split, k in zip(("oos_train", "oos_val", "oos_test"), oos):
        d[split] = [[f"{split} q{j}", "oos"] for j in range(k)]
    return d


def test_load_official_keeps_every_item_with_its_source(tmp_path):
    p = tmp_path / "data_full.json"
    p.write_text(json.dumps(_fake_full()))
    off = load_official(p)
    assert {k: len(v) for k, v in off.items()} == {
        "train": 40, "val": 16, "test": 24, "oos_train": 3, "oos_val": 3, "oos_test": 5}
    assert off["oos_val"][0] == {"id": "clinc150/oos_val/0", "text": "oos_val q0", "label": "oos",
                                 "official_split": "oos_val", "oos": True}


def test_kestrel_splits_follow_ch32(tmp_path):
    p = tmp_path / "data_full.json"
    p.write_text(json.dumps(_fake_full()))
    s = kestrel_splits(load_official(p), seed=0)
    src = {k: {r["official_split"] for r in v} for k, v in s.items()}
    assert src == {"train": {"train"}, "cal": {"val"}, "val": {"val", "oos_train", "oos_val"},
                   "test": {"test", "oos_test"}}
    assert not any(r["oos"] for r in s["train"] + s["cal"]), "OOS never trains or calibrates"
    for intent in range(4):  # official val (4 per intent) halved, stratified
        assert sum(r["label"] == f"intent{intent}" for r in s["cal"]) == 2
        assert sum(r["label"] == f"intent{intent}" for r in s["val"]) == 2
    assert len(s["test"]) == 24 + 5, "official test intact"
    assert s == kestrel_splits(load_official(p), seed=0)


def test_cross_split_duplicates_are_found_after_normalization():
    s = {"train": [{"text": "Play Music!", "label": "a"}], "test": [{"text": "play music", "label": "a"}],
         "cal": [], "val": []}
    assert cross_split_duplicates(s) == [("play music", ["test", "train"])]


@pytest.mark.skipif(not RAW.exists(), reason="run scripts/fetch_datasets.py first")
def test_real_clinc150_matches_the_committed_manifest():
    off = load_official()
    assert {k: len(v) for k, v in off.items()} == {
        "train": 15000, "val": 3000, "test": 4500, "oos_train": 100, "oos_val": 100, "oos_test": 1000}
    assert manifest(kestrel_splits(off, seed=0), seed=0) == json.loads(MANIFEST.read_text())
