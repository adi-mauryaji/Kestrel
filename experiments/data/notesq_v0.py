#!/usr/bin/env python3
"""experiments/data/notesq_v0.py -- build Kestrel-NotesQ-v0 (ADR-039): notes searches in other words.

    python experiments/data/notesq_v0.py --check     # validate datasets/kestrel_notesq_v0/queries.toml
    python experiments/data/notesq_v0.py             # write datasets/kestrel_notesq_v0/splits/

Rows: every query of every family, with the notes it should find (paths under notes/, none for the
q-none families). Families are split whole into dev / test, half each (dev takes the odd one), in
seeded order within two strata: families with notes and families without, so both splits hold both.
Output is sorted: a build is the same bytes every time.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path

from prep import dataset_fingerprint, seeded_order

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets" / "kestrel_notesq_v0"
NOTES = DATA / "notes"
LANGS = ("en", "hi-en")
SEED = 0


def load(path: Path = DATA / "queries.toml") -> list[dict]:
    return tomllib.loads(Path(path).read_text(encoding="utf-8"))["family"]


def reviewed(path: Path = DATA / "queries.toml") -> bool:
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
        problems += [f"{fid}: note {n!r} is not in notes/" for n in f.get("notes", []) if n not in on_disk]
        if not all(f.get(lang) for lang in LANGS):
            problems.append(f"{fid}: needs at least one en and one hi-en query")
    return problems


def rows(families: list[dict]) -> list[dict]:
    return [{"id": f"{f['id']}/{lang}/{i}", "family": f["id"], "lang": lang, "text": text, "notes": f["notes"]}
            for f in families for lang in LANGS for i, text in enumerate(f[lang])]


def split(rs: list[dict]) -> dict[str, list[dict]]:
    found = sorted({r["family"] for r in rs if r["notes"]})
    none = sorted({r["family"] for r in rs if not r["notes"]})
    dev = set()
    for name, fams in (("found", found), ("none", none)):
        order = seeded_order(fams, f"{SEED}:{name}")
        dev |= set(order[: (len(order) + 1) // 2])
    return {"dev": sorted((r for r in rs if r["family"] in dev), key=lambda r: r["id"]),
            "test": sorted((r for r in rs if r["family"] not in dev), key=lambda r: r["id"])}


def _sha(paths) -> str:
    h = hashlib.sha256()
    for p in sorted(paths):
        h.update(p.relative_to(DATA).as_posix().encode() + b"\0" + p.read_bytes())
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
    fingerprint_rows = [{"text": r["text"], "label": "|".join(r["notes"]) or "none"} for r in parts["dev"] + parts["test"]]
    manifest = {
        "dataset": "kestrel-notesq-v0", "status": "reviewed" if reviewed() else "draft", "seed": SEED,
        "sources": {"queries.toml": _sha([DATA / "queries.toml"]), "notes/": _sha(NOTES.rglob("*.md"))},
        "fingerprint": dataset_fingerprint(fingerprint_rows),
        "splits": {name: {"families": len({r["family"] for r in part}), "queries": len(part),
                          "none_queries": sum(not r["notes"] for r in part)}
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
