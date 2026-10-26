import json

import pytest

from kinds_review import build, ingest_files, units
from kinds_v0 import load


def test_every_family_is_one_unit_under_its_kind_with_both_languages():
    us = units()
    assert [u["id"] for u in us] == [f"kinds:{f['kind']}:{f['id']}" for f in load()]
    u = next(u for u in us if u["key"] == "email-13")
    assert u["section"] == "email" and u["title"] == "email-13 · email"
    assert [line["k"] for line in u["lines"]] == ["en", "en", "hi-en", "hi-en", "Note"]


def test_the_page_keeps_its_marks_apart_from_the_dataset_review(tmp_path):
    page = tmp_path / "kinds_review.html"
    build(page)
    html = page.read_text(encoding="utf-8")
    assert "<title>Kestrel Kinds Review</title>" in html
    assert html.count('"kestrel-kinds-review:"') == 2 and '"kestrel-review:"' not in html
    assert "kestrel-kinds-review.json" in html and "kestrel-review.json" not in html
    assert "this draft of which skill a request asks for" in html and "word lists" not in html


def _set(path, unit, hash_):
    path.write_text(json.dumps({"kind": "kestrel-review-set", "version": 1, "reviews": [
        {"unit": unit, "hash": hash_, "decision": "ok", "note": "", "at": "2026-10-26T00:00:00Z"}]}))
    return path


def test_ingest_takes_kinds_marks_and_refuses_the_dataset_reviews(tmp_path):
    u = units()[0]
    out = ingest_files([_set(tmp_path / "k.json", u["id"], u["hash"])], log=tmp_path / "log.json")
    assert out["status"]["kinds"]["ok"] == 1 and not out["status"]["kinds"]["ready"]
    with pytest.raises(ValueError, match="unknown unit"):
        ingest_files([_set(tmp_path / "r.json", "tasks:family:create-01", "x")], log=tmp_path / "log2.json")
