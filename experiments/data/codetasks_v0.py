#!/usr/bin/env python3
"""experiments/data/codetasks_v0.py -- build Kestrel-CodeTasks-v0 (v3 code loop, ADR-036): small code requests.

    python experiments/data/codetasks_v0.py --check     # validate datasets/kestrel_codetasks_v0/tasks.toml
    python experiments/data/codetasks_v0.py             # write datasets/kestrel_codetasks_v0/splits/

Four types, each family in English and Romanized Hinglish: `function` (the request names the function;
hidden asserts call it), `script` (reads stdin, prints; hidden stdin/stdout cases), `fix` (the request shows
broken code, {code}; hidden asserts), `explain` (the request shows code; a right answer holds every fact,
any of its ways). The reference solutions stay in tasks.toml (experiments/v3/test_code_eval.py runs them
in the sandbox: each passes, and each broken `fix` code fails); rows carry no solution. Families are split
whole into dev / test, half each per type (dev takes the odd one), in seeded order. Output is sorted: a
build is the same bytes every time.
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
DATA = ROOT / "datasets" / "kestrel_codetasks_v0"
TYPES = ("function", "script", "fix", "explain")
LANGS = ("en", "hi-en")
SEED = 0
CARRIED = ("entry", "code", "tests", "cases", "facts")


def load(path: Path = DATA / "tasks.toml") -> list[dict]:
    return tomllib.loads(Path(path).read_text(encoding="utf-8"))["family"]


def status(path: Path = DATA / "tasks.toml") -> str:
    """"reviewed" by the human (`default_reviewed = true`), "claude-reviewed" when the human waived the review
    (`reviewed_by = "claude"`, ADR-045), else "draft"."""
    head = tomllib.loads(Path(path).read_text(encoding="utf-8"))
    if head.get("default_reviewed"):
        return "reviewed"
    return "claude-reviewed" if head.get("reviewed_by") == "claude" else "draft"


def validate(families: list[dict]) -> list[str]:
    problems, seen = [], set()
    for f in families:
        fid, t = f.get("id", "?"), f.get("type")
        if fid in seen:
            problems.append(f"{fid}: id used twice")
        seen.add(fid)
        if t not in TYPES:
            problems.append(f"{fid}: type {t!r} is not one of {', '.join(TYPES)}")
            continue
        if not all(f.get(lang) for lang in LANGS):
            problems.append(f"{fid}: needs at least one en and one hi-en request")
        requests = [r for lang in LANGS for r in f.get(lang, [])]
        if t in ("function", "fix"):
            if not (f.get("solution") and f.get("tests")):
                problems.append(f"{fid}: a {t} task needs its solution and tests")
            if t == "function":
                problems += [f"{fid}: request {r!r} does not name {f.get('entry')}" for r in requests
                             if not f.get("entry") or f["entry"] not in r]
        if t in ("fix", "explain"):
            problems += [f"{fid}: request {r!r} does not show {{code}}" for r in requests if "{code}" not in r]
            if not f.get("code"):
                problems.append(f"{fid}: a {t} task needs its code")
        if t == "script" and not (f.get("solution") and f.get("cases")):
            problems.append(f"{fid}: a script task needs its solution and cases")
        if t == "explain" and not (f.get("facts") and all(f["facts"])):
            problems.append(f"{fid}: an explain task needs its facts")
    return problems


def rows(families: list[dict]) -> list[dict]:
    return [{"id": f"{f['id']}/{lang}/{i}", "family": f["id"], "type": f["type"], "lang": lang,
             "text": text.replace("{code}", f.get("code", "")), **{k: f[k] for k in CARRIED if k in f}}
            for f in families for lang in LANGS for i, text in enumerate(f[lang])]


def split(rs: list[dict]) -> dict[str, list[dict]]:
    dev = set()
    for t in TYPES:
        order = seeded_order(sorted({r["family"] for r in rs if r["type"] == t}), f"{SEED}:{t}")
        dev |= set(order[: (len(order) + 1) // 2])
    return {"dev": sorted((r for r in rs if r["family"] in dev), key=lambda r: r["id"]),
            "test": sorted((r for r in rs if r["family"] not in dev), key=lambda r: r["id"])}


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
    labelled = [{"text": r["text"], "label": r["type"]} for r in parts["dev"] + parts["test"]]
    manifest = {
        "dataset": "kestrel-codetasks-v0", "status": status(), "seed": SEED,
        "sources": {"tasks.toml": hashlib.sha256((DATA / "tasks.toml").read_bytes()).hexdigest()},
        "fingerprint": dataset_fingerprint(labelled),
        "splits": {name: {"families": len({r["family"] for r in part}), "requests": len(part),
                          "by_type": {t: sum(r["type"] == t for r in part) for t in TYPES}}
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
