import json

from notesq_v0 import NOTES, build, load, rows, split, validate


def fam(**kw):
    base = {"id": "q-x", "notes": ["2026-09-02-budget-plan.md"], "en": ["saving money"], "hi-en": ["paise bachana"]}
    return {**base, **kw}


def test_validate_refuses_unknown_notes_missing_languages_and_repeated_ids():
    assert validate([fam()]) == []
    assert validate([fam(notes=["no-such.md"])]) == ["q-x: note 'no-such.md' is not in notes/"]
    assert validate([fam(**{"hi-en": []})]) == ["q-x: needs at least one en and one hi-en query"]
    assert validate([fam(), fam()]) == ["q-x: id used twice"]
    assert validate([fam(notes=["../queries.toml"])]) == ["q-x: note '../queries.toml' is not in notes/"]


def test_the_committed_families_are_valid_and_every_note_is_wanted_somewhere():
    families = load()
    assert validate(families) == []
    wanted = {n for f in families for n in f["notes"]}
    on_disk = {p.relative_to(NOTES).as_posix() for p in NOTES.rglob("*.md")}
    assert wanted == on_disk


def test_families_never_straddle_and_both_splits_have_found_and_none_queries():
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
    assert m["status"] == "reviewed" and m["splits"]["dev"]["queries"] + m["splits"]["test"]["queries"] == 42


def test_the_frozen_build_says_reviewed_because_every_card_is_ok():
    from pathlib import Path
    root = Path(__file__).resolve().parents[2]
    manifest = json.loads((root / "datasets/kestrel_notesq_v0/splits/manifest.json").read_text())
    review = json.loads((root / "experiments/logs/v3/notes/review.json").read_text())
    s = review["status"]["notesq"]
    assert manifest["status"] == "reviewed"
    assert s["ready"] and s["ok"] == s["units"] and s["stale"] == 0
