"""experiments/tier4/clinc_prompt.py -- the CLINC150 Tier-4 prompt contract (ADR-016).

The 150 CLINC150 intents (from the training split, sorted) plus `oos`, an explicit answer for
requests that match none of them. Without it, an always-SLM system would be wrong on every
out-of-scope item. Label names only: CLINC150 ships no definitions, and no dataset item may
appear in the prompt (tested).

    python experiments/tier4/clinc_prompt.py      # writes both files below
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


def render(intents: list[str]) -> str:
    head = ("You are the intent classifier of a voice assistant. Read the user's request and answer with exactly "
            "one label from this list. The labels are intent names: words joined by underscores.\n\n")
    tail = ("\noos\n\nAnswer oos when the request matches none of the listed intents. "
            "Answer with the label only.\n")
    return head + "\n".join(intents) + "\n" + tail


def main():
    from splits import load_splits
    intents = sorted({r["label"] for r in load_splits("clinc150")["train"]})
    assert len(intents) == 150 and "oos" not in intents
    LABELS_FILE.write_text("\n".join(intents + ["oos"]) + "\n")
    PROMPT_FILE.write_text(render(intents))
    print(LABELS_FILE.relative_to(ROOT), PROMPT_FILE.relative_to(ROOT))


if __name__ == "__main__":
    main()
