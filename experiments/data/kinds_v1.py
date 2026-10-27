#!/usr/bin/env python3
"""experiments/data/kinds_v1.py -- the Kestrel-Kinds-v1 draft: the `notes` kind (ADR-041).

    python experiments/data/kinds_v1.py --check     # validate datasets/kestrel_kinds_v1/notes.toml
    python experiments/data/kinds_v1.py             # write datasets/kestrel_kinds_v1/splits/

Kinds-v1 = Kestrel-Kinds-v0 (frozen, tag kinds-v0-frozen) + the families of notes.toml (the `notes` kind
and its app / chat boundaries). Every Kinds-v0 row keeps the split it was frozen with; the new families are
split whole into train / cal / val / test (prep.group_split4, seed 0), stratified by kind. notes.toml must
not repeat a Kinds-v0 family id, or a text already in Kinds-v0 or in Kestrel-Intents-v0's train / cal / val
(whose items are Kinds' `app`). Labels: datasets/contracts/kinds_v1.txt. Output is sorted: a build is the
same bytes every time. The manifest says "reviewed" when Kinds-v0 is and the human's notes review marked
every notes.toml family OK.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path

import kinds_v0
from kinds_v0 import INTENTS
from kinds_v0 import load as load_v0
from prep import NAMES, dataset_fingerprint, dedupe, group_split4, norm_text

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets" / "kestrel_kinds_v1"
CONTRACT = ROOT / "datasets" / "contracts" / "kinds_v1.txt"
LABELS = [l for l in CONTRACT.read_text().splitlines() if l and not l.startswith("#")]
KINDS = ("app", "chat", "email", "rewrite", "translate", "notes")
LANGS = ("en", "hi-en")
SEED = 0
REVIEW = ROOT / "experiments" / "logs" / "v3" / "notes" / "review.json"


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


def rows(families: list[dict]) -> tuple[list[dict], list]:
    """(rows, conflicts): Kinds-v0's rows, then these families' phrasings, deduplicated."""
    v0, _ = kinds_v0.rows(load_v0())
    new = [{"id": f"{f['id']}/{lang}/{i}", "text": text, "label": f["kind"], "lang": lang, "family": f["id"],
            "group": f["id"], "stratum": f"v1-{f['kind']}", "source": "kinds_v1"}
           for f in families for lang in LANGS for i, text in enumerate(f[lang])]
    return dedupe(v0 + new)


def split(rs: list[dict]) -> dict[str, list[dict]]:
    """Kinds-v0's rows as frozen; the new families whole, by kind."""
    parts = kinds_v0.split([r for r in rs if r["source"] != "kinds_v1"])
    new = [r for r in rs if r["source"] == "kinds_v1"]
    idx = group_split4(new, "group", "stratum", seed=SEED)
    return {name: sorted(parts[name] + [new[i] for i in idx[name]], key=lambda r: r["id"]) for name in NAMES}


def reviewed() -> bool:
    if not (kinds_v0.reviewed() and REVIEW.exists()):
        return False
    s = json.loads(REVIEW.read_text())["status"].get("kindsv1", {})
    return bool(s.get("ready"))


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
    v0 = ROOT / "datasets" / "kestrel_kinds_v0"
    manifest = {
        "dataset": "kestrel-kinds-v1",
        "status": "reviewed" if reviewed() else "draft",
        "seed": SEED,
        "labels": LABELS,
        "sources": {"kestrel_kinds_v0/kinds.toml": _sha(v0 / "kinds.toml"), "notes.toml": _sha(DATA / "notes.toml"),
                    "contracts/kinds_v1.txt": _sha(CONTRACT),
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
