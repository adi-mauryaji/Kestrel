import copy
import json

import pytest

from tasks_v0 import CATEGORIES, assign_splits, build, items, load, validate


def test_the_committed_draft_is_valid():
    assert validate(load()) == []


def test_families_are_split_whole_and_every_category_is_in_both_splits():
    data = load()
    splits = assign_splits(data["families"])
    assert set(splits.values()) == {"dev", "test"}
    rows = items(data["families"], splits)
    for fam in {r["family"] for r in rows}:
        assert len({r["split"] for r in rows if r["family"] == fam}) == 1, fam
    for cat in CATEGORIES:
        assert {r["split"] for r in rows if r["category"] == cat} == {"dev", "test"}, cat
    assert splits == assign_splits(list(reversed(data["families"])))  # depends on the ids only


def test_items_carry_both_languages_and_the_default_expectations():
    data = load()
    rows = items(data["families"], assign_splits(data["families"]))
    by_fam = {}
    for r in rows:
        by_fam.setdefault(r["family"], set()).add(r["lang"])
    assert all(langs == {"en", "hi-en"} for langs in by_fam.values())
    one = next(r for r in rows if r["family"] == "create-01" and r["lang"] == "hi-en")
    assert one["text"] == "kal asha ko call karna yaad dilana"
    assert one["plan"]["nodes"][0]["op"] == "create_task"
    assert one["expect"]["outcome"] == "done" and one["expect"]["tasks_deleted"] == []
    clarify = next(r for r in rows if r["family"] == "create-08")
    assert clarify["plan"] is None and clarify["expect"]["outcome"] == "clarify"


def test_hinglish_titles_keep_the_users_words():
    # ADR-024: a per-language gold plan and expectation where the title comes from the request.
    data = load()
    rows = items(data["families"], assign_splits(data["families"]))
    by = {(r["family"], r["lang"]): r for r in rows}
    en, hi = by[("create-01", "en")], by[("create-01", "hi-en")]
    assert en["plan"]["nodes"][0]["args"]["title"] == "call asha"
    assert hi["plan"]["nodes"][0]["args"]["title"] == "asha ko call karna"
    assert hi["expect"]["tasks_added"] == [{"title": "asha ko call karna", "due": "2026-10-23"}]
    assert by[("cmp-03", "hi-en")]["expect"]["tasks_deleted"] == [1]  # merged over the shared expectation
    assert by[("search-01", "hi-en")]["plan"] == by[("search-01", "en")]["plan"]


@pytest.mark.parametrize("break_it, message", [
    (lambda d: d["families"].append(copy.deepcopy(d["families"][0])), "duplicate"),
    (lambda d: d["families"][0].pop("hi-en"), "hi-en"),
    (lambda d: d["families"][0].update(category="weather"), "category"),
    (lambda d: next(f for f in d["families"] if f["id"] == "create-08").update(plan='{"nodes": []}'), "clarify"),
    (lambda d: d["families"][0].update(plan="not json"), "JSON"),
    (lambda d: d["families"][0].pop("plan"), "plan"),
    (lambda d: d["families"][0]["expect"].update(surprise=1), "surprise"),
    (lambda d: next(f for f in d["families"] if f["id"] == "search-02")["expect"].update(read=["nope.md"]), "nope.md"),
    (lambda d: d["families"][0]["expect"].update(tasks_added=[{"title": "x", "due": "tomorrow"}]), "due"),
    (lambda d: d["malformed"][0].update(error=""), "bad-01"),
    (lambda d: d["families"][0].update({"plan_hi-en": "not json"}), "plan_hi-en"),
    (lambda d: d["families"][0].update({"expect_hi-en": {"surprise": 1}}), "surprise"),
    (lambda d: next(f for f in d["families"] if f["id"] == "create-08").update({"plan_hi-en": "{}"}), "clarify"),
])
def test_validation_catches_broken_families(break_it, message):
    data = load()
    break_it(data)
    errors = validate(data)
    assert any(message in e for e in errors), errors


def test_build_is_deterministic_and_writes_what_the_rust_check_reads(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    ma, mb = build(a), build(b)
    assert ma == mb
    for name in ("items.jsonl", "malformed.jsonl", "fixture.json"):
        assert (a / name).read_bytes() == (b / name).read_bytes(), name
    fixture = json.loads((a / "fixture.json").read_text())
    assert fixture["today"] == "2026-10-22" and fixture["notes"].endswith("fixtures/notes")
    assert ma["status"] == "draft" and ma["families"] == len(load()["families"])
    assert ma["malformed"] == len(load()["malformed"])
    assert set(ma["splits"]) == {"dev", "test"}
