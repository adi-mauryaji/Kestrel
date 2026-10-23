"""experiments/human/review_data.py -- the review units of the draft datasets, for the Dataset Review page.

The human reviews Kestrel-Tasks-v0, Kestrel-Personas-v0, Kestrel-Memory-v0 and the learning word lists
(kestrel-learning correction.rs, alias.rs) before they are frozen. A unit is what one decision covers:
a task family (both languages, its gold plan, its end state), a malformed plan, a teaching phrasing, a
persona, a request pattern with every expansion, a general request, a memory, a memory query, one word
list. Each unit has a store-safe id and a hash of what it shows, so a decision made on one version of
a unit does not carry over to a changed one (ingest_review.py reports it stale).

build_review.py renders the units into the page; ingest_review.py checks decisions against them.
"""
from __future__ import annotations

import hashlib
import json
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "datasets"
LEARNING = ROOT / "crates" / "kestrel-learning" / "src"
TITLES = {"tasks": "Kestrel-Tasks-v0", "personas": "Kestrel-Personas-v0", "memory": "Kestrel-Memory-v0",
          "words": "Word lists"}


def _rows(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def unit(ds: str, section: str, key: str, title: str, lines: list[dict], lang: str | None = None,
         tags: tuple = ()) -> dict:
    """One reviewable unit. `lines`: {"k": label, "v": text} (+ "mono": True for code)."""
    shown = {"title": title, "lines": lines, "lang": lang, "tags": list(tags)}
    digest = hashlib.sha256(json.dumps(shown, sort_keys=True, ensure_ascii=False).encode()).hexdigest()[:16]
    return {"id": f"{ds}:{section}:{key}", "ds": ds, "section": section, "key": key, **shown, "hash": digest}


def _value(v) -> str:
    if isinstance(v, str):
        return f"“{v}”" if re.search(r"[\s,]", v) else v
    return json.dumps(v, ensure_ascii=False)


def plan_lines(plan: dict | None) -> str:
    """One line per node: `id = op(args; ← deps)`."""
    if plan is None:
        return "no plan: a slot is missing, so Kestrel must ask"
    out = []
    for n in plan["nodes"]:
        inner = [", ".join(f"{k}={_value(v)}" for k, v in (n.get("args") or {}).items())]
        if n.get("deps"):
            inner.append("← " + ", ".join(n["deps"]))
        out.append(f"{n['id']} = {n['op']}({'; '.join(p for p in inner if p)})")
    return "\n".join(out)


def expect_text(e: dict) -> str:
    """An expected end state in words."""
    parts = [e.get("outcome", "?")]
    if e.get("read"):
        parts.append("reads " + ", ".join(e["read"]))
    for t in e.get("tasks_added") or []:
        extra = "".join(f", {k} {v}" for k, v in t.items() if k != "title")
        parts.append(f"adds task “{t['title']}”{extra}")
    if e.get("tasks_deleted"):
        parts.append("deletes task " + ", ".join(map(str, e["tasks_deleted"])))
    if e.get("prefs_set"):
        parts.append("sets " + ", ".join(f"{k} = {v}" for k, v in e["prefs_set"].items()))
    if e.get("answer_contains"):
        parts.append(f"answer contains “{e['answer_contains']}”")
    return " · ".join(parts)


def tasks_units() -> list[dict]:
    d = DATA / "kestrel_tasks_v0" / "build"
    items = _rows(d / "items.jsonl")
    out = []
    for fam in sorted({r["family"] for r in items}):
        rows = [r for r in items if r["family"] == fam]
        lines = [{"k": r["lang"], "v": r["text"]} for r in sorted(rows, key=lambda r: (r["lang"], r["id"]))]
        lines.append({"k": "Plan", "v": plan_lines(rows[0]["plan"]), "mono": True})
        lines += [{"k": f"Plan ({r['id']})", "v": plan_lines(r["plan"]), "mono": True}
                  for r in rows[1:] if r["plan"] != rows[0]["plan"]]
        lines.append({"k": "Expected", "v": expect_text(rows[0]["expect"])})
        lines += [{"k": f"Expected ({r['id']})", "v": expect_text(r["expect"])}
                  for r in rows[1:] if r["expect"] != rows[0]["expect"]]
        out.append(unit("tasks", "family", fam, f"{fam} · {rows[0]['category']}", lines,
                        tags=tuple(sorted({r["split"] for r in rows}))))
    for m in _rows(d / "malformed.jsonl"):
        out.append(unit("tasks", "malformed", m["id"], f"{m['id']} · must be rejected",
                        [{"k": "Plan", "v": m["plan"], "mono": True}, {"k": "Why", "v": m["why"]},
                         {"k": "Validator names", "v": m["error"]}]))
    return out


def _template_re(text: str) -> re.Pattern:
    return re.compile("^" + re.escape(text).replace(r"\{a\}", "(.+)").replace(r"\{t\}", "(.+)") + "$")


def personas_units() -> list[dict]:
    base = DATA / "kestrel_personas_v0"
    spec = tomllib.loads((base / "personas.toml").read_text(encoding="utf-8"))
    items, events = _rows(base / "build" / "items.jsonl"), _rows(base / "build" / "events.jsonl")
    out = []
    n = {"en": 0, "hi-en": 0}
    for t in spec["teach"]:
        n[t["lang"]] += 1
        rx = _template_re(t["text"])
        examples = [e["text"] for e in events if e["lang"] == t["lang"] and rx.match(e["text"])][:2]
        lines = [{"k": "Template", "v": t["text"].replace("{a}", "‹what you say›").replace("{t}", "‹what you mean›")}]
        lines += [{"k": "Example", "v": x} for x in examples]
        out.append(unit("personas", "teach", f"{t['lang']}-{n[t['lang']]}", f"Teaching phrasing {n[t['lang']]}",
                        lines, lang=t["lang"]))
    for p in spec["persona"]:
        lines = []
        for a in p["aliases"]:
            v = f"{a['phrase']} → {a['target']}" + (f" (variant “{a['variant']}”)" if a.get("variant") else "")
            lines.append({"k": a["kind"], "v": v})
        lines += [{"k": f"Teaches {e['k']}", "v": e["text"]}
                  for e in sorted((e for e in events if e["persona"] == p["id"]), key=lambda e: e["k"])]
        out.append(unit("personas", "persona", p["id"], f"Persona {p['id']}", lines, lang=p["lang"]))
    for pat in spec["pattern"]:
        mine = sorted((r for r in items if r["id"].split("/", 1)[1] == pat["id"]), key=lambda r: r["id"])
        lines = [{"k": "Template", "v": pat["text"]}, {"k": "Intent", "v": pat["intent"]}]
        lines += [{"k": r["persona"], "v": f"{r['text']} → {expect_text(r['expect'])}"} for r in mine]
        out.append(unit("personas", "pattern", pat["id"], f"{pat['id']} · {pat['kind']}" +
                        (" · variant" if mine and mine[0]["variant"] else ""), lines, lang=pat["lang"]))
    n = {"en": 0, "hi-en": 0}
    for g in spec["general"]:
        n[g["lang"]] += 1
        filled = next((r for r in items
                       if r["type"] == "general" and r["lang"] == g["lang"] and r["text"] == g["text"]), None)
        lines = [{"k": "Request", "v": g["text"]}, {"k": "Intent", "v": g["intent"]},
                 {"k": "Expected", "v": expect_text(filled["expect"] if filled else g["expect"])}]
        out.append(unit("personas", "general", f"{g['lang']}-{n[g['lang']]:02d}",
                        f"General request {g['lang']} {n[g['lang']]}", lines, lang=g["lang"]))
    return out


def memory_units() -> list[dict]:
    d = DATA / "kestrel_memory_v0" / "build"
    now = json.loads((d / "manifest.json").read_text())["now_unix"]
    store = _rows(d / "store.jsonl")
    by_id = {m["id"]: m for m in store}
    out = []
    for m in store:
        age = round((now - m["updated_at"]) / 86400)
        if m["expires_at"] is None:
            expiry = "never"
        else:
            days = round((m["expires_at"] - now) / 86400)
            expiry = f"in {days} days" if days >= 0 else f"expired {-days} days ago"
        out.append(unit("memory", "memory", m["id"], m["id"], [
            {"k": "Memory", "v": f"{m['key']} = {m['value']}"},
            {"k": "Kind", "v": f"{m['kind']} · {m['source']} · importance {m['importance']} · group {m['group']}"},
            {"k": "State", "v": ("approved" if m["approved"] else "pending guess, never retrievable")
             + f" · updated {age} days ago · expires {expiry}"}]))
    for q in _rows(d / "queries.jsonl"):
        named = lambda ids: "\n".join(f"{i}: {by_id[i]['key']} = {by_id[i]['value']}" for i in ids)  # noqa: E731
        lines = [{"k": "Query", "v": q["text"]}, {"k": "Type", "v": q["type"]},
                 {"k": "Should return", "v": named(q["expect"]) if q["expect"] else "nothing"}]
        if q["trap"]:
            lines.append({"k": "Must never return", "v": named(q["trap"])})
        out.append(unit("memory", "query", q["id"], f"{q['id']} · {q['type']}", lines, lang=q["lang"]))
    return out


_STR = re.compile(r'"((?:[^"\\]|\\.)*)"')
_CONST = re.compile(r"((?:^[ \t]*///[^\n]*\n)*)^const ([A-Z_]+):[^=\n]*= &\[(.*?)\];[ \t]*$", re.M | re.S)
_STYLE = re.compile(r'\(\s*"([^"]*)"\s*,\s*"([^"]*)"\s*,\s*&\[([^\]]*)\]\s*,?\s*\)')
_INNER = re.compile(r"&\[([^\]]*)\]")


def rust_lists(src: str) -> dict[str, dict]:
    """The `const NAME: ... = &[...];` string lists of a Rust source: {"doc", "items"} per name. Nested
    lists become their words joined by spaces; (key, value, &[words]) tuples become "key = value: words"."""
    out = {}
    for doc, name, body in _CONST.findall(src):
        body = "\n".join(re.sub(r'//[^"\n]*$', "", line) for line in body.splitlines())
        if _STYLE.search(body):
            items = [f"{a} = {b}: " + ", ".join(_STR.findall(c)) for a, b, c in _STYLE.findall(body)]
        elif _INNER.search(body):
            items = [" ".join(_STR.findall(inner)) for inner in _INNER.findall(body)]
        else:
            items = _STR.findall(body)
        text = " ".join(line.strip().removeprefix("///").strip() for line in doc.splitlines() if line.strip())
        out[name] = {"doc": text, "items": items}
    return out


def doc_comment_quotes(src: str, marker: str) -> list[str]:
    """The quoted strings in the /// block just above `marker`."""
    lines = src[: src.index(marker)].splitlines()
    block = []
    while lines and lines[-1].strip().startswith("///"):
        block.insert(0, lines.pop())
    return _STR.findall("\n".join(block))


def _source(name: str) -> str:
    src = (LEARNING / name).read_text(encoding="utf-8")
    return src.split("#[cfg(test)]")[0]


def words_units() -> list[dict]:
    out = []
    for file, section in (("correction.rs", "correction"), ("alias.rs", "alias")):
        src = _source(file)
        for name, lst in rust_lists(src).items():
            lines = ([{"k": "What it is", "v": lst["doc"]}] if lst["doc"] else [])
            lines.append({"k": "Words", "v": "\n".join(lst["items"]), "mono": True})
            out.append(unit("words", section, name, f"{file} · {name}", lines))
    forms = doc_comment_quotes(_source("alias.rs"), "pub fn detect")
    out.append(unit("words", "alias", "phrasings", "alias.rs · teaching forms it recognizes",
                    [{"k": "Forms", "v": "\n".join(forms), "mono": True},
                     {"k": "Also", "v": "any of these after a correction opener (OPENERS)"}]))
    return out


def all_units() -> list[dict]:
    return tasks_units() + personas_units() + memory_units() + words_units()


def fixtures() -> dict[str, list[dict]]:
    """The fixture notes the expected end states refer to, as reference (not reviewed units)."""
    out = {}
    for ds, d in (("tasks", "kestrel_tasks_v0"), ("personas", "kestrel_personas_v0")):
        notes = DATA / d / "fixtures" / "notes"
        out[ds] = [{"path": str(p.relative_to(notes)), "text": p.read_text(encoding="utf-8")}
                   for p in sorted(notes.rglob("*")) if p.is_file()]
    return out
