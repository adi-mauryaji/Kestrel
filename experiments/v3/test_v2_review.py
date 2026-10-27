import json

import pytest

from v2_review import build, ingest_files, units


def by_id(uid):
    return next(u for u in units() if u["id"] == uid)


def test_a_code_task_shows_its_requests_answer_and_hidden_checks_as_code():
    u = by_id("codetasks:fix:fix-average")
    assert [l["k"] for l in u["lines"]] == ["en", "hi-en", "Code to fix", "A right answer", "Hidden checks"]
    assert all(l.get("mono") for l in u["lines"][2:])
    assert "sum(xs) / len(xs) + 1" in u["lines"][2]["v"]
    sc = by_id("codetasks:script:sc-celsius")
    assert sc["lines"][-1] == {"k": "Hidden checks", "v": "input: 37 -> output: 98.6\ninput: 0 -> output: 32.0\n"
                               "input: -40 -> output: -40.0", "mono": True}
    ex = by_id("codetasks:explain:ex-sort-key")
    assert ex["lines"][-1] == {"k": "A right answer says", "v": "sort · age / umar / umr"}


def test_kinds_v2_cards_sit_under_their_kind():
    assert by_id("kindsv2:code:code-01")["title"] == "code-01 · code · function"
    assert by_id("kindsv2:chat:chat-v2-01")["lines"][-1]["k"] == "Note"
    assert {u["section"] for u in units() if u["ds"] == "kindsv2"} == {"code", "chat", "app", "rewrite", "notes"}


def test_the_page_keeps_its_marks_apart_from_the_other_reviews(tmp_path):
    page = tmp_path / "v2_review.html"
    data = build(page)
    html = page.read_text(encoding="utf-8")
    assert "<title>Kestrel Code Review</title>" in html
    assert html.count('"kestrel-v2-review:"') == 2 and '"kestrel-review:"' not in html
    assert "kestrel-v2-review.json" in html and "kestrel-review.json" not in html
    assert list(data["titles"]) == ["codetasks", "kindsv2"]
    assert sum(len(s["units"]) for s in data["sections"]) == len(units())


def _set(path, unit, hash_):
    path.write_text(json.dumps({"kind": "kestrel-review-set", "version": 1, "reviews": [
        {"unit": unit, "hash": hash_, "decision": "ok", "note": "", "at": "2026-10-27T00:00:00Z"}]}))
    return path


def test_ingest_takes_its_own_marks_and_refuses_the_notes_review(tmp_path):
    u = units()[0]
    out = ingest_files([_set(tmp_path / "c.json", u["id"], u["hash"])], log=tmp_path / "log.json")
    assert out["status"][u["ds"]]["ok"] == 1
    with pytest.raises(ValueError, match="unknown unit"):
        ingest_files([_set(tmp_path / "n.json", "kindsv1:notes:notes-01", "x")], log=tmp_path / "log2.json")
