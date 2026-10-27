#!/usr/bin/env python3
"""experiments/v3/v2_review.py -- one review page for v3's code drafts (ADR-044).

    python experiments/v3/v2_review.py                  # write experiments/human/v2_review.html
    python experiments/v3/v2_review.py ingest FILE...   # merge saved marks into experiments/logs/v3/v2/review.json

Two drafts on one page: Kestrel-CodeTasks-v0 (code requests with a reference answer and the hidden checks that
judge a model's answer) and the Kestrel-Kinds-v2 draft (the `code` kind and its boundaries, more notes /
rewrite / chat families). One unit per family; code shows as code. As notes_review.py: the Dataset Review
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
from codetasks_v0 import load as load_codetasks  # noqa: E402
from ingest_review import ingest  # noqa: E402
from kinds_v2 import load as load_kinds_v2  # noqa: E402
from review_data import unit  # noqa: E402

PAGE = ROOT / "experiments" / "human" / "v2_review.html"
LOG = ROOT / "experiments" / "logs" / "v3" / "v2" / "review.json"
TITLES = {"codetasks": "Kestrel-CodeTasks-v0", "kindsv2": "Kestrel-Kinds-v2 draft (code)"}
SECTIONS = [
    ("codetasks", "function", "Write a function",
     "Would you ask it this way? Would a right answer pass the hidden checks, and a wrong one fail?"),
    ("codetasks", "script", "Write a script",
     "The script reads the input shown and must print exactly the output shown (case and end spaces ignored)."),
    ("codetasks", "fix", "Fix broken code", "The code to fix fails the checks; the right answer passes them."),
    ("codetasks", "explain", "Explain code", "A right explanation says every one of these (any of the ways given)."),
    ("kindsv2", "code", "code: write or fix Python", "Qwen3-4B writes it, and Kestrel checks that it runs."),
    ("kindsv2", "chat", "chat: explain code, concepts, poems", "Gemma answers: explaining is not changing code."),
    ("kindsv2", "app", "Looks like code, belongs to the old path", "A task, a notes file, or the device."),
    ("kindsv2", "rewrite", "rewrite: the forms the last test missed", "Lists, captions, expanding, one line, titles."),
    ("kindsv2", "notes", "notes: the questions the last test missed", "Study, documents, addresses, appointments."),
]
RENAMES = [("<title>Kestrel Dataset Review</title>", "<title>Kestrel Code Review</title>", 1),
           ("<h1>Dataset Review</h1>", "<h1>Code Review</h1>", 1),
           ('"kestrel-review:"', '"kestrel-v2-review:"', 2),
           ("kestrel-review.json", "kestrel-v2-review.json", 2),
           ("Claude wrote these three datasets and two word lists;",
            "Claude wrote these two drafts for v3's code feature (ADR-044);", 1),
           ("<li>Do the plans and expected results match what the request asks?</li>",
            "<li>Would a right answer pass the hidden checks, and only a right one?</li>", 1),
           ("<li>Are the word lists missing anything you would say?</li>",
            "<li>Does each Kinds-v2 card belong where it is: code, chat, app, rewrite or notes?</li>", 1)]


def _asked(f) -> list[dict]:
    mono = f.get("type") in ("fix", "explain")
    return [{"k": lang, "v": t.replace("{code}", f.get("code", "")), **({"mono": True} if mono else {})}
            for lang in ("en", "hi-en") for t in f[lang]]


def _code(k: str, v: str) -> dict:
    return {"k": k, "v": v.strip("\n"), "mono": True}


def _checks(f) -> dict:
    if f["type"] == "script":
        return _code("Hidden checks", "\n".join(f"input: {' / '.join(c['stdin'].split(chr(10))).strip(' /')} -> "
                                                f"output: {c['stdout']}" for c in f["cases"]))
    return _code("Hidden checks", f["tests"])


def units() -> list[dict]:
    out = []
    for f in load_codetasks():
        lines = _asked(f)
        if f["type"] in ("fix", "explain"):
            lines = [{"k": l["k"], "v": f[l["k"]][0].split("{code}")[0].strip()} for l in lines]
            lines.append(_code("Code to fix" if f["type"] == "fix" else "Code to explain", f["code"]))
        if f["type"] == "explain":
            lines.append({"k": "A right answer says", "v": " · ".join(" / ".join(ways) for ways in f["facts"])})
        else:
            lines += [_code("A right answer", f["solution"]), _checks(f)]
        out.append(unit("codetasks", f["type"], f["id"], f"{f['id']} · {f['type']}", lines))
    for f in load_kinds_v2():
        title = " · ".join([f["id"], f["kind"], *([f["domain"]] if "domain" in f else [])])
        lines = [{"k": lang, "v": " | ".join(f[lang])} for lang in ("en", "hi-en")]
        lines += [{"k": "Note", "v": f["note"]}] if "note" in f else []
        out.append(unit("kindsv2", f["kind"], f["id"], title, lines))
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
