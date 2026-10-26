import pytest

from splits import CLINC_RAW, DEV_SPLITS, load_splits


def test_dev_splits_never_include_test():
    # The test sets are touched once, after the PREREG freeze (Ch. 34.6). Tier-3 work stops at val.
    assert DEV_SPLITS == ("train", "cal", "val")


def test_intents_v0_splits_have_the_frozen_sizes_and_fields():
    s = load_splits("intents_v0")
    assert set(s) == set(DEV_SPLITS)
    assert [len(s[k]) for k in DEV_SPLITS] == [604, 100, 128]
    row = s["val"][0]
    assert {"text", "label", "lang"} <= set(row)


def test_kinds_v0_dev_splits_load_with_the_contract_classes():
    s = load_splits("kinds_v0")
    assert set(s) == set(DEV_SPLITS)
    assert {r["label"] for r in s["train"]} == {"app", "chat", "email", "rewrite", "translate"}
    assert all({"text", "label", "lang"} <= set(r) for r in s["val"])


@pytest.mark.skipif(not CLINC_RAW.exists(), reason="needs datasets/raw (scripts/fetch_datasets.py)")
def test_clinc150_splits_match_the_manifest_sizes():
    s = load_splits("clinc150")
    assert [len(s[k]) for k in DEV_SPLITS] == [15000, 1500, 1700]


def test_unknown_dataset_is_an_error():
    with pytest.raises(KeyError):
        load_splits("banking77")


def test_parity_texts_are_50_english_then_50_hinglish_train_items_by_id():
    from splits import parity_texts

    rows = parity_texts()
    assert [r["lang"] for r in rows] == ["en"] * 50 + ["hi-en"] * 50
    train_ids = {r["id"] for r in load_splits("intents_v0")["train"]}
    assert {r["id"] for r in rows} <= train_ids
    en = [r["id"] for r in rows[:50]]
    assert en == sorted(en)
    assert parity_texts() == rows  # deterministic
