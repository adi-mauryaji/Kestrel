"""experiments/human/build_review.py -- builds the Kestrel Dataset Review page.

    python experiments/human/build_review.py

Writes experiments/human/review.html (review.template.html with the units of review_data.py and the
fixture notes inlined). Opened from disk, marks stay in the browser and "Save review file" writes
kestrel-review.json; published as an Artifact with the `db` and `user` capabilities, each decision is
also one document `reviews/<unit id>` in the page's store. ingest_review.py reads either.
"""
from __future__ import annotations

import json
from pathlib import Path

from review_data import ROOT, TITLES, all_units, fixtures

HERE = Path(__file__).resolve().parent
TEMPLATE = HERE / "review.template.html"
PAGE = HERE / "review.html"
SECTIONS = {
    ("tasks", "family"): ("Task families", "Both requests ask for the same thing and sound like a real person. The "
                          "plan does what they ask, and the expected result is what should happen with the fixture "
                          "notes."),
    ("tasks", "malformed"): ("Malformed plans", "Each plan really is invalid, for the reason given. Kestrel must "
                             "refuse every one of them."),
    ("personas", "teach"): ("Teaching phrasings", "Someone could really teach Kestrel a nickname this way."),
    ("personas", "persona"): ("Personas", "The nicknames are believable for one person, and each teaching sentence "
                              "reads naturally."),
    ("personas", "pattern"): ("Request patterns", "The template sounds natural, every filled-in request reads well, "
                              "and its expected result is right. A variant changes the taught phrase only a little."),
    ("personas", "general"): ("General requests", "A plain request with no nickname, and the right expected result."),
    ("memory", "memory"): ("Memories", "A realistic thing for an assistant to remember about someone. Kind, source, "
                           "age and expiry make sense."),
    ("memory", "query"): ("Queries", "The question sounds natural. It should find exactly the listed memories, and "
                          "never the ones under “Must never return”."),
    ("words", "correction"): ("Correction words (correction.rs)", "These decide when a message is a correction and "
                              "whether it is meant for good. Add missing words in a note; flag any that would fire "
                              "wrongly."),
    ("words", "alias"): ("Nickname words (alias.rs)", "These decide when a message teaches a nickname. Add the ways "
                         "you would say it in a note."),
}


def page_data() -> dict:
    units = all_units()
    sections = [{"ds": ds, "section": sec, "title": title, "help": help_,
                 "units": [u for u in units if (u["ds"], u["section"]) == (ds, sec)]}
                for (ds, sec), (title, help_) in SECTIONS.items()]
    return {"version": 1, "titles": TITLES, "sections": sections, "fixtures": fixtures()}


def build(page: Path = PAGE) -> dict:
    data = page_data()
    blob = json.dumps(data, ensure_ascii=False).replace("</", "<\\/")
    html = TEMPLATE.read_text(encoding="utf-8")
    assert html.count("/*__DATA__*/null") == 1
    page.write_text(html.replace("/*__DATA__*/null", blob), encoding="utf-8")
    return data


if __name__ == "__main__":
    d = build()
    n = sum(len(s["units"]) for s in d["sections"])
    print(f"{n} units in {len(d['sections'])} sections -> {PAGE.relative_to(ROOT)}")
