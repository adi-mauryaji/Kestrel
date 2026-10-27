import json

from notesqa_v0 import build, load, rows, split, validate


def fam(**kw):
    base = {"id": "qa-x", "notes": ["2026-09-02-budget-plan.md"], "facts": [["12000"]],
            "en": ["how much is my rent?"], "hi-en": ["mera rent kitna hai?"]}
    return {**base, **kw}


def test_validate_refuses_unknown_notes_facts_without_a_note_and_missing_languages():
    assert validate([fam()]) == []
    assert validate([fam(notes=[], facts=[])]) == []
    assert validate([fam(notes=["no-such.md"])]) == ["qa-x: note 'no-such.md' is not in the NotesQ-v0 notes"]
    assert validate([fam(facts=[])]) == ["qa-x: a family with a note needs its facts"]
    assert validate([fam(notes=[])]) == ["qa-x: a family without a note has no facts"]
    assert validate([fam(facts=[["12000"], []])]) == ["qa-x: an empty fact"]
    assert validate([fam(**{"hi-en": []})]) == ["qa-x: needs at least one en and one hi-en question"]
    assert validate([fam(), fam()]) == ["qa-x: id used twice"]


def test_the_committed_families_are_valid():
    assert validate(load()) == []


def test_families_never_straddle_and_both_splits_have_both_kinds_of_question():
    parts = split(rows(load()))
    home = {}
    for name, rs in parts.items():
        assert {bool(r["notes"]) for r in rs} == {True, False}, name
        for r in rs:
            assert home.setdefault(r["family"], name) == name
    assert set(parts) == {"dev", "test"}


def test_the_build_is_byte_for_byte_deterministic(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    build(a)
    build(b)
    names = sorted(p.name for p in a.iterdir())
    assert names == ["dev.jsonl", "manifest.json", "test.jsonl"]
    assert all((a / n).read_bytes() == (b / n).read_bytes() for n in names)
    m = json.loads((a / "manifest.json").read_text())
    assert m["status"] == "draft" and m["splits"]["dev"]["questions"] + m["splits"]["test"]["questions"] == 51
