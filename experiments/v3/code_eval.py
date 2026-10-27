#!/usr/bin/env python3
"""experiments/v3/code_eval.py -- v3's code requests on Kestrel-CodeTasks-v0 (ADR-036), dev only.

Judging (hidden from the model): a `function` or `fix` answer is imported as solution.py and the family's
asserts run against it; a `script` answer is run on each case's stdin and its output compared, ignoring
case and spacing at the ends; an `explain` answer holds every fact (any of its ways). Code runs only in
experiments/v3/sandbox.py.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from sandbox import run_python  # noqa: E402

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
