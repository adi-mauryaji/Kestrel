#!/usr/bin/env python3
"""experiments/data/kinds_v1.py -- the Kestrel-Kinds-v1 draft: the `notes` kind (ADR-041).

    python experiments/data/kinds_v1.py --check     # validate datasets/kestrel_kinds_v1/notes.toml

Kinds-v1 = Kestrel-Kinds-v0 (once the human has reviewed and frozen it) + these families; the build comes
then. Until that, the draft is validated against what it must not repeat: a Kinds-v0 family id, or a text
already in Kinds-v0 or in Kestrel-Intents-v0's train / cal / val (whose items are Kinds' `app`).
"""
from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

from kinds_v0 import INTENTS
from kinds_v0 import load as load_v0
from prep import norm_text

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets" / "kestrel_kinds_v1"
KINDS = ("app", "chat", "email", "rewrite", "translate", "notes")
LANGS = ("en", "hi-en")


def load(path: Path = DATA / "notes.toml") -> list[dict]:
    return tomllib.loads(Path(path).read_text(encoding="utf-8"))["family"]


def _known_texts() -> set[str]:
    texts = {norm_text(t) for f in load_v0() for lang in LANGS for t in f[lang]}
    for name in ("train", "cal", "val"):
        texts |= {norm_text(json.loads(l)["text"]) for l in (INTENTS / f"{name}.jsonl").open(encoding="utf-8")}
    return texts


def validate(families: list[dict]) -> list[str]:
    problems, seen = [], set()
    v0_ids = {f["id"] for f in load_v0()}
    known = _known_texts()
    for f in families:
        fid = f.get("id", "?")
        if fid in seen:
            problems.append(f"{fid}: id used twice")
        seen.add(fid)
        if fid in v0_ids:
            problems.append(f"{fid}: id already in Kinds-v0")
        if f.get("kind") not in KINDS:
            problems.append(f"{fid}: kind {f.get('kind')!r} is not one of {', '.join(KINDS)}")
        if not all(f.get(lang) for lang in LANGS):
            problems.append(f"{fid}: needs at least one en and one hi-en request")
        problems += [f"{fid}: {t!r} is already in Kinds-v0 or Intents-v0"
                     for lang in LANGS for t in f.get(lang, []) if norm_text(t) in known]
    return problems


def main(argv=None) -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="validate only (the only mode until Kinds-v0 is frozen)")
    ap.parse_args(argv)
    problems = validate(load())
    if problems:
        sys.exit("\n".join(problems))
    print("ok")


if __name__ == "__main__":
    main()
