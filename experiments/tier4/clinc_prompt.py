"""experiments/tier4/clinc_prompt.py -- the CLINC150 Tier-4 prompt contract (ADR-016).

The 150 CLINC150 intents (from the training split, sorted) plus `oos`, an explicit answer for
requests that match none of them. Without it, an always-SLM system would be wrong on every
out-of-scope item. Label names only: CLINC150 ships no definitions, and no dataset item may
appear in the prompt (tested).

A second contract (ADR-018) gives each intent a one-line description (datasets/contracts/
clinc150_descriptions.tsv, written by Claude from the label names and train examples) and a few
rules for the pairs the label-only prompt confused on val (k = 1 run, 2026-10-17).

    python experiments/tier4/clinc_prompt.py      # writes the label list and both prompts
"""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "tier3"))
sys.path.insert(0, str(ROOT / "experiments" / "data"))

CONTRACTS = ROOT / "datasets" / "contracts"
LABELS_FILE = CONTRACTS / "clinc150_labels.txt"
PROMPT_FILE = CONTRACTS / "tier4_clinc150_prompt.txt"
DESC_FILE = CONTRACTS / "clinc150_descriptions.tsv"
DESC_PROMPT_FILE = CONTRACTS / "tier4_clinc150_desc_prompt.txt"

# Distinctions the label-only prompt got wrong on val; general statements, no dataset text.
DESC_RULES = [
    "calendar, reminder, shopping_list and todo_list read the list. Adding, removing or changing an entry is the "
    "matching _update intent; setting a new reminder is reminder_update.",
    "freeze_account: the user asks for their account to be blocked. account_blocked: it is already blocked.",
    "travel_notification: the user tells the bank about their own trip. travel_alert: warnings about a place.",
    "income: how much the user earns. w2: the W-2 form. taxes: tax amounts.",
    "recipe: how to make a named dish. meal_suggestion: ideas for what to eat.",
    "restaurant_suggestion: where to eat. restaurant_reviews: how good a named restaurant is.",
    "Answer oos when the request matches none of the intents; do not pick the nearest one.",
]


def render(intents: list[str]) -> str:
    head = ("You are the intent classifier of a voice assistant. Read the user's request and answer with exactly "
            "one label from this list. The labels are intent names: words joined by underscores.\n\n")
    tail = ("\noos\n\nAnswer oos when the request matches none of the listed intents. "
            "Answer with the label only.\n")
    return head + "\n".join(intents) + "\n" + tail


def load_descriptions(path: Path = DESC_FILE) -> dict[str, str]:
    """label -> description, in file order (comment lines skipped)."""
    out = {}
    for line in path.read_text().splitlines():
        if line and not line.startswith("#"):
            label, desc = line.split("\t")
            if label in out:
                raise ValueError(f"{label} described twice")
            out[label] = desc
    return out


def render_desc(desc: dict[str, str]) -> str:
    head = ("You are the intent classifier of a voice assistant. Read the user's request and answer with exactly "
            "one label from this list. Each line gives a label (an intent name: words joined by underscores) and "
            "what it means.\n\n")
    body = "".join(f"{label}: {d}\n" for label, d in desc.items()) + "oos: none of the listed intents\n"
    rules = "\nRules:\n" + "".join(f"- {r}\n" for r in DESC_RULES) + "- Answer with the label only.\n"
    return head + body + rules


def main():
    from splits import load_splits
    intents = sorted({r["label"] for r in load_splits("clinc150")["train"]})
    assert len(intents) == 150 and "oos" not in intents
    LABELS_FILE.write_text("\n".join(intents + ["oos"]) + "\n")
    PROMPT_FILE.write_text(render(intents))
    desc = load_descriptions()
    if list(desc) != intents:
        raise ValueError("descriptions must cover the 150 train intents, in sorted order")
    DESC_PROMPT_FILE.write_text(render_desc(desc))
    print(LABELS_FILE.relative_to(ROOT), PROMPT_FILE.relative_to(ROOT), DESC_PROMPT_FILE.relative_to(ROOT))


if __name__ == "__main__":
    main()
