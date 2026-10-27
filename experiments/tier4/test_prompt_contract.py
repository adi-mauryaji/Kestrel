"""The Tier-4 prompts are data contracts (PREREG: R0 and Tier 4 use the same template; v3's kind
prompt likewise for its fallback, ADR-037). A prompt may name every label but must not contain
any dataset item: that would leak evaluation text into the prompt."""
import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "data"))
from prep import norm_text  # noqa: E402

CONTRACTS = ROOT / "datasets" / "contracts"
# (prompt, labels, the dataset whose items must stay out of it)
CASES = {
    "intents": ("tier4_intent_prompt.txt", "intents.txt", "kestrel_intents_v0"),
    "kinds": ("tier4_kind_prompt.txt", "kinds.txt", "kestrel_kinds_v0"),
    "kinds_v1": ("tier4_kind_prompt_v1.txt", "kinds_v1.txt", "kestrel_kinds_v1"),  # ADR-042
}


def contract(name):
    prompt, labels, dataset = CASES[name]
    labels = [l for l in (CONTRACTS / labels).read_text().split("\n") if l and not l.startswith("#")]
    return (CONTRACTS / prompt).read_text(encoding="utf-8"), labels, ROOT / "datasets" / dataset / "splits"


@pytest.mark.parametrize("name", sorted(CASES))
def test_every_label_is_defined_once_at_the_start_of_a_line(name):
    prompt, labels, _ = contract(name)
    for label in labels:
        assert prompt.count(f"\n{label}: ") == 1, label


@pytest.mark.parametrize("name", sorted(CASES))
def test_no_dataset_item_appears_in_the_prompt(name):
    prompt, _, splits = contract(name)
    prompt = norm_text(prompt)
    leaked = []
    for split in ("train", "cal", "val", "test"):
        for line in (splits / f"{split}.jsonl").open(encoding="utf-8"):
            text = norm_text(json.loads(line)["text"])
            if len(text.split()) >= 3 and text in prompt:  # 1-2 word items ("hi") are too generic
                leaked.append((split, text))
    assert not leaked, leaked


@pytest.mark.parametrize("name", sorted(CASES))
def test_the_prompt_has_no_chat_template_tokens(name):
    # The model-specific wrapper (ChatML, ...) is added in Rust, not stored here.
    prompt, _, _ = contract(name)
    assert "<|" not in prompt and "<start_of_turn>" not in prompt
