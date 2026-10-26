#!/usr/bin/env python3
"""experiments/data/kinds_v0.py -- build Kestrel-Kinds-v0 (ADR-037): which skill a request asks for.

    python experiments/data/kinds_v0.py --check     # validate datasets/kestrel_kinds_v0/kinds.toml
    python experiments/data/kinds_v0.py             # write datasets/kestrel_kinds_v0/splits/

Rows: every phrasing of every kinds.toml family (label = its kind), plus every Kestrel-Intents-v0
train / cal / val item as `app` (its test split is never read). Families are split whole into
train / cal / val / test (prep.group_split4, seed 0), stratified by kind; the borrowed items by their
Intents-v0 intent and grouped as Intents-v0 groups them, so every split holds every intent; Claude's
`app` boundary families are their own stratum. A family with `split = "<name>"` is pinned there and left out
of the shuffle, so adding it moves no other family (families written after looking at dev errors are
pinned to train: val and test stay the sets earlier heads were measured on; such rows carry "pinned").
A duplicate text keeps its first copy; a text with two
labels is dropped and listed in the manifest. Output is sorted: a build is the same bytes every time.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path

from prep import NAMES, dataset_fingerprint, dedupe, group_split4

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets" / "kestrel_kinds_v0"
CONTRACT = ROOT / "datasets" / "contracts" / "kinds.txt"
INTENTS = ROOT / "datasets" / "kestrel_intents_v0" / "splits"
LANGS = ("en", "hi-en")
LABELS = [l for l in CONTRACT.read_text().splitlines() if l and not l.startswith("#")]
SEED = 0


def load(path: Path = DATA / "kinds.toml") -> list[dict]:
    return tomllib.loads(Path(path).read_text(encoding="utf-8"))["family"]


def validate(families: list[dict]) -> list[str]:
    problems, seen = [], set()
    for f in families:
        fid = f.get("id", "?")
        if fid in seen:
            problems.append(f"{fid}: id used twice")
        seen.add(fid)
        if f.get("kind") not in LABELS:
            problems.append(f"{fid}: kind {f.get('kind')!r} is not in datasets/contracts/kinds.txt")
        if not all(f.get(lang) for lang in LANGS):
            problems.append(f"{fid}: needs at least one en and one hi-en request")
        if "split" in f and f["split"] not in NAMES:
            problems.append(f"{fid}: split {f['split']!r} is not one of {', '.join(NAMES)}")
    return problems


def rows(families: list[dict]) -> tuple[list[dict], list]:
    """(rows, conflicts): the kinds.toml phrasings and the borrowed Intents-v0 dev items, deduplicated."""
    out = []
    for f in families:
        stratum = "app-boundary" if f["kind"] == "app" else f["kind"]
        pin = {"pinned": f["split"]} if "split" in f else {}
        for lang in LANGS:
            for i, text in enumerate(f[lang]):
                out.append({"id": f"{f['id']}/{lang}/{i}", "text": text, "label": f["kind"], "lang": lang,
                            "family": f["id"], "group": f["id"], "stratum": stratum, "source": "kinds_v0"} | pin)
    for name in ("train", "cal", "val"):
        for line in (INTENTS / f"{name}.jsonl").open(encoding="utf-8"):
            r = json.loads(line)
            intent = r["label"] if len(r["intents"]) == 1 else "compound"
            out.append({"id": "intents:" + r["id"], "text": r["text"], "label": "app", "lang": r["lang"],
                        "family": "intents:" + r["family"], "group": "intents:" + r["group"],
                        "stratum": f"app-intents:{intent}", "source": "intents_v0"})
    return dedupe(out)


def split(rs: list[dict]) -> dict[str, list[dict]]:
    free = [r for r in rs if "pinned" not in r]
    idx = group_split4(free, "group", "stratum", seed=SEED)
    return {name: sorted([free[i] for i in idx[name]] + [r for r in rs if r.get("pinned") == name],
                         key=lambda r: r["id"]) for name in NAMES}


def _sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build(out: Path = DATA / "splits") -> dict:
    families = load()
    problems = validate(families)
    if problems:
        raise ValueError("\n".join(problems))
    rs, conflicts = rows(families)
    parts = split(rs)
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    for name, part in parts.items():
        text = "".join(json.dumps(r, ensure_ascii=False, sort_keys=True) + "\n" for r in part)
        (out / f"{name}.jsonl").write_text(text, encoding="utf-8")
    manifest = {
        "dataset": "kestrel-kinds-v0",
        "status": "draft",
        "seed": SEED,
        "labels": LABELS,
        "sources": {"kinds.toml": _sha(DATA / "kinds.toml"), "contracts/kinds.txt": _sha(CONTRACT),
                    **{f"kestrel_intents_v0/splits/{n}.jsonl": _sha(INTENTS / f"{n}.jsonl")
                       for n in ("train", "cal", "val")}},
        "conflicts_dropped": [[text, labels] for text, labels in conflicts],
        "splits": {name: {"items": len(part), "fingerprint": dataset_fingerprint(part),
                          "by_label": {lab: sum(r["label"] == lab for r in part) for lab in LABELS},
                          "by_lang": {lang: sum(r["lang"] == lang for r in part) for lang in LANGS}}
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
