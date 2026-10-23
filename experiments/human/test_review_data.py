import re
from collections import Counter

from review_data import all_units, doc_comment_quotes, fixtures, rust_lists, unit

DB_ID = re.compile(r"^[A-Za-z0-9_\-.~:@+]{1,200}$")  # the artifact store's document-id grammar


def by_id():
    return {u["id"]: u for u in all_units()}


def text(u):
    return "\n".join(f"{line['k']}: {line['v']}" for line in u["lines"])


def test_every_unit_has_a_unique_store_safe_id():
    ids = [u["id"] for u in all_units()]
    assert len(ids) == len(set(ids))
    assert all(DB_ID.match(i) for i in ids)


def test_units_per_section():
    c = Counter((u["ds"], u["section"]) for u in all_units())
    assert c[("tasks", "family")] == 44 and c[("tasks", "malformed")] == 29
    assert (c[("personas", "teach")], c[("personas", "persona")], c[("personas", "pattern")],
            c[("personas", "general")]) == (6, 8, 48, 20)
    assert c[("memory", "memory")] == 40 and c[("memory", "query")] == 66
    assert c[("words", "correction")] >= 7 and c[("words", "alias")] >= 4


def test_the_hash_follows_the_content():
    a = unit("tasks", "family", "x", "t", [{"k": "en", "v": "hello"}])
    b = unit("tasks", "family", "x", "t", [{"k": "en", "v": "hello"}])
    c = unit("tasks", "family", "x", "t", [{"k": "en", "v": "hello!"}])
    assert a["hash"] == b["hash"] != c["hash"] and len(a["hash"]) == 16
    assert all_units() == all_units()  # deterministic


def test_a_family_shows_both_languages_its_plan_and_its_end_state():
    u = by_id()["tasks:family:cmp-01"]
    keys = [line["k"] for line in u["lines"]]
    assert {"en", "hi-en", "Plan", "Expected"} <= set(keys)
    t = text(u)
    assert "summarize my last three meeting notes and create a task list" in t
    assert "fetch_recent_notes(n=3, tag=meeting)" in t and "create_task(← x)" in t
    assert "send the q4 budget draft to finance" in t


def test_a_malformed_plan_shows_the_plan_and_why_it_must_be_rejected():
    t = text(by_id()["tasks:malformed:bad-01"])
    assert "run_shell" in t and "op outside the allowlist" in t


def test_a_pattern_lists_every_expansion_of_its_language():
    u = by_id()["personas:pattern:doc-open-en"]
    expansions = [line for line in u["lines"] if re.fullmatch(r"p\d", line["k"])]
    assert len(expansions) == 4  # the 4 English personas
    assert any("open my standup doc" in line["v"] and "docs/team-sync.md" in line["v"] for line in expansions)


def test_a_persona_shows_its_aliases_and_teaching_events():
    t = text(by_id()["personas:persona:p1"])
    assert "standup doc → team sync" in t and "when I say standup doc, I mean team sync" in t


def test_general_requests_are_reviewed_once_per_language():
    general = [u for u in all_units() if u["section"] == "general"]
    assert Counter(u["lang"] for u in general) == {"en": 10, "hi-en": 10}
    assert "open the project plan" in text(by_id()["personas:general:en-01"])


def test_a_query_names_the_memories_it_expects_and_must_not_return():
    t = text(by_id()["memory:query:q-asha-01"])
    assert "When is Asha's birthday?" in t and "asha-birthday" in t


def test_rust_lists_reads_nested_lists_style_tuples_and_their_doc_comments():
    src = '''
/// What opens a correction.
const OPENERS: &[&[&str]] = &[
    &["no"],
    &["that's", "wrong"],
];
const ONE_OFF: &[&str] = &[
    "this time", // a comment
    "is baar",
];
const STYLES: &[(&str, &str, &[&str])] = &[
    (
        "answer_length",
        "short",
        &[
            "short", "brief",
        ],
    ),
    ("language", "english", &["english", "angrezi"]),
];
const MAX: usize = 6;
const KEY: &str = "alias_";
'''
    assert rust_lists(src) == {
        "OPENERS": {"doc": "What opens a correction.", "items": ["no", "that's wrong"]},
        "ONE_OFF": {"doc": "", "items": ["this time", "is baar"]},
        "STYLES": {"doc": "", "items": ["answer_length = short: short, brief", "language = english: english, angrezi"]},
    }


def test_word_lists_come_from_the_rust_sources():
    units = by_id()
    assert "hamesha" in text(units["words:correction:DURABLE"])
    assert "bolun" in text(units["words:alias:SAY"])
    assert "when i say X, i mean Y" in text(units["words:alias:phrasings"])


def test_doc_comment_quotes():
    src = '/// The forms: "a b", "c d";\n/// or "e".\npub fn detect() {}\n'
    assert doc_comment_quotes(src, "pub fn detect") == ["a b", "c d", "e"]


def test_fixture_notes_are_included_for_reference():
    f = fixtures()
    assert len(f["tasks"]) == 12 and len(f["personas"]) == 22
    assert all(n["path"] and n["text"] for n in f["tasks"] + f["personas"])
