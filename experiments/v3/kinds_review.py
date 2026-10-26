#!/usr/bin/env python3
"""experiments/v3/kinds_review.py -- the review page and log for Kestrel-Kinds-v0 (ADR-038).

    python experiments/v3/kinds_review.py                  # write experiments/human/kinds_review.html
    python experiments/v3/kinds_review.py ingest FILE...   # merge saved marks into experiments/logs/v3/kinds/review.json

One unit per kinds.toml family, under its kind. The page is the Dataset Review page's template with its
own title, browser storage and file name: every file:// page shares one storage in Chrome, so sharing
the names would mix these marks into the research review's. Marks are ingested by ingest_review.ingest
against these units only, so neither log accepts the other's.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "experiments" / "human"), str(ROOT / "experiments" / "data")]

from build_review import TEMPLATE  # noqa: E402
from ingest_review import ingest  # noqa: E402
from kinds_v0 import load  # noqa: E402
from review_data import unit  # noqa: E402

PAGE = ROOT / "experiments" / "human" / "kinds_review.html"
LOG = ROOT / "experiments" / "logs" / "v3" / "kinds" / "review.json"
TITLES = {"kinds": "Kestrel-Kinds-v0"}
SECTIONS = {
    "chat": ("Chat", "The request asks Kestrel to advise, plan, brainstorm, write something creative or explain a "
             "timeless idea, and sounds like a real person. Current facts and calculations belong to app."),
    "email": ("Email and messages", "A draft written to someone: an email, letter, message or reply. Sending it "
              "is app."),
    "rewrite": ("Rewrite", "Changes text given in the request: tone, length, grammar, form. A summary of your own "
                "notes is app."),
    "translate": ("Translate", "Into or out of another language, or into Hinglish."),
    "app": ("Looks new, belongs to the old path", "The existing assistant handles it: a task, your notes, a "
            "preference, sending, current facts. The note says which."),
}
# Same template, its own names: (text in the template, replacement, how often it occurs).
RENAMES = [("<title>Kestrel Dataset Review</title>", "<title>Kestrel Kinds Review</title>", 1),
           ("<h1>Dataset Review</h1>", "<h1>Kinds Review</h1>", 1),
           ('"kestrel-review:"', '"kestrel-kinds-review:"', 2),
           ("kestrel-review.json", "kestrel-kinds-review.json", 2),
           ("Claude wrote these three datasets and two word lists;",
            "Claude wrote this draft of which skill a request asks for;", 1),
           ("<li>Do the plans and expected results match what the request asks?</li>",
            "<li>Does each card belong to the kind it is under?</li>", 1),
           ("<li>Are the word lists missing anything you would say?</li>",
            "<li>Is the note under an app card the right reason?</li>", 1)]


def units() -> list[dict]:
    out = []
    for f in load():
        title = " · ".join([f["id"], f["kind"], *([f["domain"]] if "domain" in f else [])])
        lines = [{"k": lang, "v": text} for lang in ("en", "hi-en") for text in f[lang]]
        lines += [{"k": "Note", "v": f["note"]}] if "note" in f else []
        out.append(unit("kinds", f["kind"], f["id"], title, lines))
    return out


def build(page: Path = PAGE) -> dict:
    us = units()
    data = {"version": 1, "titles": TITLES, "fixtures": {},
            "sections": [{"ds": "kinds", "section": k, "title": t, "help": h,
                          "units": [u for u in us if u["section"] == k]} for k, (t, h) in SECTIONS.items()]}
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
        s = out["status"]["kinds"]
        print(f"{TITLES['kinds']}: {s['ok']}/{s['units']} ok, {s['fix']} to fix, {s['stale']} changed since "
              f"reviewed, {s['missing']} not reviewed → {'ready to freeze' if s['ready'] else 'not ready'}")
        for f in out["fixes"]:
            print(f"  fix {f['unit']}: {f['note'] or '(no note)'}")
        return
    n = sum(len(s["units"]) for s in build()["sections"])
    print(f"{n} units -> {PAGE.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
