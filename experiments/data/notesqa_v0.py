#!/usr/bin/env python3
"""experiments/data/notesqa_v0.py -- build Kestrel-NotesQA-v0 (ADR-041): questions the notes answer, or do not.

    python experiments/data/notesqa_v0.py --check     # validate datasets/kestrel_notesqa_v0/questions.toml
    python experiments/data/notesqa_v0.py             # write datasets/kestrel_notesqa_v0/splits/

Rows: every question of every family, with the note that answers it and the facts a right answer holds
(or neither: the notes do not say). The notes are Kestrel-NotesQ-v0's fixture notes. Families are split
whole into dev / test as in notesq_v0.py (half each, dev takes the odd one, seeded, two strata: answered
and not). Output is sorted: a build is the same bytes every time.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path

from notesq_v0 import NOTES
from prep import dataset_fingerprint, seeded_order

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets" / "kestrel_notesqa_v0"
LANGS = ("en", "hi-en")
SEED = 0


def load(path: Path = DATA / "questions.toml") -> list[dict]:
    return tomllib.loads(Path(path).read_text(encoding="utf-8"))["family"]


def reviewed(path: Path = DATA / "questions.toml") -> bool:
    """The human reviewed every family (`default_reviewed = true`, set when the review log is all OK)."""
    return tomllib.loads(Path(path).read_text(encoding="utf-8")).get("default_reviewed", False)


def validate(families: list[dict]) -> list[str]:
    problems, seen = [], set()
    on_disk = {p.relative_to(NOTES).as_posix() for p in NOTES.rglob("*.md")}
    for f in families:
        fid = f.get("id", "?")
        if fid in seen:
            problems.append(f"{fid}: id used twice")
        seen.add(fid)
        notes, facts = f.get("notes", []), f.get("facts", [])
        problems += [f"{fid}: note {n!r} is not in the NotesQ-v0 notes" for n in notes if n not in on_disk]
        if notes and not facts:
            problems.append(f"{fid}: a family with a note needs its facts")
        if facts and not notes:
            problems.append(f"{fid}: a family without a note has no facts")
        if any(not alt for alt in facts):
            problems.append(f"{fid}: an empty fact")
        if not all(f.get(lang) for lang in LANGS):
            problems.append(f"{fid}: needs at least one en and one hi-en question")
    return problems


def rows(families: list[dict]) -> list[dict]:
    return [{"id": f"{f['id']}/{lang}/{i}", "family": f["id"], "lang": lang, "text": text, "notes": f["notes"],
             "facts": f["facts"]}
            for f in families for lang in LANGS for i, text in enumerate(f[lang])]


def split(rs: list[dict]) -> dict[str, list[dict]]:
    answered = sorted({r["family"] for r in rs if r["notes"]})
    none = sorted({r["family"] for r in rs if not r["notes"]})
    dev = set()
    for name, fams in (("answered", answered), ("none", none)):
        order = seeded_order(fams, f"{SEED}:{name}")
        dev |= set(order[: (len(order) + 1) // 2])
    return {"dev": sorted((r for r in rs if r["family"] in dev), key=lambda r: r["id"]),
            "test": sorted((r for r in rs if r["family"] not in dev), key=lambda r: r["id"])}


def _sha(paths, base: Path) -> str:
    h = hashlib.sha256()
    for p in sorted(paths):
        h.update(p.relative_to(base).as_posix().encode() + b"\0" + p.read_bytes())
    return h.hexdigest()


def build(out: Path = DATA / "splits") -> dict:
    families = load()
    problems = validate(families)
    if problems:
        raise ValueError("\n".join(problems))
    parts = split(rows(families))
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    for name, part in parts.items():
        text = "".join(json.dumps(r, ensure_ascii=False, sort_keys=True) + "\n" for r in part)
        (out / f"{name}.jsonl").write_text(text, encoding="utf-8")
    labelled = [{"text": r["text"], "label": "|".join(r["notes"]) or "none"} for r in parts["dev"] + parts["test"]]
    manifest = {
        "dataset": "kestrel-notesqa-v0", "status": "reviewed" if reviewed() else "draft", "seed": SEED,
        "sources": {"questions.toml": _sha([DATA / "questions.toml"], DATA),
                    "kestrel_notesq_v0/notes/": _sha(NOTES.rglob("*.md"), NOTES)},
        "fingerprint": dataset_fingerprint(labelled),
        "splits": {name: {"families": len({r["family"] for r in part}), "questions": len(part),
                          "unanswerable": sum(not r["notes"] for r in part)}
                   for name, part in parts.items()},
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    return manifest


def main(argv=None) -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="validate only")
    a = ap.parse_args(argv)
    problems = validate(load())
    if problems:
        sys.exit("\n".join(problems))
    if a.check:
        print("ok")
        return
    print(json.dumps(build()["splits"], indent=1))


if __name__ == "__main__":
    main()
