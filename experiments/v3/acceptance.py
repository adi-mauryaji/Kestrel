#!/usr/bin/env python3
"""experiments/v3/acceptance.py -- v3's acceptance run before the freeze: every goal feature, end to end.

Drives a running `kestrel serve` (a scratch one: fixture notes, its own memory and runtime dir; never the
human's) over its Unix socket, as `kestrel chat` does: {"say"} out; {"part"}, {"line"}, {"ask"}, {"done"}
in; a question is answered by the case's `answer`. Each case says what a right reply holds (and what it must
not), and may check a side effect (a note file, the task list). The log keeps every reply in full.

    XDG_RUNTIME_DIR=<scratch> python experiments/v3/acceptance.py NOTES_DIR OUT.json

The features are ADR-034/035's daily use (tasks, times, Hinglish, notes, memory, the service) and ADR-036's
v3 list (the kind decision, chat, email, rewrite, translate, notes by meaning, notes Q&A, code, save to
notes, cut-off notes).
"""
from __future__ import annotations

import json
import os
import re
import socket
import sys
import time
from pathlib import Path

DEVANAGARI = re.compile(r"[ऀ-ॿ]")


def say(sock_path: str, text: str, answer: bool | None) -> dict:
    """One request; returns the written parts, the lines, the questions asked and the seconds taken."""
    s = socket.socket(socket.AF_UNIX)
    s.connect(sock_path)
    f = s.makefile("rw", encoding="utf-8")
    t0 = time.time()
    f.write(json.dumps({"say": text}) + "\n")
    f.flush()
    parts, lines, asked = [], [], []
    for raw in f:
        m = json.loads(raw)
        if "part" in m:
            parts.append(m["part"])
        elif "line" in m:
            lines.append(m["line"])
        elif "ask" in m:
            asked.append(m["ask"])
            f.write(json.dumps({"answer": bool(answer)}) + "\n")
            f.flush()
        elif m.get("done"):
            break
    s.close()
    written = "".join(parts)
    return {"written": written, "lines": lines, "asked": asked, "s": round(time.time() - t0, 1),
            "text": (written + "\n" + "\n".join(lines)).strip()}


def has(*words):
    return lambda r, ctx: all(w.lower() in r["text"].lower() for w in words)


def has_re(pattern):
    return lambda r, ctx: re.search(pattern, r["text"], re.I | re.S) is not None


def lacks(*words):
    return lambda r, ctx: not any(w.lower() in r["text"].lower() for w in words)


def due_on_friday(r, ctx):
    """The reply's due date (YYYY-MM-DD) falls on a Friday."""
    import datetime
    m = re.search(r"(\d{4}-\d{2}-\d{2})", r["text"])
    return bool(m) and datetime.date.fromisoformat(m.group(1)).weekday() == 4


def note_saved(prefix_text):
    def check(r, ctx):
        m = re.search(r"Saved to your notes: (\S+\.md)", r["text"])
        return bool(m) and (Path(ctx["notes"]) / m.group(1)).read_text(encoding="utf-8").startswith(prefix_text)
    return check


def no_new_note(r, ctx):
    return sorted(p.name for p in Path(ctx["notes"]).iterdir()) == ctx["notes_before"]


# (feature, request, answer to a question, [checks]); the order matters: tasks build on each other.
CASES = [
    ("small talk", "hello", None, [has("tasks", "Python code", "notes")]),
    ("tasks: create with a time", "remind me to call asha tomorrow at 5 pm", None,
     [has_re(r"created task #\d+: call asha"), has_re(r"5(:00)? ?pm|17:00")]),
    ("tasks: Hinglish with a time", "kal shaam 6 baje bill bharna yaad dila dena", None,
     [has_re(r"created task #\d+: bill bharna"), has_re(r"6(:00)? ?pm|18:00")]),
    ("tasks: chat spellings", "kl subah 9 baje gym jana yaad dilana", None, [has_re(r"created task #\d+: gym")]),
    ("tasks: list (Hinglish)", "mere tasks btao", None, [has("call asha", "bill bharna", "gym")]),
    ("tasks: done", "task 1 done", None, [has_re(r"done: #1")]),
    ("tasks: rename", "rename task 2 to pay the electricity bill", None, [has("pay the electricity bill")]),
    # The first run's check looked for the word "friday"; the reply gives the date (a check error).
    ("tasks: reschedule", "move task 2 to friday", None, [has_re(r"#2"), due_on_friday]),
    ("tasks: delete asks first", "delete task 3", True, [has_re(r"deleted.*#?3|deleted task")]),
    ("tasks: list after", "show my tasks", None, [has("pay the electricity bill"), lacks("gym jana")]),
    ("notes: keyword search", "search my notes for budget", None, [has("2026-09-02-budget-plan.md")]),
    ("notes: by meaning", "search my notes for saving money every month", None, [has("budget-plan")]),
    ("notes: by meaning, Hinglish", "search my notes for maa ke liye tohfa", None, [has("mom-birthday")]),
    ("notes: summary", "summarize my latest note", None, [lambda r, c: len(r["text"]) > 40]),
    ("notes: open a document", "open wifi-setup.md", None, [has_re(r"router|password|wifi")]),
    # The first run's check looked for "short answers"; the reply is "answer_length = short" (a check error).
    ("memory: a preference", "I prefer short answers", None, [has_re(r"saved preference: answer_length = short")]),
    ("memory: recall by the stored words", "what do you remember about answer length?", None,
     [has("answer_length: short")]),
    # Known limitation (Stage 2's lexical memory search): asking for "preferences" in general finds none.
    ("memory: recall of preferences in general", "which preferences have i set?", None, [has_re(r"short")]),
    ("chat: explanation", "how does a for loop work in python?", True,
     [has("Written offline by a small model"), lambda r, c: len(r["written"]) > 80]),
    ("chat: Hinglish poem", "sardi ki subah pe chaar lines likho", True, [has("Written offline by a small model")]),
    ("email: Hinglish", "manager ko friday ki chutti ke liye mail likh do", True, [has_re(r"subject")]),
    ("email: English", "write an email to my landlord saying the kitchen tap is leaking", True,
     [has_re(r"subject"), has_re(r"tap|leak")]),
    ("rewrite: keeps English", "make this polite: send me the file now", True,
     [lambda r, c: "please" in r["text"].lower() or "could you" in r["text"].lower(), lacks("subject:")]),
    ("translate: into Hindi", "translate to hindi: good morning, see you tomorrow", True,
     [lambda r, c: DEVANAGARI.search(r["written"] + " ".join(r["lines"])) is not None]),
    ("translate: into English", "translate to english: aaj bahut garmi hai", True, [has_re(r"hot|heat")]),
    ("notes Q&A: English", "how much is my rent?", True, [has("12000"), has("budget-plan.md")]),
    ("notes Q&A: Hinglish", "maa ka birthday kab hai?", True, [has_re(r"20"), has("mom-birthday.md")]),
    ("notes Q&A: not in the notes", "goa wale hotel ka naam kya hai?", True,
     [has("The notes do not say this")]),
    ("code: write a function", "prime number check karne ka python function likho", True,
     [has("def "), has("I checked it in a safe sandbox: no errors")]),
    ("code: fix", "fix this function: def add(a, b): return a - b", True, [has("a + b"), has("sandbox")]),
    ("code: explain is chat", "ye code kya karta hai samjhao: print([x * 2 for x in range(3)])", True,
     [has("Written offline by a small model: it may be wrong"), has_re(r"0, 2, 4|\[0, 2, 4\]")]),
    ("save to notes: no", "Buy rice, dal and oil on Saturday from the market. add this to notes", False,
     [has("Not saved."), no_new_note]),
    ("save to notes: yes", "Buy rice, dal and oil on Saturday from the market. add this to notes", True,
     [has("Saved to your notes"), note_saved("Buy rice, dal and oil on Saturday from the market.")]),
    ("save to notes: nothing given", "add this to my notes", True, [has("What should I save?")]),
    ("refusal: live data", "what's the weather in delhi today?", True, [has("I can't do that")]),
    ("cut-off note", "write a very long and detailed essay on the full history of india, from ancient times to "
                     "today, with many sections", True,
     [has("Written offline"), lambda r, c: "Cut off at the length limit" in r["text"] or len(r["written"]) < 2400]),
]


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    notes, out_path = argv
    sock_path = str(Path(os.environ["XDG_RUNTIME_DIR"]) / "kestrel.sock")
    results = []
    for feature, text, answer, checks in CASES:
        ctx = {"notes": notes, "notes_before": sorted(p.name for p in Path(notes).iterdir())}
        r = say(sock_path, text, answer)
        r["passed_checks"] = [bool(c(r, ctx)) for c in checks]
        r |= {"feature": feature, "request": text, "answer": answer, "ok": all(r["passed_checks"])}
        results.append(r)
        print(f"{'PASS' if r['ok'] else 'FAIL'} {r['s']:5.1f}s  {feature}: {text[:50]!r}"
              + (f"  asked {r['asked']}" if r["asked"] else ""), flush=True)
    summary = {"cases": len(results), "passed": sum(r["ok"] for r in results),
               "failed": [r["feature"] for r in results if not r["ok"]],
               "seconds": round(sum(r["s"] for r in results), 1)}
    Path(out_path).parent.mkdir(parents=True, exist_ok=True)
    Path(out_path).write_text(json.dumps({"summary": summary, "results": results}, indent=1,
                                         ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
