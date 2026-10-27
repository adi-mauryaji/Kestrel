#!/usr/bin/env python3
"""experiments/v3/code_eval.py -- v3's code requests on Kestrel-CodeTasks-v0 (ADR-036), dev only.

Judging (hidden from the model): a `function` or `fix` answer is imported as solution.py and the family's
asserts run against it; a `script` answer is run on each case's stdin and its output compared, ignoring
case and spacing at the ends; an `explain` answer holds every fact (any of its ways). Code runs only in
experiments/v3/sandbox.py.

The loop, fixed before any model was run (2026-10-27): a code request gets CODE_SYSTEM; the model answers
with a ```python block and its own check (```tests asserts for a function, an ```input sample for a
script). The sandbox runs that check (`self_check`: the asserts against solution.py, which also imports
the code, or the script on its sample input); when it fails, the error goes back (`feedback`), at most
RETRIES times. Only the model's own checks drive the loop, as in the app; the hidden checks judge the
first answer (pass@1) and the last (after the loop). An explain request gets EXPLAIN_SYSTEM, one answer.
The harness knows each request's type; telling explain from write in the app is Kinds-v2's job.

    cargo build --release -p kestrel-inference --features llama --example chat_lines
    python experiments/v3/code_eval.py --model qwen3-4b [--limit N]   # -> experiments/logs/v3/code/
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from sandbox import run_python  # noqa: E402
from writer_lang import is_hinglish  # noqa: E402

DATA = ROOT / "datasets" / "kestrel_codetasks_v0" / "splits"
LOG = ROOT / "experiments" / "logs" / "v3" / "code"
CHAT = ROOT / "target" / "release" / "examples" / "chat_lines"
MODELS = {"gemma-4-e2b": "gemma-4-E2B-it-Q4_0.gguf", "qwen3-1.7b": "Qwen3-1.7B-Q4_K_M.gguf",
          "qwen3-4b": "Qwen3-4B-Q4_K_M.gguf"}
RETRIES = 2
MAX_TOKENS = {"code": 640, "explain": 220}
CODE_SYSTEM = (
    "You write small Python 3 programs for the user, using only the standard library. Reply with the code "
    "in one ```python block. After it, add one check: for a function, a ```tests block with three or four "
    "assert lines that call it; for a script that reads input, an ```input block with one sample input. A "
    "script reads its input without printing prompts and prints only what is asked. No other text.")
EXPLAIN_SYSTEM = (
    "You explain Python code to the user: what it does and what it gives, in two to four short sentences. "
    "Answer in the language of the request: English for English, Hinglish (Hindi in Latin letters) for "
    "Hinglish.")

FENCE = re.compile(r"```([A-Za-z]*)[^\n]*\n(.*?)```", re.S)


def blocks(text: str) -> tuple[str, str]:
    """(code, self-check): the first fenced block not named tests / input (or the whole text when there
    is no fence), and the first block named tests or input."""
    found = FENCE.findall(text)
    if not found:
        return text, ""
    code = next((b for name, b in found if name.lower() not in ("tests", "test", "input")), "")
    check = next((b for name, b in found if name.lower() in ("tests", "test", "input")), "")
    return code, check


def _norm(s: str) -> str:
    return " ".join(s.split()).lower()


def passes(row: dict, code: str) -> bool:
    if row["type"] in ("function", "fix"):
        r = run_python("from solution import *\n" + row["tests"], files={"solution.py": code})
        return r.ok
    if row["type"] == "script":
        return all((r := run_python(code, stdin=c["stdin"])).ok and _norm(r.stdout) == _norm(c["stdout"])
                   for c in row["cases"])
    raise ValueError(f"{row['type']} answers are judged by facts_hold")


def facts_hold(row: dict, text: str) -> bool:
    t = text.lower()
    return all(any(way.lower() in t for way in fact) for fact in row["facts"])


def self_check(row: dict, code: str, check: str) -> tuple[bool, str]:
    """The model's own check in the sandbox: (passed, the error's text). A script without a sample input
    is only compiled."""
    if row["type"] == "script" and not check.strip():
        r = run_python("import py_compile\npy_compile.compile('solution.py', doraise=True)",
                       files={"solution.py": code})
    elif row["type"] == "script":
        r = run_python(code, stdin=check)
    else:
        r = run_python("from solution import *\n" + check, files={"solution.py": code})
    if r.ok:
        return True, ""
    return False, ("timed out" if r.timed_out else r.stderr.strip() or f"exit code {r.returncode}")


def feedback(request: str, code: str, error: str) -> str:
    """The next attempt's message: the request, the code, and the end of the error."""
    return (f"{request}\n\nYour code:\n```python\n{code.rstrip()}\n```\n\nRunning your check gave this "
            f"error:\n{error[-1500:]}\n\nFix the code. Reply in the same form as before.")


class Chat:
    """One model in target/release/examples/chat_lines: loaded once, one JSON line per question."""

    def __init__(self, model: str):
        self.p = subprocess.Popen([str(CHAT), str(ROOT / "models" / MODELS[model])], cwd=ROOT, text=True,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        assert json.loads(self.p.stdout.readline()).get("ready"), "chat_lines did not start"

    def ask(self, system: str, user: str, max_tokens: int) -> dict:
        self.p.stdin.write(json.dumps({"system": system, "user": user, "max_tokens": max_tokens}) + "\n")
        self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())

    def close(self):
        self.p.stdin.close()
        self.p.wait()


def run_row(chat: Chat, row: dict) -> dict:
    out = {k: row[k] for k in ("id", "type", "lang")} | {"attempts": []}
    if row["type"] == "explain":
        a = chat.ask(EXPLAIN_SYSTEM, row["text"], MAX_TOKENS["explain"])
        text = a.get("text", "")
        out["attempts"].append({"text": text, "ms": a.get("total_ms"), "tokens": a.get("tokens")})
        out["pass1"] = out["pass_loop"] = facts_hold(row, text)
        out["hinglish"] = is_hinglish(text)
        return out
    user, first = row["text"], None
    for k in range(RETRIES + 1):
        a = chat.ask(CODE_SYSTEM, user, MAX_TOKENS["code"])
        code, check = blocks(a.get("text", ""))
        ok, err = self_check(row, code, check)
        out["attempts"].append({"text": a.get("text", ""), "ms": a.get("total_ms"), "tokens": a.get("tokens"),
                                "has_check": bool(check.strip()), "self_ok": ok, "error": err[-400:]})
        first = code if first is None else first
        if ok:
            break
        user = feedback(row["text"], code, err)
    out["pass1"] = passes(row, first)
    out["pass_loop"] = passes(row, code)
    return out


def summary(results: list[dict]) -> dict:
    def part(rs):
        return {"n": len(rs), "pass1": sum(r["pass1"] for r in rs), "pass_loop": sum(r["pass_loop"] for r in rs),
                "attempts": sum(len(r["attempts"]) for r in rs),
                "seconds": round(sum((a["ms"] or 0) for r in rs for a in r["attempts"]) / 1000, 1)}
    out = {"all": part(results)}
    out |= {t: part([r for r in results if r["type"] == t]) for t in sorted({r["type"] for r in results})}
    out |= {lang: part([r for r in results if r["lang"] == lang]) for lang in ("en", "hi-en")}
    ex = [r for r in results if r["type"] == "explain" and r["lang"] == "hi-en"]
    out["explain_hi_en_in_hinglish"] = f"{sum(r['hinglish'] for r in ex)}/{len(ex)}"
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--model", choices=sorted(MODELS), required=True)
    ap.add_argument("--limit", type=int, help="the first N dev requests only (a smoke run, not logged)")
    a = ap.parse_args(argv)
    rows = [json.loads(l) for l in (DATA / "dev.jsonl").read_text(encoding="utf-8").splitlines()]
    rows = rows[: a.limit] if a.limit else rows
    chat, t0, results = Chat(a.model), time.time(), []
    try:
        for row in rows:
            results.append(run_row(chat, row))
            r = results[-1]
            print(f"{r['id']:28s} pass1={r['pass1']!s:5s} loop={r['pass_loop']!s:5s} attempts={len(r['attempts'])}",
                  flush=True)
    finally:
        chat.close()
    out = {"model": a.model, "model_file": MODELS[a.model], "split": "dev", "retries": RETRIES,
           "wall_s": round(time.time() - t0, 1), "summary": summary(results), "results": results}
    print(json.dumps(out["summary"], indent=1))
    if not a.limit:
        LOG.mkdir(parents=True, exist_ok=True)
        (LOG / f"dev_{a.model}.json").write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
        print(f"wrote {(LOG / f'dev_{a.model}.json').relative_to(ROOT)}")


if __name__ == "__main__":
    main()
