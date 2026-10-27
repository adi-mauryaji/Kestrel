"""experiments/v3/notes_qa.py -- questions answered from the notes (ADR-041), on Kestrel-NotesQA-v0 dev.

The runtime path, per question: the notes closest in meaning (src/meaning.rs's scoring: e5 "query:" /
"passage:", title + paragraph chunks, a note's best chunk; a question with a common Hindi word is first
put into English by the service's writer, as the service does), the top N go with the question to the
worker's {"answer"} (src/notes_qa.rs: Gemma answers from them only or says NOT IN NOTES; the worker's
grounding check says why an answer is not grounded). Fixed before looking: N = 2, and the gate theta =
0.83 (ADR-040's): when even the closest note is below it, the answer is "not in notes" without Gemma.
Gemma answers every question once; the gate is applied offline, so other thetas are reported for
information only. The test split stays closed until the human has reviewed NotesQA-v0.

What the user sees: an answer, "not in your notes", or nothing shown (an ungrounded answer is withheld).
A question the notes answer is right when the answer holds every fact (any of its ways), wrong when an
answer lacks one, abstained otherwise; a question they do not answer is right unless it gets an answer.

    cargo build --release -p kestrel-cli --features llama
    python experiments/v3/notes_qa.py        # ~3-4 min of full CPU (Gemma on 24 questions)
"""
from __future__ import annotations

import dataclasses
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
for sub in ("data", "tier3", "v3"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from embed import embed, load_encoder  # noqa: E402
from notes_meaning import chunks, needs_translation  # noqa: E402
from writer_lang import BIN, CACHE, CONFIG  # noqa: E402

DATA = ROOT / "datasets" / "kestrel_notesqa_v0"
NOTES = ROOT / "datasets" / "kestrel_notesq_v0" / "notes"
ENCODER = "multilingual-e5-small-q8_0"
N = 2
THETA = 0.83
GRID = [round(0.78 + 0.01 * i, 2) for i in range(10)]
LOG = ROOT / "experiments" / "logs" / "v3" / "notes" / "qa_dev.json"


def has_facts(text: str, facts: list[list[str]]) -> bool:
    t = text.lower()
    return all(any(way.lower() in t for way in fact) for fact in facts)


def outcome(text: str, ungrounded: str | None, gated: bool) -> str:
    if gated or text.strip().lower().rstrip(".!").startswith("not in notes"):
        return "not in notes"
    return "withheld" if ungrounded else "answered"


def grade(row: dict, seen: str, text: str) -> str:
    if not row["notes"]:
        return "wrong" if seen == "answered" else "right"
    if seen != "answered":
        return "abstained"
    return "right" if has_facts(text, row["facts"]) else "wrong"


def note_text(raw: str) -> str:
    """As Notes gives it to the runtime: without a first line "tags: ..."."""
    first, _, rest = raw.partition("\n")
    return rest if first.startswith("tags:") else raw


class Worker:
    """The service's Gemma worker (kestrel tier4-worker): one JSON request, one reply (parts skipped)."""

    def __init__(self):
        CACHE.mkdir(parents=True, exist_ok=True)
        self.p = subprocess.Popen([str(BIN), "tier4-worker", CONFIG], cwd=ROOT, text=True, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  env={**os.environ, "XDG_CACHE_HOME": str(CACHE)})
        assert json.loads(self.p.stdout.readline()).get("ready"), "the worker did not start"

    def ask(self, msg: dict) -> dict:
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        while "part" in (reply := json.loads(self.p.stdout.readline())):
            pass
        return reply

    def close(self):
        self.p.stdin.close()
        self.p.wait()


def _rates(rows, theta):
    out = {}
    for name, part in (("answerable", [r for r in rows if r["notes"]]), ("unanswerable", [r for r in rows if not r["notes"]])):
        grades = [grade(r, outcome(r["answer"], r["ungrounded"], r["top_score"] < theta), r["answer"]) for r in part]
        out[name] = {g: grades.count(g) for g in ("right", "wrong", "abstained") if g in grades or g != "abstained"}
    return out


def main():
    notes = {p.relative_to(NOTES).as_posix(): note_text(p.read_text(encoding="utf-8")) for p in sorted(NOTES.rglob("*.md"))}
    rows = [json.loads(l) for l in (DATA / "splits" / "dev.jsonl").read_text(encoding="utf-8").splitlines()]
    enc = load_encoder(ENCODER)
    passage = dataclasses.replace(enc, prefix="passage: ")
    owner, texts = [], []
    for name, text in notes.items():
        for c in chunks(text):
            owner.append(name)
            texts.append(c)
    p = embed(passage, texts)
    w = Worker()
    try:
        for r in rows:
            r["asked"] = r["text"]
            if needs_translation(r["text"]):
                said = w.ask({"id": 0, "write": {"kind": "translate", "text": f"translate to english: {r['text']}"}})
                r["asked"] = (said.get("text") or r["text"]).strip().rstrip(".")
        q = embed(enc, [r["asked"] for r in rows])
        for r, qv in zip(rows, q):
            sims = p @ qv
            scores = {n: float(max(s for o, s in zip(owner, sims) if o == n)) for n in notes}
            top = sorted(scores, key=lambda n: -scores[n])[:N]
            r["top"] = [[n, round(scores[n], 4)] for n in top]
            r["top_score"] = scores[top[0]]
            r["retrieved"] = bool(set(r["notes"]) & set(top)) if r["notes"] else None
            t0 = time.time()
            reply = w.ask({"id": 0, "answer": {"question": r["text"], "notes": [{"name": n, "text": notes[n]} for n in top]}})
            r["s"] = round(time.time() - t0, 2)
            r["answer"], r["ungrounded"], r["error"] = reply.get("text") or "", reply.get("ungrounded"), reply.get("error")
    finally:
        w.close()
    for r in rows:
        r["seen"] = outcome(r["answer"], r["ungrounded"], r["top_score"] < THETA)
        r["grade"] = grade(r, r["seen"], r["answer"])
    answerable = [r for r in rows if r["notes"]]
    out = {
        "encoder": ENCODER, "n_notes_given": N, "theta": THETA, "dev_questions": len(rows),
        "retrieved_at_n": sum(r["retrieved"] for r in answerable) / len(answerable),
        "at_theta": _rates(rows, THETA),
        "by_lang_at_theta": {lang: _rates([r for r in rows if r["lang"] == lang], THETA) for lang in ("en", "hi-en")},
        "grid": {str(t): _rates(rows, t) for t in GRID},
        "answer_s": {"mean": round(sum(r["s"] for r in rows) / len(rows), 2), "max": max(r["s"] for r in rows)},
        "errors": [r["id"] for r in rows if r["error"]],
        "questions": [{k: r[k] for k in ("id", "text", "asked", "notes", "top", "answer", "ungrounded", "seen", "grade", "s")}
                      for r in rows],
    }
    LOG.parent.mkdir(parents=True, exist_ok=True)
    LOG.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    for r in rows:
        print(f"{r['grade']:9s} {r['seen']:12s} {r['text'][:42]!r} -> {r['answer'][:70]!r}")
    print(json.dumps({k: v for k, v in out.items() if k not in ("grid", "questions")}, ensure_ascii=False))
    print(f"wrote {LOG.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
