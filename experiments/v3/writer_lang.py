"""experiments/v3/writer_lang.py -- does the v3 writer answer in the language and script asked for?

The human writes Hinglish (ADR-005). A rewrite keeps the language of the text it is given (Hinglish
stays Hinglish in Latin letters, English stays English, whatever language the instruction is in); a
translation answers in the language asked for, with "hindi" in Devanagari and "hinglish" / "roman
hindi" in Latin letters (the human, 2026-10-26: "continue" on Devanagari for "hindi").

Items: experiments/v3/writer_lang_items.jsonl (Claude's, not from Kinds-v0), each with the script and,
for Latin answers, the language wanted. They go through the service's own writer: `kestrel
tier4-worker` with `{"write": {kind, text}}` (src/kinds.rs prompts, greedy, src/service.rs), so the
release binary must be built from the prompts being judged. The checks are heuristics: the script is
the one most letters are in; Latin text is Hinglish when enough of its words are common Hindi words.

    cargo build --release -p kestrel-cli --features llama
    python experiments/v3/writer_lang.py --tag baseline       # ~1-2 min of full CPU (Gemma)
    python experiments/v3/writer_lang.py --rescore experiments/logs/v3/writer/lang_baseline.json
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ITEMS = ROOT / "experiments" / "v3" / "writer_lang_items.jsonl"
LOG = ROOT / "experiments" / "logs" / "v3" / "writer"
BIN = ROOT / "target" / "release" / "kestrel"
CONFIG = "experiments/configs/router_intents_v0.json"
CACHE = ROOT / "experiments" / "cache" / "worker"  # the worker's saved prefix; not the user's cache

DEVANAGARI = re.compile(r"[ऀ-ॿ]")
LATIN = re.compile(r"[A-Za-z]")
# Common Hindi words in Latin letters; the v3 writer's language note reads the same list.
HINDI_WORDS = {l.strip() for l in (ROOT / "datasets" / "contracts" / "hinglish_words.txt").read_text(encoding="utf-8")
               .splitlines() if l.strip() and not l.startswith("#")}


def script(text: str) -> str:
    return "devanagari" if len(DEVANAGARI.findall(text)) > len(LATIN.findall(text)) else "latin"


def is_hinglish(text: str) -> bool:
    words = re.findall(r"[a-z]+", text.lower())
    hits = sum(w in HINDI_WORDS for w in words)
    return hits >= 2 and hits / max(len(words), 1) >= 0.15


def passes(text: str, want: dict) -> bool:
    if script(text) != want["script"]:
        return False
    if want.get("lang") == "hinglish":
        return is_hinglish(text)
    if want.get("lang") == "english":
        return not is_hinglish(text)
    return True


def score(rows: list[dict]) -> dict:
    """Each row's pass (from its answer and want) and the counts per group, in place."""
    groups = {}
    for r in rows:
        r["pass"] = r["answer"] is not None and passes(r["answer"], r["want"])
        g = groups.setdefault(r["group"], {"n": 0, "pass": 0})
        g["n"] += 1
        g["pass"] += r["pass"]
    return {"n": len(rows), "pass": sum(r["pass"] for r in rows), "groups": groups}


def rescore(path: Path) -> dict:
    """A logged run scored again with the current checks (no model); the log is rewritten."""
    log = json.loads(Path(path).read_text(encoding="utf-8"))
    log |= score(log["items"])
    Path(path).write_text(json.dumps(log, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    return log


def write_all(items: list[dict]) -> list[dict]:
    """Each item's answer from one worker process, in order, with its time."""
    CACHE.mkdir(parents=True, exist_ok=True)
    p = subprocess.Popen([str(BIN), "tier4-worker", CONFIG], cwd=ROOT, text=True, stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                         env={**os.environ, "XDG_CACHE_HOME": str(CACHE)})
    assert json.loads(p.stdout.readline()).get("ready"), "the worker did not start"
    out = []
    for it in items:
        t0 = time.time()
        p.stdin.write(json.dumps({"id": 0, "write": {"kind": it["kind"], "text": it["text"]}}) + "\n")
        p.stdin.flush()
        while "part" in (reply := json.loads(p.stdout.readline())):
            pass
        out.append({"answer": reply.get("text"), "error": reply.get("error"), "s": round(time.time() - t0, 2)})
    p.stdin.close()
    p.wait()
    return out


def _git(*args) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True).stdout.strip()


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--tag", help="name of this run (log file name)")
    ap.add_argument("--rescore", type=Path, help="score a logged run again instead of running the model")
    args = ap.parse_args(argv)
    if args.rescore:
        out = rescore(args.rescore)
        print(json.dumps({k: out[k] for k in ("tag", "n", "pass", "groups")}, ensure_ascii=False))
        return
    if not args.tag:
        ap.error("--tag is needed for a run")
    items = [json.loads(l) for l in ITEMS.read_text(encoding="utf-8").splitlines() if l.strip()]
    rows = [it | a for it, a in zip(items, write_all(items))]
    out = {"tag": args.tag, "commit": _git("rev-parse", "--short", "HEAD"),
           "prompts_changed_since_commit": bool(_git("status", "--porcelain", "apps/cli/src/kinds.rs")),
           "binary_mtime": time.strftime("%Y-%m-%d %H:%M", time.localtime(BIN.stat().st_mtime))}
    out |= score(rows) | {"items": rows}
    LOG.mkdir(parents=True, exist_ok=True)
    path = LOG / f"lang_{args.tag}.json"
    path.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    for r in rows:
        shown = (r["answer"] or f"error: {r['error']}").replace("\n", " / ")[:80]
        print(f"{'ok ' if r['pass'] else 'BAD'} [{r['group']}] {r['text'][:45]!r} -> {shown!r}")
    print(json.dumps({k: out[k] for k in ("tag", "n", "pass", "groups")}, ensure_ascii=False))
    print(f"wrote {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
