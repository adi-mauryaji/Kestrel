#!/usr/bin/env python3
"""experiments/data/kinds_v2.py -- the Kestrel-Kinds-v2 draft: the `code` kind (ADR-044).

    python experiments/data/kinds_v2.py --check     # validate datasets/kestrel_kinds_v2/families.toml
    python experiments/data/kinds_v2.py             # write datasets/kestrel_kinds_v2/splits/

Kinds-v2 = Kestrel-Kinds-v1 (frozen, tag kinds-v1-frozen) + the families of families.toml: `code` (write or fix
Python; Qwen3-4B), its boundaries (explaining code and programming concepts are `chat`, for Gemma; a task about
code or a named notes file is `app`), and more `notes`, `rewrite` and `chat` families of the types Kinds-v1's test
missed (968764b). Every Kinds-v1 row keeps its split; the new families are split whole into train / cal / val /
test (prep.group_split4, seed 0), stratified by kind, so the test split holds fresh families. Kinds-v1's test rows
were seen once (968764b) and its misses shaped these drafts: the manifest counts the fresh test items apart.
families.toml must not repeat a Kinds-v0 / Kinds-v1 id, or a text already in them or in Kestrel-Intents-v0's train
/ cal / val. Labels: datasets/contracts/kinds_v2.txt. Output is sorted: a build is the same bytes every time.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path

import kinds_v0
import kinds_v1
from prep import NAMES, dataset_fingerprint, dedupe, group_split4, norm_text

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets" / "kestrel_kinds_v2"
CONTRACT = ROOT / "datasets" / "contracts" / "kinds_v2.txt"
LABELS = [l for l in CONTRACT.read_text().splitlines() if l and not l.startswith("#")]
KINDS = tuple(LABELS)
LANGS = ("en", "hi-en")
SEED = 0
REVIEW = ROOT / "experiments" / "logs" / "v3" / "v2" / "review.json"


def load(path: Path = DATA / "families.toml") -> list[dict]:
    return tomllib.loads(Path(path).read_text(encoding="utf-8"))["family"]


def _known() -> tuple[set[str], set[str]]:
    """(ids, normalized texts) already in Kinds-v0, Kinds-v1 or Intents-v0's train / cal / val."""
    old = kinds_v0.load() + kinds_v1.load()
    texts = {norm_text(t) for f in old for lang in LANGS for t in f[lang]} | kinds_v1._known_texts()
    return {f["id"] for f in old}, texts


def validate(families: list[dict]) -> list[str]:
    problems, seen = [], set()
    old_ids, known = _known()
    for f in families:
        fid = f.get("id", "?")
        if fid in seen:
            problems.append(f"{fid}: id used twice")
        seen.add(fid)
        if fid in old_ids:
            problems.append(f"{fid}: id already in Kinds-v0 or Kinds-v1")
        if f.get("kind") not in KINDS:
            problems.append(f"{fid}: kind {f.get('kind')!r} is not one of {', '.join(KINDS)}")
        if not all(f.get(lang) for lang in LANGS):
            problems.append(f"{fid}: needs at least one en and one hi-en request")
        problems += [f"{fid}: {t!r} is already in Kinds-v0, Kinds-v1 or Intents-v0"
                     for lang in LANGS for t in f.get(lang, []) if norm_text(t) in known]
    return problems


def rows(families: list[dict]) -> tuple[list[dict], list]:
    """(rows, conflicts): Kinds-v1's rows, then these families' phrasings, deduplicated."""
    v1, _ = kinds_v1.rows(kinds_v1.load())
    new = [{"id": f"{f['id']}/{lang}/{i}", "text": text, "label": f["kind"], "lang": lang, "family": f["id"],
            "group": f["id"], "stratum": f"v2-{f['kind']}", "source": "kinds_v2"}
           for f in families for lang in LANGS for i, text in enumerate(f[lang])]
    return dedupe(v1 + new)


def split(rs: list[dict]) -> dict[str, list[dict]]:
    """Kinds-v1's rows as frozen; the new families whole, by kind."""
    parts = kinds_v1.split([r for r in rs if r["source"] != "kinds_v2"])
    new = [r for r in rs if r["source"] == "kinds_v2"]
    idx = group_split4(new, "group", "stratum", seed=SEED)
    return {name: sorted(parts[name] + [new[i] for i in idx[name]], key=lambda r: r["id"]) for name in NAMES}


def reviewed() -> bool:
    if not (kinds_v1.reviewed() and REVIEW.exists()):
        return False
    return bool(json.loads(REVIEW.read_text())["status"].get("kindsv2", {}).get("ready"))


def status(path: Path = DATA / "families.toml") -> str:
    """"reviewed" by the human (the review log all OK), "claude-reviewed" when the human waived the review
    (`reviewed_by = "claude"`, ADR-045), else "draft"."""
    if reviewed():
        return "reviewed"
    head = tomllib.loads(Path(path).read_text(encoding="utf-8"))
    return "claude-reviewed" if head.get("reviewed_by") == "claude" else "draft"


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
        "dataset": "kestrel-kinds-v2",
        "status": status(),
        "seed": SEED,
        "labels": LABELS,
        "sources": {"kestrel_kinds_v1/splits/manifest.json": _sha(kinds_v1.DATA / "splits" / "manifest.json"),
                    "families.toml": _sha(DATA / "families.toml"), "contracts/kinds_v2.txt": _sha(CONTRACT)},
        "conflicts_dropped": [[text, labels] for text, labels in conflicts],
        "splits": {name: {"items": len(part), "fresh_items": sum(r["source"] == "kinds_v2" for r in part),
                          "fingerprint": dataset_fingerprint(part),
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
