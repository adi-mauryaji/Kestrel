"""experiments/data/personas_v0.py -- the persona simulator: build Kestrel-Personas-v0 (ADR-028, E4).

    python experiments/data/personas_v0.py --check    # validate datasets/kestrel_personas_v0/personas.toml
    python experiments/data/personas_v0.py            # write datasets/kestrel_personas_v0/build/

A persona teaches 5 aliases in scripted events (teaching phrasings rotate per persona), then makes
held-out requests: every request pattern of each alias's kind in the persona's language (3 exact,
1 variant a literal matcher misses) and the general requests of its language. Expectations are
hand-written templates in personas.toml, filled here from `today` and the fixture notes (dates; the
N newest notes with a tag, newest first by file name, as the notes tool orders them); nothing is
computed by the planner. Personas are split whole into dev / test, per language (seeded hash order).

Out (build/): items.jsonl, events.jsonl, fixture.json (the Kestrel-Tasks-v0 format: today, notes,
no tasks, no memories), manifest.json. The Rust check apps/cli/tests/personas_v0.rs certifies the
expectations (every canonical request reaches its expectation with the template planner) and the
events (each is detected as the alias it teaches).
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
DATA = ROOT / "datasets" / "kestrel_personas_v0"
LANGS = ("en", "hi-en")
KINDS = ("doc", "group", "tag", "count", "place", "date")
DUES = ("tomorrow", "day_after", "saturday", "monday", "friday")
WEEKDAYS = {"monday": 0, "saturday": 5, "friday": 4}
NEEDS = {"doc": ("path",), "tag": ("tag",), "count": ("n",), "date": ("due",), "group": (), "place": ()}
DATED = re.compile(r"(\d{4}-\d{2}-\d{2})-")


def load(path: Path = DATA / "personas.toml") -> dict:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    for row in data.get("persona", []) + data.get("pattern", []) + data.get("general", []):
        row.setdefault("author", data.get("default_author", "claude-draft"))
        row.setdefault("reviewed", data.get("default_reviewed", False))
    return {"today": data["today"], "notes": DATA / data["notes"], "teach": data["teach"],
            "patterns": data["pattern"], "general": data["general"], "personas": data["persona"], "path": path}


def notes_index(data) -> list[tuple[str, list[str]]]:
    """(name relative to the notes folder, tags) of every dated note."""
    out = []
    for p in sorted(data["notes"].rglob("*")):
        if p.is_file() and DATED.match(p.name):
            first = p.read_text(encoding="utf-8").split("\n", 1)[0]
            tags = [t.strip().lower() for t in first[5:].split(",")] if first.startswith("tags:") else []
            out.append((p.relative_to(data["notes"]).as_posix(), tags))
    return out


def recent(data, tag: str | None, n: int) -> list[str]:
    """The n newest dated notes (with `tag` if given): file name descending, as the notes tool does."""
    hits = [(name.rsplit("/", 1)[-1], name) for name, tags in notes_index(data) if tag is None or tag in tags]
    return [name for _, name in sorted(hits, reverse=True)[:n]]


def resolve_due(spec: str, today: str) -> str:
    d = dt.date.fromisoformat(today)
    if spec == "tomorrow":
        return (d + dt.timedelta(days=1)).isoformat()
    if spec == "day_after":
        return (d + dt.timedelta(days=2)).isoformat()
    ahead = (WEEKDAYS[spec] - d.weekday()) % 7 or 7  # strictly after today
    return (d + dt.timedelta(days=ahead)).isoformat()


def _words(text: str) -> list[str]:
    return [w.replace("'", "") for w in re.findall(r"[\w']+", text.lower())]


def _contains(text: str, phrase: str) -> bool:
    t, p = _words(text), _words(phrase)
    return any(t[i:i + len(p)] == p for i in range(len(t) - len(p) + 1))


def validate(data: dict) -> list[str]:
    errors = []
    tags = {t for _, ts in notes_index(data) for t in ts}
    docs = {p.relative_to(data["notes"]).as_posix() for p in data["notes"].rglob("*") if p.is_file()}
    for lang in LANGS:
        if sum(t["lang"] == lang for t in data["teach"]) != 3:
            errors.append(f"teach: {lang} needs 3 phrasings")
        for kind in KINDS:
            mine = [p for p in data["patterns"] if p["kind"] == kind and p["lang"] == lang]
            if Counter(bool(p.get("variant")) for p in mine) != {False: 3, True: 1}:
                errors.append(f"patterns {kind}/{lang}: need 3 exact and 1 variant")
    ids = Counter(p["id"] for p in data["patterns"])
    errors += [f"pattern {i}: duplicate id" for i, n in ids.items() if n > 1]
    for p in data["patterns"]:
        if p.get("variant") and "{v}" not in p["text"]:
            errors.append(f"pattern {p['id']}: a variant pattern uses {{v}}")
        if bool(p.get("variant")) != ("canonical" in p):
            errors.append(f"pattern {p['id']}: a variant pattern, and only one, has a canonical form")
    seen = Counter(p["id"] for p in data["personas"])
    errors += [f"persona {i}: duplicate id" for i, n in seen.items() if n > 1]
    for per in data["personas"]:
        pid, aliases = per["id"], per["aliases"]
        if per["lang"] not in LANGS:
            errors.append(f"persona {pid}: lang {per['lang']!r}")
        if len(aliases) != 5 or len({a["kind"] for a in aliases}) != 5:
            errors.append(f"persona {pid}: needs 5 aliases of 5 different kinds")
        for a in aliases:
            where = f"persona {pid} alias {a.get('phrase')!r}"
            if a.get("kind") not in KINDS:
                errors.append(f"{where}: kind {a.get('kind')!r}")
                continue
            for k in ("phrase", "target", "variant") + NEEDS[a["kind"]]:
                if k not in a or a[k] in ("", None):
                    errors.append(f"{where}: missing {k}")
            if a["kind"] == "doc" and a.get("path") not in docs:
                errors.append(f"{where}: path {a.get('path')!r} is not a fixture document")
            if a["kind"] == "tag" and a.get("tag") not in tags:
                errors.append(f"{where}: tag {a.get('tag')!r} is not a fixture tag")
            if a["kind"] == "date" and a.get("due") not in DUES:
                errors.append(f"{where}: due {a.get('due')!r}")
            if a["kind"] in ("doc", "tag", "count") and _words(a["phrase"])[:1] in (["the"], ["my"]):
                errors.append(f"{where}: starts with the/my, which the patterns add")
            if _words(a.get("variant", "")) == _words(a["phrase"]):
                errors.append(f"{where}: the variant equals the phrase")
        for g in data["general"]:
            if g["lang"] == per["lang"]:
                for a in aliases:
                    if _contains(g["text"], a["phrase"]):
                        errors.append(f"persona {pid}: general request {g['text']!r} contains alias {a['phrase']!r}")
    return errors


def assign_splits(personas, seed: int = 0) -> dict[str, str]:
    """persona -> dev/test: per language, seeded hash order, the first half dev."""
    splits = {}
    for lang in LANGS:
        order = seeded_order(sorted(p["id"] for p in personas if p["lang"] == lang), f"{seed}:{lang}")
        half = (len(order) + 1) // 2
        splits |= {pid: ("dev" if i < half else "test") for i, pid in enumerate(order)}
    return splits


def _expect(spec: dict, data: dict, alias: dict | None) -> dict:
    fill = (lambda s: s.replace("{t}", alias["target"])) if alias else (lambda s: s)  # noqa: E731
    read = []
    for r in spec.get("read", []):
        if r == "doc":
            read.append(alias["path"])
        elif r == "recent":
            read += recent(data, None, alias["n"])
        elif r.startswith("recent:") and alias is not None:
            read += recent(data, alias["tag"], int(r.split(":")[1]))
        elif r.startswith("recent:"):
            _, tag, n = r.split(":")
            read += recent(data, tag, int(n))
        else:
            read.append(r)
    tasks = []
    for t in spec.get("tasks_added", []):
        task = {"title": fill(t["title"])}
        if "due" in t:
            task["due"] = resolve_due(alias["due"] if t["due"] == "alias" else t["due"], data["today"])
        tasks.append(task)
    out = {"outcome": "done"}
    if read:
        out["read"] = read
    if tasks:
        out["tasks_added"] = tasks
    return out


def items(data: dict, splits: dict) -> list[dict]:
    rows = []
    for per in sorted(data["personas"], key=lambda p: p["id"]):
        pid, lang = per["id"], per["lang"]
        for k, a in enumerate(per["aliases"], start=1):
            for p in (p for p in data["patterns"] if p["kind"] == a["kind"] and p["lang"] == lang):
                fill = lambda s: s.replace("{a}", a["phrase"]).replace("{v}", a["variant"]).replace("{t}", a["target"])  # noqa: E731
                rows.append({"id": f"{pid}/{p['id']}", "persona": pid, "split": splits[pid], "lang": lang,
                             "type": "personalized", "kind": a["kind"], "alias": k, "variant": bool(p.get("variant")),
                             "intent": p["intent"], "text": fill(p["text"]),
                             "canonical": fill(p.get("canonical", p["text"].replace("{a}", "{t}"))),
                             "expect": _expect(p["expect"], data, a)})
        for i, g in enumerate(g for g in data["general"] if g["lang"] == lang):
            rows.append({"id": f"{pid}/general-{i + 1:02d}", "persona": pid, "split": splits[pid], "lang": lang,
                         "type": "general", "kind": None, "alias": None, "variant": False, "intent": g["intent"],
                         "text": g["text"], "canonical": g["text"], "expect": _expect(g["expect"], data, None)})
    return rows


def events(data: dict) -> list[dict]:
    out = []
    for idx, per in enumerate(sorted(data["personas"], key=lambda p: p["id"])):
        teach = [t["text"] for t in data["teach"] if t["lang"] == per["lang"]]
        for k, a in enumerate(per["aliases"], start=1):
            text = teach[(idx + k - 1) % len(teach)].replace("{a}", a["phrase"]).replace("{t}", a["target"])
            out.append({"persona": per["id"], "k": k, "lang": per["lang"], "kind": a["kind"], "text": text,
                        "phrase": a["phrase"], "target": a["target"]})
    return out


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build(out_dir: Path = DATA / "build", seed: int = 0) -> dict:
    data = load()
    errors = validate(data)
    if errors:
        raise SystemExit("invalid personas.toml:\n  " + "\n  ".join(errors))
    splits = assign_splits(data["personas"], seed)
    rows, evs = items(data, splits), events(data)
    out_dir.mkdir(parents=True, exist_ok=True)
    write = lambda name, rs: (out_dir / name).write_text(  # noqa: E731
        "".join(json.dumps(r, ensure_ascii=False, sort_keys=True) + "\n" for r in rs), encoding="utf-8")
    write("items.jsonl", rows)
    write("events.jsonl", evs)
    fixture = {"today": data["today"], "notes": data["notes"].relative_to(ROOT).as_posix(), "tasks": [],
               "memories": []}
    (out_dir / "fixture.json").write_text(json.dumps(fixture, indent=1, sort_keys=True) + "\n")
    reviewed = all(r["reviewed"] for r in data["personas"] + data["patterns"] + data["general"])
    manifest = {
        "dataset": "kestrel-personas-v0", "status": "reviewed" if reviewed else "draft", "seed": seed,
        "today": data["today"], "personas": {pid: splits[pid] for pid in sorted(splits)},
        "items": len(rows), "events": len(evs),
        "per_split": {s: dict(sorted(Counter(r["type"] for r in rows if r["split"] == s).items()))
                      for s in ("dev", "test")},
        "sources": {"personas.toml": _sha256(DATA / "personas.toml")},
        "items_sha256": _sha256(out_dir / "items.jsonl"), "events_sha256": _sha256(out_dir / "events.jsonl"),
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=1) + "\n")
    return manifest


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args(argv)
    if args.check:
        errors = validate(load())
        print("\n".join(errors) if errors else "personas.toml: ok")
        return 1 if errors else 0
    print(json.dumps({k: v for k, v in build().items() if k in ("status", "personas", "items", "events", "per_split")},
                     indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
