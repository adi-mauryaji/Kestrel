"""The Tier-4 plan prompt and its examples are data contracts (ADR-023). Gold plans are never shown
to a model: no Kestrel-Tasks-v0 request may appear in them, no example may have the op structure of
a test family's gold plan (dev families may share one: dev is for tuning), and an example's content
words must not mostly be a dataset request's."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CONTRACTS = ROOT / "datasets" / "contracts"
PROMPT = (CONTRACTS / "tier4_plan_prompt.txt").read_text(encoding="utf-8")
EXAMPLES = [json.loads(l) for l in (CONTRACTS / "tier4_plan_examples.jsonl").read_text(encoding="utf-8").splitlines()]
ITEMS = [json.loads(l) for l in (ROOT / "datasets/kestrel_tasks_v0/build/items.jsonl").read_text(encoding="utf-8").splitlines()]
OPS = ["fetch_recent_notes", "search_notes", "read_document", "summarize", "extract_action_items", "create_task",
       "delete_task", "retrieve_memory", "update_preference"]


def test_every_op_is_described_once_at_the_start_of_a_line():
    for op in OPS:
        assert sum(line.startswith(op + " ") or line.startswith(op + ":") for line in PROMPT.splitlines()) == 1, op


def test_the_placeholders_are_the_ones_the_runtime_fills():
    import re
    assert set(re.findall(r"\{([a-z_]+)\}", PROMPT)) == {"today", "weekday", "calendar", "tomorrow", "day_after", "documents", "tags"}
    for ex in EXAMPLES:
        assert set(re.findall(r"\{([a-z_]+)\}", json.dumps(ex))) <= {"tomorrow"}, ex


def test_no_dataset_request_or_plan_leaks_into_the_contracts():
    text = PROMPT.lower() + json.dumps(EXAMPLES).lower()
    for item in ITEMS:
        if len(item["text"].split()) >= 3:
            assert item["text"].lower() not in text, item["id"]
    test_shapes = {shape(i["plan"]) for i in ITEMS if i["split"] == "test" and i["plan"]}
    for ex in EXAMPLES:
        words = content(ex["request"])
        for item in ITEMS:
            overlap = len(words & content(item["text"])) / len(words)
            assert overlap < 0.5, (ex["request"], item["id"], round(overlap, 2))
        assert shape(ex["plan"]) not in test_shapes, ex["request"]


FUNCTION_WORDS = {"my", "the", "a", "an", "and", "aur", "of", "to", "in", "on", "for", "from", "into", "me", "i",
                  "ke", "ka", "ki", "ko", "mein", "se", "do", "karo", "kar", "hai", "mera", "meri", "mere",
                  "notes", "note", "tasks", "task", "ab", "due", "what", "you", "about",
                  # the Hinglish reminder construction itself ("kal ... karna yaad dilana"), in every reminder
                  "kal", "karna", "yaad", "dilana"}


def content(text):
    return {w.strip(",.") for w in text.lower().split()} - FUNCTION_WORDS


def shape(plan):
    """The op structure: each node's op and its deps' ops, in order."""
    ops = {n["id"]: n["op"] for n in plan["nodes"]}
    return tuple((n["op"], tuple(sorted(ops[d] for d in n.get("deps", [])))) for n in plan["nodes"])


def test_the_prompt_has_no_chat_template_tokens():
    assert "<|" not in PROMPT and "<turn|>" not in PROMPT
