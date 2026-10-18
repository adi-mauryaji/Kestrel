"""experiments/human/human_data.py -- human data for κ (Week 4) and the Hinglish claim (RQ7).

Both collections run through one page, experiments/human/collect.html (built by build_page.py):

1. Second annotator: at least 20% of Kestrel-Intents-v0, stratified by label x language, labelled
   blind (no gold label shown) from the guide's definitions and rules. Compound requests are
   one category, `compound`. Agreement with the gold labels is Cohen's κ.
2. Human-written requests: people write what they would say to Kestrel for ten plain-language
   tasks, in Hinglish (English optional). This becomes a human-written test set for RQ7.
   Today's items were drafted by Claude, which also wrote the Tier-1 rules (dataset card).

A submission is JSON: the page saves it to the artifact's store, or the person copies it and
sends it. `parse_submission` validates either kind before anything enters the repo.
"""
from __future__ import annotations

import json
import math
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "data"))
from prep import norm_text, seeded_order  # noqa: E402

SPLITS = ROOT / "datasets" / "kestrel_intents_v0" / "splits"
INTENTS = [l for l in (ROOT / "datasets" / "contracts" / "intents.txt").read_text().split("\n")
           if l and not l.startswith("#")]
KAPPA_LABELS = INTENTS + ["compound"]
LANGS = ("en", "hi-en")
MAX_TEXT = 300

# What writers are asked for, in plain words. No dataset text appears here (tested).
TASKS = [
    {"id": "remind", "intent": "create_task",
     "ask": "Ask Kestrel to remind you about something later, or to add something to your to-do list."},
    {"id": "cancel", "intent": "delete_task",
     "ask": "Ask Kestrel to remove or cancel a reminder or task you made earlier."},
    {"id": "find", "intent": "search_notes",
     "ask": "Ask Kestrel to find your notes about some topic."},
    {"id": "open", "intent": "read_document",
     "ask": "Ask Kestrel to open or show one particular file or document, by its name."},
    {"id": "summary", "intent": "summarize_notes",
     "ask": "Ask Kestrel for a short summary of your notes or of a document."},
    {"id": "remember", "intent": "set_preference",
     "ask": "Tell Kestrel something it should remember about you, or how you like things done."},
    {"id": "recall", "intent": "retrieve_memory",
     "ask": "Ask Kestrel what it remembers about you, or about something you told it before."},
    {"id": "chat", "intent": "chitchat",
     "ask": "Just talk to Kestrel: say hello, thank it, or ask it about itself."},
    {"id": "outside", "intent": "unsupported",
     "ask": "Ask for something an offline notes-and-tasks assistant cannot do: weather, food orders, "
            "sending a message, news, a calculation."},
    {"id": "two", "intent": "compound",
     "ask": "Ask for two different things in one sentence (for example, find something and then do something with it)."},
]
TASK_IDS = {t["id"] for t in TASKS}


def load_intents_v0() -> list[dict]:
    return [json.loads(l) for s in ("train", "cal", "val", "test")
            for l in (SPLITS / f"{s}.jsonl").open(encoding="utf-8")]


def gold_kappa_label(row: dict) -> str:
    return "compound" if "|" in row["label"] else row["label"]


def kappa_sample(rows, frac: float = 0.2, seed: int = 0, exclude_texts=()) -> list[dict]:
    """ceil(frac * n) items from every (label, language) cell, spread over template families
    (round-robin over families in seeded order), returned in one seeded presentation order."""
    banned = {norm_text(t) for t in exclude_texts}
    cells = defaultdict(list)
    for r in rows:
        if norm_text(r["text"]) not in banned:
            cells[(gold_kappa_label(r), r["lang"])].append(r)
    picked = []
    for cell in sorted(cells):
        cands = cells[cell]
        quota = math.ceil(frac * len(cands))
        order = {k: i for i, k in enumerate(seeded_order([r["id"] for r in cands], seed))}
        by_family = defaultdict(list)
        for r in sorted(cands, key=lambda r: order[r["id"]]):
            by_family[r.get("family", r["id"])].append(r)
        families = sorted(by_family, key=lambda f: order[by_family[f][0]["id"]])
        take, depth = [], 0
        while len(take) < quota:
            for f in families:
                if depth < len(by_family[f]) and len(take) < quota:
                    take.append(by_family[f][depth])
            depth += 1
        picked += take
    order = {k: i for i, k in enumerate(seeded_order([r["id"] for r in picked], seed + 1))}
    return sorted(picked, key=lambda r: order[r["id"]])


def cohen_kappa(a, b) -> float:
    if len(a) != len(b) or not a:
        raise ValueError("cohen_kappa needs two equally long, non-empty label lists")
    n = len(a)
    po = sum(x == y for x, y in zip(a, b)) / n
    cats = set(a) | set(b)
    pe = sum((a.count(c) / n) * (b.count(c) / n) for c in cats)
    return 1.0 if pe == 1 else (po - pe) / (1 - pe)


def _code(sub: dict) -> str:
    code = str(sub.get("code", "")).strip()
    if not 1 <= len(code) <= 24:
        raise ValueError("code must be 1-24 characters")
    return code


def parse_submission(text: str, known_ids: set[str]) -> dict:
    """A validated, cleaned submission (annotation or written requests); ValueError if malformed."""
    sub = json.loads(text)
    kind = sub.get("kind")
    if sub.get("version") != 1 or kind not in ("kestrel-annotation", "kestrel-requests"):
        raise ValueError(f"unknown kind/version: {kind!r} v{sub.get('version')}")
    if kind == "kestrel-annotation":
        answers = sub.get("answers")
        if not isinstance(answers, dict):
            raise ValueError("answers must be an object {item id: label}")
        for item, label in answers.items():
            if item not in known_ids:
                raise ValueError(f"unknown item id {item!r}")
            if label not in KAPPA_LABELS:
                raise ValueError(f"unknown label {label!r} for {item!r}")
        return {"kind": kind, "code": _code(sub), "answers": dict(answers)}
    if sub.get("consent") is not True:
        raise ValueError("written requests need consent: true")
    out = []
    for req in sub.get("requests", []):
        if req.get("task") not in TASK_IDS:
            raise ValueError(f"unknown task {req.get('task')!r}")
        if req.get("lang") not in LANGS:
            raise ValueError(f"unknown lang {req.get('lang')!r}")
        t = " ".join(str(req.get("text", "")).split())
        if not t or len(t) > MAX_TEXT:
            raise ValueError(f"request text must be 1-{MAX_TEXT} characters")
        out.append({"task": req["task"], "lang": req["lang"], "text": t})
    return {"kind": kind, "code": _code(sub), "consent": True, "requests": out}

# The annotator's guide: docs/intent_ontology.md's definitions and decision rules. The examples are
# new ones, not dataset items (tested), so the guide cannot show the annotator a sampled answer.
GUIDE = [
    ("search_notes", "Find notes by topic or keywords.",
     ["find what i wrote about the hackathon", "biology lab wale notes dhoondh do"]),
    ("read_document", "Open or show one specific document, named by its file name or title.",
     ["open budget-2026.xlsx", "resume.pdf kholo"]),
    ("create_task", "Add a task or a reminder for later.",
     ["remind me to renew my bus pass on monday", "parso dentist appointment yaad dilana"]),
    ("delete_task", "Delete, remove or cancel a task or reminder that already exists.",
     ["cancel the gym reminder", "dentist wala reminder hata do"]),
    ("summarize_notes", "Summarize notes or a document: a summary, gist, recap or key points.",
     ["give me the gist of my physics notes", "meeting notes ka chhota summary bana do"]),
    ("set_preference", "Tell Kestrel to remember a standing preference or a personal fact.",
     ["remember that i am allergic to peanuts", "yaad rakhna ki mujhe chai bina cheeni pasand hai"]),
    ("retrieve_memory", "Ask what Kestrel has stored about the user.",
     ["what do you know about my diet", "maine apne bhai ke baare mein kya bataya tha"]),
    ("chitchat", "Greetings, thanks, small talk, or questions about Kestrel itself. No action.",
     ["how old are you, kestrel", "tumhara naam kisne rakha"]),
    ("unsupported", "Anything no Kestrel tool can do: internet, email, calls, messages, device control, "
                    "shopping, travel, live data, general knowledge, calculations.",
     ["book a cab to the station", "aaj cricket match ka score kya hai"]),
    ("compound", "Two or more different actions in one request.",
     ["find my chemistry notes and summarize them", "purana reminder hatao aur kal ke liye naya lagao"]),
]
RULES = [
    "Two or more actions asked: compound.",
    "A summary, gist, recap or key points: summarize_notes, even when one document is named.",
    "One named document to open, show or read: read_document. A topic or keywords instead: search_notes.",
    "What Kestrel has stored about the user: retrieve_memory. The user's own notes and files: search_notes.",
    "Keeping a standing preference or a personal fact about the user: set_preference.",
    "Something to do later: create_task. Removing or cancelling one: delete_task.",
    "A capability Kestrel lacks: unsupported. Calling or messaging someone right now is unsupported; "
    "a reminder to do it later is create_task.",
    "Social talk with no action: chitchat.",
    "Label what the person asks for, not what Kestrel would do well. Ignore spelling, casing and mixing "
    "of Hindi and English: the same request gets the same label in either language.",
]
