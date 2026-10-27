import json

import pytest

from notes_review import build, ingest_files, units


def by_id(uid):
    return next(u for u in units() if u["id"] == uid)


def test_a_notesq_family_shows_its_queries_and_the_note_they_should_find():
    u = by_id("notesq:found:q-budget")
    assert u["title"] == "q-budget · 2026-09-02-budget-plan.md"
    assert [l["k"] for l in u["lines"]] == ["en", "hi-en", "Note"]
    assert u["lines"][2]["v"].startswith("Monthly budget Rent 12000")
    assert by_id("notesq:none:q-none-guitar")["lines"][2]["v"] == "No note fits: anything found is noise."


def test_a_notesqa_family_shows_the_facts_a_right_answer_holds():
    u = by_id("notesqa:answered:qa-rent")
    assert [l["k"] for l in u["lines"]] == ["en", "hi-en", "Facts", "Note"]
    assert u["lines"][2]["v"] == "12000 / 12,000"
    assert by_id("notesqa:unanswered:qa-none-salary")["lines"][2]["v"].startswith("The notes do not say")


def test_kinds_v1_families_sit_under_their_kind():
    assert by_id("kindsv1:notes:notes-01")["title"] == "notes-01 · notes · schedule"
    assert by_id("kindsv1:app:app-v1-03")["lines"][-1]["k"] == "Note"


def test_the_page_keeps_its_marks_apart_from_the_other_reviews(tmp_path):
    page = tmp_path / "notes_review.html"
    build(page)
    html = page.read_text(encoding="utf-8")
    assert "<title>Kestrel Notes Review</title>" in html
    assert html.count('"kestrel-notes-review:"') == 2 and '"kestrel-review:"' not in html
    assert "kestrel-notes-review.json" in html and "kestrel-review.json" not in html
    assert "these three drafts for v3's notes features" in html and "word lists" not in html


def _set(path, unit, hash_):
    path.write_text(json.dumps({"kind": "kestrel-review-set", "version": 1, "reviews": [
        {"unit": unit, "hash": hash_, "decision": "ok", "note": "", "at": "2026-10-27T00:00:00Z"}]}))
    return path


def test_ingest_takes_its_own_marks_and_refuses_the_kinds_review(tmp_path):
    u = units()[0]
    out = ingest_files([_set(tmp_path / "n.json", u["id"], u["hash"])], log=tmp_path / "log.json")
    assert out["status"][u["ds"]]["ok"] == 1
    with pytest.raises(ValueError, match="unknown unit"):
        ingest_files([_set(tmp_path / "k.json", "kinds:chat:chat-01", "x")], log=tmp_path / "log2.json")


def test_the_frozen_notes_datasets_are_ok_on_the_cards_as_they_are_now():
    from notes_review import LOG
    from ingest_review import status
    s = status(json.loads(LOG.read_text())["decisions"], units())
    for ds in ("notesq", "notesqa"):
        assert s[ds]["ok"] == s[ds]["units"], (ds, s[ds])
