"""experiments/data/tasks_v0.py -- build Kestrel-Tasks-v0 from tasks.toml (Ch. 31, ADR-023).

    python experiments/data/tasks_v0.py --check      # validate datasets/kestrel_tasks_v0/tasks.toml
    python experiments/data/tasks_v0.py              # write datasets/kestrel_tasks_v0/build/

A *family* is one request in English and Romanized Hinglish with one gold plan and its expected end
state; families are split whole into dev and test, per category (seeded hash order), so a
translation never sits in another set than its original. The malformed plans are not split: they
test the validator, not a model.

Out (build/): items.jsonl (one row per family x language x phrasing, with its split, parsed gold plan
and expectations), malformed.jsonl, fixture.json (today, notes folder, seed tasks and memories),
manifest.json. The Rust check crates/kestrel-tools/tests/tasks_v0.rs reads them: every gold plan
validates and reaches its expected end state on a fresh copy of the fixtures, every malformed plan
is rejected with its message. Python only checks the structure; the Rust validator is the judge.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import tomllib
from collections import Counter
from pathlib import Path

from prep import seeded_order

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets" / "kestrel_tasks_v0"
LANGS = ("en", "hi-en")
CATEGORIES = ("create_task", "search_notes", "summarize_notes", "delete_task", "read_document",
              "set_preference", "retrieve_memory", "compound")
FAMILY_KEYS = {"id", "category", "plan", "expect", "note", "author", "reviewed", *LANGS}
EXPECT_KEYS = {"outcome", "tasks_added", "tasks_deleted", "prefs_set", "read", "answer_contains"}
OUTCOMES = ("done", "refused", "clarify")
MALFORMED_KEYS = {"id", "why", "plan", "error"}


def load(path: Path = DATA / "tasks.toml") -> dict:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    data.setdefault("family", [])
    data.setdefault("malformed", [])
    for f in data["family"]:
        f.setdefault("author", data.get("default_author", "claude-draft"))
        f.setdefault("reviewed", data.get("default_reviewed", False))
        f.setdefault("expect", {})
    return {"fixture": data["fixture"], "families": data["family"], "malformed": data["malformed"],
            "path": path}


def _is_date(s) -> bool:
    import datetime as dt
    try:
        return isinstance(s, str) and len(s) == 10 and dt.date.fromisoformat(s).isoformat() == s
    except ValueError:
        return False


def validate(data: dict) -> list[str]:
    errors = []
    notes = data["path"].parent / data["fixture"]["notes"]
    if not notes.is_dir():
        errors.append(f"fixture notes folder {notes} is missing")
    seen = Counter(f.get("id") for f in data["families"])
    errors += [f"duplicate family id {i}" for i, n in seen.items() if n > 1]
    for f in data["families"]:
        fid = f.get("id", "?")
        errors += [f"{fid}: unknown key {k}" for k in set(f) - FAMILY_KEYS]
        if f.get("category") not in CATEGORIES:
            errors.append(f"{fid}: category {f.get('category')!r} is not one of {CATEGORIES}")
        for lang in LANGS:
            texts = f.get(lang)
            if not (isinstance(texts, list) and texts and all(isinstance(t, str) and t.strip() for t in texts)):
                errors.append(f"{fid}: {lang} needs at least one non-empty request")
        exp = f.get("expect", {})
        errors += [f"{fid}: unknown expect key {k}" for k in set(exp) - EXPECT_KEYS]
        outcome = exp.get("outcome", "done")
        if outcome not in OUTCOMES:
            errors.append(f"{fid}: outcome {outcome!r}")
        if outcome == "clarify":
            if "plan" in f:
                errors.append(f"{fid}: a clarify family has no plan")
        elif "plan" not in f:
            errors.append(f"{fid}: no plan")
        else:
            try:
                plan = json.loads(f["plan"])
                if not isinstance(plan.get("nodes"), list):
                    errors.append(f"{fid}: plan has no nodes list")
            except (json.JSONDecodeError, AttributeError) as e:
                errors.append(f"{fid}: plan is not JSON ({e})")
        for t in exp.get("tasks_added", []):
            if not isinstance(t.get("title"), str) or not t["title"].strip():
                errors.append(f"{fid}: tasks_added needs titles")
            if "due" in t and not _is_date(t["due"]):
                errors.append(f"{fid}: tasks_added due {t['due']!r} is not YYYY-MM-DD")
        for name in exp.get("read", []):
            if not (notes / name).is_file():
                errors.append(f"{fid}: read names {name}, which is not a fixture note")
    seen = Counter(m.get("id") for m in data["malformed"])
    errors += [f"duplicate malformed id {i}" for i, n in seen.items() if n > 1]
    for m in data["malformed"]:
        mid = m.get("id", "?")
        errors += [f"{mid}: unknown key {k}" for k in set(m) - MALFORMED_KEYS]
        if not (m.get("plan") and m.get("error") and m.get("why")):
            errors.append(f"{mid}: needs plan, error and why")
    return errors


def assign_splits(families, seed: int = 0) -> dict[str, str]:
    """Family id -> dev | test, per category in seeded hash order; test gets the odd one."""
    out = {}
    for cat in CATEGORIES:
        ids = seeded_order(sorted(f["id"] for f in families if f["category"] == cat), f"{seed}:{cat}")
        for k, fid in enumerate(ids):
            out[fid] = "dev" if k < len(ids) // 2 else "test"
    return out


def items(families, splits) -> list[dict]:
    rows = []
    for f in sorted(families, key=lambda f: f["id"]):
        exp = f["expect"]
        expect = {"outcome": exp.get("outcome", "done"), "tasks_added": exp.get("tasks_added", []),
                  "tasks_deleted": exp.get("tasks_deleted", []), "prefs_set": exp.get("prefs_set", {}),
                  "read": exp.get("read", []), "answer_contains": exp.get("answer_contains")}
        plan = json.loads(f["plan"]) if "plan" in f else None
        for lang in LANGS:
            for k, text in enumerate(f[lang]):
                rows.append({"id": f"{f['id']}/{lang}/{k}", "family": f["id"], "category": f["category"],
                             "lang": lang, "text": text, "split": splits[f["id"]], "plan": plan, "expect": expect})
    return rows


def _notes_fingerprint(notes: Path) -> str:
    h = hashlib.sha256()
    for p in sorted(x for x in notes.rglob("*") if x.is_file()):
        h.update(f"{p.relative_to(notes).as_posix()}\0".encode() + hashlib.sha256(p.read_bytes()).digest())
    return h.hexdigest()


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build(out_dir: Path = DATA / "build", seed: int = 0) -> dict:
    data = load()
    errors = validate(data)
    if errors:
        raise SystemExit("invalid tasks.toml:\n  " + "\n  ".join(errors))
    splits = assign_splits(data["families"], seed)
    rows = items(data["families"], splits)
    out_dir.mkdir(parents=True, exist_ok=True)
    write = lambda name, rs: (out_dir / name).write_text(  # noqa: E731
        "".join(json.dumps(r, ensure_ascii=False, sort_keys=True) + "\n" for r in rs), encoding="utf-8")
    write("items.jsonl", rows)
    write("malformed.jsonl", sorted(data["malformed"], key=lambda m: m["id"]))
    notes = (DATA / data["fixture"]["notes"]).relative_to(ROOT).as_posix()
    fixture = {"today": data["fixture"]["today"], "notes": notes, "tasks": data["fixture"]["tasks"],
               "memories": data["fixture"]["memories"]}
    (out_dir / "fixture.json").write_text(json.dumps(fixture, indent=1, ensure_ascii=False, sort_keys=True) + "\n")
    rows_by = lambda split: [r for r in rows if r["split"] == split]  # noqa: E731
    manifest = {
        "dataset": "kestrel-tasks-v0",
        "status": "draft" if any(not f["reviewed"] for f in data["families"]) else "reviewed",
        "seed": seed,
        "families": len(data["families"]),
        "unreviewed_families": sum(not f["reviewed"] for f in data["families"]),
        "malformed": len(data["malformed"]),
        "sources": {"tasks.toml": _sha256(DATA / "tasks.toml"), "fixtures/notes": _notes_fingerprint(DATA / data["fixture"]["notes"])},
        "items_sha256": _sha256(out_dir / "items.jsonl"),
        "splits": {s: {"families": sum(v == s for v in splits.values()), "items": len(rows_by(s)),
                       "by_category": dict(sorted(Counter(r["category"] for r in rows_by(s)).items()))}
                   for s in ("dev", "test")},
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=1, ensure_ascii=False) + "\n")
    return manifest


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="validate only")
    args = ap.parse_args(argv)
    if args.check:
        errors = validate(load())
        print("\n".join(errors) if errors else "ok")
        raise SystemExit(1 if errors else 0)
    print(json.dumps(build(), indent=1))


if __name__ == "__main__":
    main()
