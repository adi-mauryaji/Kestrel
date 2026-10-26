import json

import pytest

from kinds_v0 import INTENTS, LABELS, build, load, rows, split, validate


def fam(**kw):
    base = {"id": "chat-01", "kind": "chat", "en": ["explain recursion"], "hi-en": ["recursion samjhao"]}
    return {**base, **kw}


def test_validate_refuses_unknown_kinds_missing_languages_and_repeated_ids():
    assert validate([fam()]) == []
    assert validate([fam(kind="code")]) == ["chat-01: kind 'code' is not in datasets/contracts/kinds.txt"]
    assert validate([fam(**{"hi-en": []})]) == ["chat-01: needs at least one en and one hi-en request"]
    assert validate([fam(), fam()]) == ["chat-01: id used twice"]


def test_every_new_kind_has_families_in_both_languages():
    families = load()
    assert validate(families) == []
    for kind in LABELS:
        mine = [f for f in families if f["kind"] == kind]
        assert len(mine) >= (10 if kind == "app" else 12), kind


def test_app_rows_come_from_the_intents_dev_splits_and_never_its_test_split():
    test_ids = {json.loads(l)["id"] for l in (INTENTS / "test.jsonl").open(encoding="utf-8")}
    dev_ids = {json.loads(l)["id"] for s in ("train", "cal", "val") for l in (INTENTS / f"{s}.jsonl").open(encoding="utf-8")}
    borrowed = {r["id"].removeprefix("intents:") for r in rows(load())[0] if r["source"] == "intents_v0"}
    assert borrowed == dev_ids and not borrowed & test_ids


def test_families_never_straddle_splits_and_every_split_has_every_label():
    parts = split(rows(load())[0])
    home = {}
    for name, rs in parts.items():
        assert {r["label"] for r in rs} == set(LABELS), name
        for r in rs:
            assert home.setdefault(r["group"], name) == name, r["group"]


def test_the_build_is_byte_for_byte_deterministic(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    build(a)
    build(b)
    names = sorted(p.name for p in a.iterdir())
    assert names == ["cal.jsonl", "manifest.json", "test.jsonl", "train.jsonl", "val.jsonl"]
    assert all((a / n).read_bytes() == (b / n).read_bytes() for n in names)


@pytest.mark.parametrize("text, label", [
    ("ye email abhi asha ko bhej do", "app"),
    ("manager ko friday ki chutti ke liye email likh do", "email"),
])
def test_rows_carry_the_kind_language_and_family(text, label):
    row = next(r for r in rows(load())[0] if r["text"] == text)
    assert (row["label"], row["lang"], row["source"]) == (label, "hi-en", "kinds_v0")
    assert row["group"] == row["family"]
