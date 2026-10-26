import copy
import json
from collections import Counter

from personas_v0 import assign_splits, build, events, items, load, recent, resolve_due, validate


def test_the_committed_draft_is_valid():
    assert validate(load()) == []


def test_each_persona_gets_20_personalized_10_general_and_5_events():
    data = load()
    rows = items(data, assign_splits(data["personas"]))
    per = Counter((r["persona"], r["type"]) for r in rows)
    for p in data["personas"]:
        assert per[(p["id"], "personalized")] == 20 and per[(p["id"], "general")] == 10, p["id"]
    assert sum(r["variant"] for r in rows) == 8 * 5
    evs = events(data)
    assert [e["k"] for e in evs if e["persona"] == "p1"] == [1, 2, 3, 4, 5]
    assert evs[0]["text"] == "when I say standup doc, I mean team sync"


def test_personas_are_split_whole_two_per_language_per_split():
    data = load()
    splits = assign_splits(data["personas"])
    langs = {p["id"]: p["lang"] for p in data["personas"]}
    assert Counter((s, langs[p]) for p, s in splits.items()) == {
        ("dev", "en"): 2, ("test", "en"): 2, ("dev", "hi-en"): 2, ("test", "hi-en"): 2}
    assert splits == assign_splits(list(reversed(data["personas"])))


def test_expectations_are_computed_from_today_and_the_notes():
    data = load()
    assert resolve_due("tomorrow", data["today"]) == "2026-10-24"
    assert resolve_due("saturday", data["today"]) == "2026-10-24"   # today is a Friday
    assert resolve_due("monday", data["today"]) == "2026-10-26"
    assert resolve_due("friday", data["today"]) == "2026-10-30"     # strictly after today
    assert recent(data, "scrum", 2) == ["2026-10-21-scrum.md", "2026-10-20-scrum.md"]
    assert recent(data, "gym", 5) == ["2026-10-22-gym.md", "2026-10-18-gym.md"]
    assert recent(data, None, 3) == ["2026-10-22-lecture.md", "2026-10-22-gym.md", "2026-10-22-groceries.md"]
    rows = {r["id"]: r for r in items(data, assign_splits(data["personas"]))}
    call = rows["p1/grp-call-en"]
    assert call["text"] == "remind me to call the gang tomorrow"
    assert call["canonical"] == "remind me to call riya and kabir tomorrow"
    assert call["expect"]["tasks_added"] == [{"title": "call riya and kabir", "due": "2026-10-24"}]
    var = rows["p5/plc-var-hi"]
    assert var["variant"] and var["text"] == "kal adde pe chai peene ka reminder lagao"
    assert var["canonical"] == "kal sharma tea stall pe chai peene ka reminder lagao"
    assert rows["p2/day-form-en"]["expect"]["tasks_added"][0]["due"] == "2026-10-30"


def test_validation_catches_dataset_mistakes():
    def broken(mutate):
        data = copy.deepcopy(load())
        mutate(data)
        return validate(data)

    p1 = lambda d: d["personas"][0]  # noqa: E731
    cases = {
        "path": lambda d: p1(d)["aliases"][0].update(path="docs/nope.md"),
        "tag": lambda d: p1(d)["aliases"][2].update(tag="nope"),
        "starts with": lambda d: p1(d)["aliases"][0].update(phrase="my standup doc"),
        "kinds": lambda d: p1(d)["aliases"][1].update(kind="doc", path="docs/team-sync.md"),
        "variant": lambda d: p1(d)["aliases"][0].update(variant="standup doc"),
        "general": lambda d: d["general"].append({"lang": "en", "intent": "read_document",
                                                  "text": "open the gang", "expect": {"read": ["docs/recipes.md"]}}),
    }
    for word, mutate in cases.items():
        errors = broken(mutate)
        assert errors and any(word in e for e in errors), (word, errors)


def test_build_writes_items_events_fixture_and_manifest(tmp_path):
    m = build(tmp_path)
    rows = [json.loads(l) for l in (tmp_path / "items.jsonl").read_text().splitlines()]
    evs = [json.loads(l) for l in (tmp_path / "events.jsonl").read_text().splitlines()]
    fixture = json.loads((tmp_path / "fixture.json").read_text())
    assert (len(rows), len(evs)) == (240, 40) == (m["items"], m["events"])
    assert fixture == {"today": "2026-10-23", "notes": "datasets/kestrel_personas_v0/fixtures/notes",
                       "tasks": [], "memories": []}
    assert m["status"] == "reviewed"
