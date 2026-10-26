"""experiments/v3/notes_meaning.py -- notes found by meaning (ADR-039), on Kestrel-NotesQ-v0 dev.

The keyword search stays as it is (Notes::search: a note matches when it contains every query term;
here a Python port of that rule); when it finds fewer than k notes, notes whose meaning is close
enough (cosine at or above theta) are added after its hits. Meaning: multilingual-e5-small Q8_0
through the Tier-3 runtime embedder (experiments/tier3/embed.py), "query: " + the query, "passage: "
+ each chunk of a note (its title and one paragraph); a note's score is its best chunk's.

theta is chosen on dev by a rule fixed before looking: on a grid 0.70..0.95 (step 0.01), the value
with the highest (share of queries whose note is among the k results) - (share of no-note queries
that get any note by meaning); ties go to the higher theta (fewer additions). The test split stays
closed until the human has reviewed NotesQ-v0.

    python experiments/v3/notes_meaning.py        # seconds: 18 notes + 22 queries through e5
    python experiments/v3/notes_meaning.py --translate   # + Gemma: a query with any Hindi word is
                                                         # translated to English first (~30 s)
"""
from __future__ import annotations

import argparse
import dataclasses
import json
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
for sub in ("data", "tier3", "v3"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from embed import embed, load_encoder  # noqa: E402
from writer_lang import HINDI_WORDS, write_all  # noqa: E402

DATA = ROOT / "datasets" / "kestrel_notesq_v0"
ENCODER = "multilingual-e5-small-q8_0"
K = 3
GRID = [round(0.70 + 0.01 * i, 2) for i in range(26)]
LOG = ROOT / "experiments" / "logs" / "v3" / "notes" / "meaning_dev.json"


def needs_translation(query: str) -> bool:
    """e5 does not know Romanized Hindi: a query with any common Hindi word goes through English."""
    return any(w in HINDI_WORDS for w in query.lower().split())


def chunks(text: str) -> list[str]:
    """The note's title (first line that is not "tags:") with each paragraph after it."""
    lines = [l for l in text.splitlines() if not l.lower().startswith("tags:")]
    title_at = next((i for i, l in enumerate(lines) if l.strip()), None)
    if title_at is None:
        return []
    title = lines[title_at].strip()
    paras = [p.strip() for p in "\n".join(lines[title_at + 1:]).split("\n\n") if p.strip()]
    return [f"{title}. {' '.join(p.split())}" for p in paras] or [title]


def lexical(query: str, notes: dict[str, str]) -> list[str]:
    """Notes::search's rule: every term occurs (case-insensitive); most occurrences first."""
    terms = query.lower().split()
    hits = []
    for name, text in notes.items():
        counts = [text.lower().count(t) for t in terms]
        if all(counts):
            hits.append((-sum(counts), name))
    return [name for _, name in sorted(hits)]


def policy(keyword: list[str], scores: dict[str, float], theta: float, k: int) -> tuple[list[str], set[str]]:
    """The k results: the keyword hits, then notes by meaning at or above theta; and which came by meaning."""
    out = keyword[:k]
    added = [n for n, s in sorted(scores.items(), key=lambda kv: -kv[1]) if s >= theta and n not in out]
    added = added[: k - len(out)]
    return out + added, set(added)


def choose_theta(table: dict[float, tuple[float, float]]) -> float:
    return max(table, key=lambda t: (table[t][0] - table[t][1], t))


def _rates(rows, theta):
    found = [r for r in rows if r["notes"]]
    none = [r for r in rows if not r["notes"]]
    res = [policy(r["keyword"], r["scores"], theta, K) for r in rows]
    hit = sum(bool(set(r["notes"]) & set(out)) for r, (out, _) in zip(rows, res) if r["notes"])
    noise = sum(bool(added) for r, (_, added) in zip(rows, res) if not r["notes"])
    return hit / len(found), noise / len(none) if none else 0.0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--translate", action="store_true", help="translate queries with Hindi words to English (Gemma)")
    args = ap.parse_args(argv)
    notes = {p.relative_to(DATA / "notes").as_posix(): p.read_text(encoding="utf-8")
             for p in sorted((DATA / "notes").rglob("*.md"))}
    rows = [json.loads(l) for l in (DATA / "splits" / "dev.jsonl").read_text(encoding="utf-8").splitlines()]
    enc = load_encoder(ENCODER)
    passage = dataclasses.replace(enc, prefix="passage: ")
    owner, texts = [], []
    for name, text in notes.items():
        for c in chunks(text):
            owner.append(name)
            texts.append(c)
    p = embed(passage, texts)
    for r in rows:
        r["asked"] = r["text"]
    if args.translate:
        todo = [r for r in rows if needs_translation(r["text"])]
        said = write_all([{"kind": "translate", "text": f"translate to english: {r['text']}"} for r in todo])
        for r, a in zip(todo, said):
            r["asked"] = (a["answer"] or r["text"]).strip().rstrip(".")
    q = embed(enc, [r["asked"] for r in rows])
    for r, qv in zip(rows, q):
        sims = p @ qv
        r["scores"] = {n: float(max(s for o, s in zip(owner, sims) if o == n)) for n in notes}
        r["keyword"] = lexical(r["text"], notes)  # the keyword search sees the query as typed
        best = max(r["scores"], key=r["scores"].get)
        r["best"] = [best, round(r["scores"][best], 4)]
        r["note_rank"] = (sorted(r["scores"], key=lambda n: -r["scores"][n]).index(r["notes"][0]) + 1
                          if r["notes"] else None)
    table = {t: _rates(rows, t) for t in GRID}
    theta = choose_theta(table)
    found, none = [r for r in rows if r["notes"]], [r for r in rows if not r["notes"]]
    out = {
        "encoder": ENCODER, "translate": args.translate, "k": K, "chunks": len(texts), "dev_queries": len(rows),
        "keyword_only_found": sum(bool(set(r["notes"]) & set(r["keyword"][:K])) for r in found) / len(found),
        "meaning_rank1": sum(r["note_rank"] == 1 for r in found) / len(found),
        "meaning_rank_le_k": sum(r["note_rank"] <= K for r in found) / len(found),
        "theta": theta, "found_and_noise_at_theta": table[theta],
        "by_lang_at_theta": {lang: _rates([r for r in rows if r["lang"] == lang], theta) for lang in ("en", "hi-en")},
        "grid": {str(t): v for t, v in table.items()},
        "none_best_scores": [r["best"][1] for r in none],
        "found_note_scores": [round(r["scores"][r["notes"][0]], 4) for r in found],
        "queries": [{k: r[k] for k in ("id", "text", "asked", "notes", "keyword", "best", "note_rank")} for r in rows],
    }
    log = LOG.with_name("meaning_dev_translated.json") if args.translate else LOG
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in out.items() if k not in ("grid", "queries")}, indent=1))
    print(f"wrote {log.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
