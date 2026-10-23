import json
from collections import Counter

from build_review import SECTIONS, build, page_data
from review_data import all_units


def test_every_unit_is_in_exactly_one_section_with_guidance():
    data = page_data()
    ids = [u["id"] for s in data["sections"] for u in s["units"]]
    assert Counter(ids) == Counter(u["id"] for u in all_units())
    assert all(s["title"] and s["help"] for s in data["sections"])
    assert [(s["ds"], s["section"]) for s in data["sections"]] == list(SECTIONS)
    assert len(data["fixtures"]["tasks"]) == 12


def test_the_page_inlines_the_data_safely(tmp_path):
    page = tmp_path / "review.html"
    build(page)
    html = page.read_text(encoding="utf-8")
    assert "/*__DATA__*/null" not in html and "<title>Kestrel Dataset Review</title>" in html
    start = html.index("const DATA = ") + len("const DATA = ")
    blob = html[start: html.index(";\n", start)]
    assert "</" not in blob  # a "</script>" inside a note would end the script early
    assert len(json.loads(blob.replace("<\\/", "</"))["sections"]) == len(SECTIONS)
