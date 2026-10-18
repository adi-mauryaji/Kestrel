"""experiments/human/build_page.py -- builds the Kestrel Request Lab page and records the κ sample.

    python experiments/human/build_page.py

Writes datasets/kestrel_intents_v0/annotation/kappa_sample_v0.json (the 20% sample's ids) and
experiments/human/collect.html (collect.template.html with the data inlined; published as an
Artifact). The page is blind: items carry only id and text, tasks only their wording.
"""
from __future__ import annotations

import json
from pathlib import Path

from human_data import GUIDE, RULES, TASKS, kappa_sample, load_intents_v0

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
TEMPLATE = HERE / "collect.template.html"
PAGE = HERE / "collect.html"
SAMPLE = ROOT / "datasets" / "kestrel_intents_v0" / "annotation" / "kappa_sample_v0.json"
SEED, FRAC = 0, 0.2
GLOSS = {"search_notes": "find notes", "read_document": "open one document", "create_task": "reminder / to-do",
         "delete_task": "remove a task", "summarize_notes": "summary", "set_preference": "remember about me",
         "retrieve_memory": "what do you know about me", "chitchat": "small talk", "unsupported": "can't do that",
         "compound": "two or more things"}


def page_data(sample: list[dict]) -> dict:
    return {
        "version": 1,
        "items": [{"id": r["id"], "text": r["text"]} for r in sample],
        "guide": [{"label": lab, "definition": d, "gloss": GLOSS[lab], "examples": ex} for lab, d, ex in GUIDE],
        "rules": RULES,
        "tasks": [{"id": t["id"], "ask": t["ask"]} for t in TASKS],
    }


def build(page: Path = PAGE, sample_file: Path = SAMPLE) -> list[dict]:
    sample = kappa_sample(load_intents_v0(), frac=FRAC, seed=SEED)
    sample_file.parent.mkdir(parents=True, exist_ok=True)
    sample_file.write_text(json.dumps({"seed": SEED, "frac": FRAC, "n": len(sample),
                                       "ids": [r["id"] for r in sample]}, indent=1) + "\n")
    data = json.dumps(page_data(sample), ensure_ascii=False).replace("</", "<\\/")
    html = TEMPLATE.read_text(encoding="utf-8")
    assert html.count("/*__DATA__*/null") == 1
    page.write_text(html.replace("/*__DATA__*/null", data), encoding="utf-8")
    return sample


if __name__ == "__main__":
    s = build()
    print(f"{len(s)} items -> {PAGE.relative_to(ROOT)}, {SAMPLE.relative_to(ROOT)}")
