"""experiments/data/memory_v0.py -- build Kestrel-Memory-v0 from memory.toml (Ch. 18, ADR-026).

    python experiments/data/memory_v0.py --check      # validate datasets/kestrel_memory_v0/memory.toml
    python experiments/data/memory_v0.py              # write datasets/kestrel_memory_v0/build/

One seeded memory store and retrieval queries for the Week-10 baseline recall@k. Every query runs
against the whole store. Queries are split dev/test by *group*: a group's memories are asked about
in one split only, so tuning on dev never sees a test memory's questions. Topic groups and the
negative groups (`neg-*`, no memories) are split separately (seeded hash order, the first half dev).

Out (build/): store.jsonl (memories in file order with `row` = their SQLite id when loaded in order,
Unix times, approval), queries.jsonl (with split), manifest.json. The Rust check
crates/kestrel-memory/tests/memory_v0.rs verifies the lexical / paraphrase labels against the real
retrieval terms; crates/kestrel-memory/examples/recall_eval.rs measures recall@k.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import re
import tomllib
from collections import Counter
from pathlib import Path

from prep import seeded_order

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets" / "kestrel_memory_v0"
DAY = 86_400
KINDS = ("preference", "fact", "summary", "task_history")
SOURCES = ("user_explicit", "correction", "model_inferred")
LANGS = ("en", "hi-en")
TYPES = ("lexical", "paraphrase", "negative")
MEMORY_KEYS = {"id", "group", "kind", "key", "value", "source", "importance", "age_days", "expires_in_days",
               "pending", "note", "author", "reviewed"}
QUERY_KEYS = {"id", "group", "lang", "type", "text", "expect", "trap", "note", "author", "reviewed"}
KEY = re.compile(r"[a-z][a-z0-9_]{0,63}")


def _unix(day: str) -> int:
    return int(dt.datetime.fromisoformat(day).replace(tzinfo=dt.timezone.utc).timestamp())


def load(path: Path = DATA / "memory.toml") -> dict:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    for row in data.get("memory", []) + data.get("query", []):
        row.setdefault("author", data.get("default_author", "claude-draft"))
        row.setdefault("reviewed", data.get("default_reviewed", False))
    for q in data.get("query", []):
        q.setdefault("trap", [])
    return {"now": data["now"], "now_unix": _unix(data["now"]), "memories": data.get("memory", []),
            "queries": data.get("query", []), "path": path}


def _gone(m: dict) -> bool:
    """Pending (never approved) or expired at `now`: retrieval must never return it."""
    return bool(m.get("pending")) or m.get("expires_in_days", 1) <= 0


def validate(data: dict) -> list[str]:
    errors = []
    mems = {}
    for m in data["memories"]:
        mid = m.get("id", "?")
        if mid in mems:
            errors.append(f"memory {mid}: duplicate id")
        mems[mid] = m
        for k in sorted(set(m) - MEMORY_KEYS):
            errors.append(f"memory {mid}: unknown field {k}")
        for k in ("group", "kind", "key", "value", "source", "importance", "age_days"):
            if k not in m:
                errors.append(f"memory {mid}: missing {k}")
        if m.get("kind") not in KINDS:
            errors.append(f"memory {mid}: kind {m.get('kind')!r}")
        if m.get("source") not in SOURCES:
            errors.append(f"memory {mid}: source {m.get('source')!r}")
        if not KEY.fullmatch(str(m.get("key", ""))):
            errors.append(f"memory {mid}: key {m.get('key')!r} is not [a-z][a-z0-9_]*")
        if not isinstance(m.get("importance"), int) or not 1 <= m["importance"] <= 5:
            errors.append(f"memory {mid}: importance {m.get('importance')!r} not in 1..5")
        if not isinstance(m.get("age_days"), int) or m["age_days"] < 0:
            errors.append(f"memory {mid}: age_days {m.get('age_days')!r}")
        if m.get("pending") and m.get("source") != "model_inferred":
            errors.append(f"memory {mid}: only a model_inferred memory can be pending")
        if m.get("group", "").startswith("neg-"):
            errors.append(f"memory {mid}: group {m['group']} is for negatives")
    keys = Counter((m.get("kind"), m.get("key")) for m in data["memories"])
    errors += [f"memory key {k}: duplicate (kind, key)" for k, n in keys.items() if n > 1]
    topics = {m.get("group") for m in data["memories"]}
    seen = set()
    for q in data["queries"]:
        qid = q.get("id", "?")
        if qid in seen:
            errors.append(f"query {qid}: duplicate id")
        seen.add(qid)
        for k in sorted(set(q) - QUERY_KEYS):
            errors.append(f"query {qid}: unknown field {k}")
        if q.get("lang") not in LANGS:
            errors.append(f"query {qid}: lang {q.get('lang')!r}")
        if q.get("type") not in TYPES:
            errors.append(f"query {qid}: type {q.get('type')!r}")
        if not str(q.get("text", "")).strip():
            errors.append(f"query {qid}: empty text")
        group = q.get("group", "")
        if q.get("type") == "negative" and group.startswith("neg-"):
            pass
        elif group not in topics:
            errors.append(f"query {qid}: group {group!r} has no memories")
        expect = q.get("expect", [])
        if (q.get("type") == "negative") != (expect == []):
            errors.append(f"query {qid}: a negative query and only a negative has expect = []")
        for mid in expect + q["trap"]:
            if mid not in mems:
                errors.append(f"query {qid}: unknown memory {mid}")
        for mid in expect:
            if mid in mems and _gone(mems[mid]):
                errors.append(f"query {qid}: expects {mid}, which is pending or expired")
        for mid in q["trap"]:
            if mid in mems and not _gone(mems[mid]):
                errors.append(f"query {qid}: trap {mid} is neither pending nor expired")
    return errors


def assign_splits(queries, seed: int = 0) -> dict[str, str]:
    """group -> dev/test: topic groups and negative groups each ordered by seeded hash, first half dev."""
    splits = {}
    for cat, groups in (("topic", {q["group"] for q in queries if not q["group"].startswith("neg-")}),
                        ("negative", {q["group"] for q in queries if q["group"].startswith("neg-")})):
        order = seeded_order(sorted(groups), f"{seed}:{cat}")
        half = (len(order) + 1) // 2
        splits |= {g: ("dev" if i < half else "test") for i, g in enumerate(order)}
    return splits


def store_rows(memories, now: str) -> list[dict]:
    """The store in file order; `row` is the id SQLite gives when they are inserted in this order."""
    t = _unix(now)
    rows = []
    for row, m in enumerate(memories, start=1):
        exp = m.get("expires_in_days")
        rows.append({"row": row, "id": m["id"], "group": m["group"], "kind": m["kind"], "key": m["key"],
                     "value": m["value"], "source": m["source"], "importance": m["importance"],
                     "approved": not m.get("pending", False), "updated_at": t - m["age_days"] * DAY,
                     "expires_at": None if exp is None else t + exp * DAY})
    return rows


def query_rows(queries, splits) -> list[dict]:
    return [{"id": q["id"], "group": q["group"], "lang": q["lang"], "type": q["type"], "text": q["text"],
             "expect": q["expect"], "trap": q["trap"], "split": splits[q["group"]]}
            for q in sorted(queries, key=lambda q: q["id"])]


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build(out_dir: Path = DATA / "build", seed: int = 0) -> dict:
    data = load()
    errors = validate(data)
    if errors:
        raise SystemExit("invalid memory.toml:\n  " + "\n  ".join(errors))
    splits = assign_splits(data["queries"], seed)
    store = store_rows(data["memories"], data["now"])
    queries = query_rows(data["queries"], splits)
    out_dir.mkdir(parents=True, exist_ok=True)
    write = lambda name, rs: (out_dir / name).write_text(  # noqa: E731
        "".join(json.dumps(r, ensure_ascii=False, sort_keys=True) + "\n" for r in rs), encoding="utf-8")
    write("store.jsonl", store)
    write("queries.jsonl", queries)
    per = lambda split: [q for q in queries if q["split"] == split]  # noqa: E731
    rows = data["memories"] + data["queries"]
    manifest = {
        "dataset": "kestrel-memory-v0",
        "status": "draft" if any(not r["reviewed"] for r in rows) else "reviewed",
        "seed": seed,
        "now": data["now"],
        "now_unix": data["now_unix"],
        "memories": len(store),
        "pending_or_expired": sum(_gone(m) for m in data["memories"]),
        "queries": len(queries),
        "splits": {s: {"queries": len(per(s)), "groups": sorted({q["group"] for q in per(s)}),
                       "types": dict(sorted(Counter(q["type"] for q in per(s)).items())),
                       "langs": dict(sorted(Counter(q["lang"] for q in per(s)).items()))}
                   for s in ("dev", "test")},
        "sources": {"memory.toml": _sha256(DATA / "memory.toml")},
        "store_sha256": _sha256(out_dir / "store.jsonl"),
        "queries_sha256": _sha256(out_dir / "queries.jsonl"),
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=1, ensure_ascii=False) + "\n")
    return manifest


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="validate only")
    args = ap.parse_args(argv)
    if args.check:
        errors = validate(load())
        print("\n".join(errors) if errors else "memory.toml: ok")
        return 1 if errors else 0
    m = build()
    print(json.dumps({k: m[k] for k in ("status", "memories", "queries", "splits")}, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
