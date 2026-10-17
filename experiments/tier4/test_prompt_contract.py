"""The Tier-4 intent prompt is a data contract (PREREG: R0 and Tier 4 use the same template).
It may name every label but must not contain any dataset item: that would leak evaluation text
into the prompt."""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "data"))
from prep import norm_text  # noqa: E402

PROMPT = (ROOT / "datasets" / "contracts" / "tier4_intent_prompt.txt").read_text(encoding="utf-8")
LABELS = [l for l in (ROOT / "datasets" / "contracts" / "intents.txt").read_text().split("\n")
          if l and not l.startswith("#")]
SPLITS = ROOT / "datasets" / "kestrel_intents_v0" / "splits"


def test_every_label_is_defined_once_at_the_start_of_a_line():
    for label in LABELS:
        assert PROMPT.count(f"\n{label}: ") == 1, label


def test_no_dataset_item_appears_in_the_prompt():
    prompt = norm_text(PROMPT)
    leaked = []
    for split in ("train", "cal", "val", "test"):
        for line in (SPLITS / f"{split}.jsonl").open(encoding="utf-8"):
            text = norm_text(json.loads(line)["text"])
            if len(text.split()) >= 3 and text in prompt:  # 1-2 word items ("hi") are too generic
                leaked.append((split, text))
    assert not leaked, leaked


def test_the_prompt_has_no_chat_template_tokens():
    # The model-specific wrapper (ChatML, ...) is added in Rust, not stored here.
    assert "<|" not in PROMPT and "<start_of_turn>" not in PROMPT
