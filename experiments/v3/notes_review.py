#!/usr/bin/env python3
"""experiments/v3/notes_review.py -- the review page and log for v3's notes data (ADR-039, ADR-041).

    python experiments/v3/notes_review.py                  # write experiments/human/notes_review.html
    python experiments/v3/notes_review.py ingest FILE...   # merge saved marks into experiments/logs/v3/notes/review.json

Three drafts on one page: Kestrel-NotesQ-v0 (searches in other words than the note uses), Kestrel-NotesQA-v0
(questions the notes answer, or do not) and the Kestrel-Kinds-v1 draft (the `notes` kind and its boundaries).
One unit per family; a card shows the fixture note it is about. As kinds_review.py: the Dataset Review
page's template with its own title, browser storage and file name, and ingest_review.ingest against these
units only.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "experiments" / "human"), str(ROOT / "experiments" / "data")]

from build_review import TEMPLATE  # noqa: E402
from ingest_review import ingest  # noqa: E402
from kinds_v1 import load as load_kinds_v1  # noqa: E402
from notesq_v0 import NOTES  # noqa: E402
from notesq_v0 import load as load_notesq  # noqa: E402
from notesqa_v0 import load as load_notesqa  # noqa: E402
from review_data import unit  # noqa: E402

PAGE = ROOT / "experiments" / "human" / "notes_review.html"
LOG = ROOT / "experiments" / "logs" / "v3" / "notes" / "review.json"
TITLES = {"notesq": "Kestrel-NotesQ-v0", "notesqa": "Kestrel-NotesQA-v0", "kindsv1": "Kestrel-Kinds-v1 draft (notes)"}
SECTIONS = [
    ("notesq", "found", "Searches that should find a note",
     "A search in other words than the note uses. Is it something you would type, and is the note shown the one "
     "it should find?"),
    ("notesq", "none", "Searches no note fits", "No fixture note is about this: Kestrel should find nothing."),
    ("notesqa", "answered", "Questions a note answers",
     "Would you ask it this way? Does the note answer it, and would a right answer hold every fact listed "
     "(the ways separated by /)?"),
    ("notesqa", "unanswered", "Questions the notes do not answer",
     "The right reply is that the notes do not say. Check that no note does."),
    ("kindsv1", "notes", "notes: a question your own notes answer",
     "Who, when, how much, which, about your life; with or without the word notes. Not a named file, not "
     "\"what do you remember\", not the last N notes (those are app)."),
    ("kindsv1", "app", "Looks like notes, belongs to the old path",
     "Finding, summarizing or opening notes, or what Kestrel remembers. The note says which."),
    ("kindsv1", "chat", "The same topics asked in general", "Not about what you wrote: Kestrel answers from general "
     "knowledge."),
]
RENAMES = [("<title>Kestrel Dataset Review</title>", "<title>Kestrel Notes Review</title>", 1),
           ("<h1>Dataset Review</h1>", "<h1>Notes Review</h1>", 1),
           ('"kestrel-review:"', '"kestrel-notes-review:"', 2),
           ("kestrel-review.json", "kestrel-notes-review.json", 2),
           ("Claude wrote these three datasets and two word lists;",
            "Claude wrote these three drafts for v3's notes features;", 1),
           ("<li>Do the plans and expected results match what the request asks?</li>",
            "<li>Is the note, or are the facts, the right answer to what is asked?</li>", 1),
           ("<li>Are the word lists missing anything you would say?</li>",
            "<li>Does each Kinds-v1 card belong where it is: notes, app or chat?</li>", 1)]


def _note(name: str) -> str:
    """The fixture note as one line, without its tags line."""
    text = (NOTES / name).read_text(encoding="utf-8")
    first, _, rest = text.partition("\n")
    return " ".join((rest if first.startswith("tags:") else text).split())


def _asked(f) -> list[dict]:
    return [{"k": lang, "v": t} for lang in ("en", "hi-en") for t in f[lang]]


def units() -> list[dict]:
    out = []
    for f in load_notesq():
        if f["notes"]:
            out.append(unit("notesq", "found", f["id"], f"{f['id']} · {', '.join(f['notes'])}",
                            _asked(f) + [{"k": "Note", "v": " | ".join(_note(n) for n in f["notes"])}]))
        else:
            out.append(unit("notesq", "none", f["id"], f["id"],
                            _asked(f) + [{"k": "Note", "v": "No note fits: anything found is noise."}]))
    for f in load_notesqa():
        if f["notes"]:
            facts = " · ".join(" / ".join(ways) for ways in f["facts"])
            out.append(unit("notesqa", "answered", f["id"], f"{f['id']} · {', '.join(f['notes'])}",
                            _asked(f) + [{"k": "Facts", "v": facts},
                                         {"k": "Note", "v": " | ".join(_note(n) for n in f["notes"])}]))
        else:
            out.append(unit("notesqa", "unanswered", f["id"], f["id"],
                            _asked(f) + [{"k": "Note", "v": "The notes do not say this: the right reply is that "
                                                             "they do not."}]))
    for f in load_kinds_v1():
        title = " · ".join([f["id"], f["kind"], *([f["domain"]] if "domain" in f else [])])
        lines = _asked(f) + ([{"k": "Note", "v": f["note"]}] if "note" in f else [])
        out.append(unit("kindsv1", f["kind"], f["id"], title, lines))
    return out


def build(page: Path = PAGE) -> dict:
    us = units()
    data = {"version": 1, "titles": TITLES, "fixtures": {},
            "sections": [{"ds": ds, "section": k, "title": t, "help": h,
                          "units": [u for u in us if u["ds"] == ds and u["section"] == k]}
                         for ds, k, t, h in SECTIONS]}
    html = TEMPLATE.read_text(encoding="utf-8")
    for old, new, n in RENAMES:
        assert html.count(old) == n, (old, html.count(old))
        html = html.replace(old, new)
    blob = json.dumps(data, ensure_ascii=False).replace("</", "<\\/")
    page.write_text(html.replace("/*__DATA__*/null", blob), encoding="utf-8")
    return data


def ingest_files(sources, log: Path = LOG) -> dict:
    return ingest(sources, units(), log)


def main(argv=None) -> None:
    argv = sys.argv[1:] if argv is None else argv
    if argv[:1] == ["ingest"]:
        out = ingest_files([Path(a) for a in argv[1:]])
        for ds, title in TITLES.items():
            s = out["status"][ds]
            print(f"{title}: {s['ok']}/{s['units']} ok, {s['fix']} to fix, {s['stale']} changed since reviewed, "
                  f"{s['missing']} not reviewed → {'ready to freeze' if s['ready'] else 'not ready'}")
        for f in out["fixes"]:
            print(f"  fix {f['unit']}: {f['note'] or '(no note)'}")
        return
    n = sum(len(s["units"]) for s in build()["sections"])
    print(f"{n} units -> {PAGE.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
