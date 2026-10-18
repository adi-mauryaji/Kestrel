import pytest

from clinc_prompt import DESC_FILE, DESC_PROMPT_FILE, LABELS_FILE, PROMPT_FILE, load_descriptions, render, render_desc
from splits import load_splits

clinc = pytest.importorskip("clinc150")
pytestmark = pytest.mark.skipif(not clinc.RAW.exists(), reason="needs datasets/raw (scripts/fetch_datasets.py)")


def test_labels_are_the_150_train_intents_then_oos():
    labels = [l for l in LABELS_FILE.read_text().split("\n") if l]
    train = sorted({r["label"] for r in load_splits("clinc150")["train"]})
    assert labels == train + ["oos"]


def test_committed_prompt_is_current_and_defines_every_label_once():
    labels = [l for l in LABELS_FILE.read_text().split("\n") if l]
    assert PROMPT_FILE.read_text() == render(labels[:-1])
    for label in labels:
        assert PROMPT_FILE.read_text().count(f"\n{label}\n") == 1, label


@pytest.mark.parametrize("path", [PROMPT_FILE, DESC_PROMPT_FILE], ids=["labels", "descriptions"])
def test_no_clinc150_item_appears_in_the_prompt(path):
    from prep import norm_text

    prompt = norm_text(path.read_text().replace("\n", " "))
    official = clinc.load_official()
    leaked = [r["text"] for rows in official.values() for r in rows
              if len(r["text"].split()) >= 3 and norm_text(r["text"]) in prompt]
    assert not leaked, leaked[:5]


def test_every_intent_has_one_description():
    desc = load_descriptions(DESC_FILE)
    labels = [l for l in LABELS_FILE.read_text().split("\n") if l]
    assert list(desc) == labels[:-1]  # the 150 intents, in label order; oos is defined by the prompt
    assert all(d and d == d.strip() and "\t" not in d for d in desc.values())


def test_committed_description_prompt_is_current_and_lists_every_label_once():
    labels = [l for l in LABELS_FILE.read_text().split("\n") if l]
    text = DESC_PROMPT_FILE.read_text()
    assert text == render_desc(load_descriptions(DESC_FILE))
    for label in labels:
        assert text.count(f"\n{label}: ") == 1, label
