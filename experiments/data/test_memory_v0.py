import copy
import json

from memory_v0 import DAY, assign_splits, build, load, query_rows, store_rows, validate


def test_the_committed_draft_is_valid():
    assert validate(load()) == []


def test_groups_are_split_whole_and_both_splits_get_topics_and_negatives():
    data = load()
    splits = assign_splits(data["queries"])
    rows = query_rows(data["queries"], splits)
    for group in {r["group"] for r in rows}:
        assert len({r["split"] for r in rows if r["group"] == group}) == 1, group
    for split in ("dev", "test"):
        types = {r["type"] for r in rows if r["split"] == split}
        assert types == {"lexical", "paraphrase", "negative"}, (split, types)
    assert splits == assign_splits(list(reversed(data["queries"])))  # depends on the ids only


def test_store_rows_carry_times_approval_and_expiry():
    data = load()
    rows = {r["id"]: r for r in store_rows(data["memories"], data["now"])}
    now = data["now_unix"]
    assert [r["row"] for r in store_rows(data["memories"], data["now"])] == list(range(1, len(rows) + 1))
    pref = rows["pref-length"]
    assert (pref["approved"], pref["updated_at"], pref["expires_at"]) == (True, now - 12 * DAY, None)
    assert rows["guess-asha-spicy"]["approved"] is False
    assert rows["task-demo-room"]["expires_at"] == now - 5 * DAY
    assert rows["travel-flight"]["expires_at"] == now + 11 * DAY


def _broken(mutate):
    data = copy.deepcopy(load())
    mutate(data)
    return validate(data)


def test_validation_catches_label_and_store_mistakes():
    def mem(data, mid):
        return next(m for m in data["memories"] if m["id"] == mid)

    def query(data, qid):
        return next(q for q in data["queries"] if q["id"] == qid)

    cases = {
        "duplicate": lambda d: d["memories"].append(dict(mem(d, "pref-length"))),
        "unknown memory": lambda d: query(d, "q-pref-01").update(expect=["nope"]),
        "pending or expired": lambda d: query(d, "q-pref-01").update(expect=["guess-meeting-time"]),
        "pending": lambda d: mem(d, "pref-length").update(pending=True),
        "negative": lambda d: query(d, "q-neg-01").update(expect=["pref-length"]),
        "trap": lambda d: query(d, "q-neg-01").update(trap=["pref-length"]),
        "importance": lambda d: mem(d, "pref-length").update(importance=6),
        "key": lambda d: mem(d, "pref-length").update(key="Answer Length"),
        "lang": lambda d: query(d, "q-pref-01").update(lang="fr"),
        "group": lambda d: query(d, "q-pref-01").update(group="nowhere"),
    }
    for word, mutate in cases.items():
        errors = _broken(mutate)
        assert errors and any(word in e for e in errors), (word, errors)


def test_build_writes_the_store_the_queries_and_a_manifest(tmp_path):
    manifest = build(tmp_path)
    store = [json.loads(line) for line in (tmp_path / "store.jsonl").read_text().splitlines()]
    queries = [json.loads(line) for line in (tmp_path / "queries.jsonl").read_text().splitlines()]
    assert len(store) == manifest["memories"] == len(load()["memories"])
    assert len(queries) == manifest["queries"]
    assert manifest["status"] == "draft"
    assert manifest["splits"]["dev"]["queries"] + manifest["splits"]["test"]["queries"] == len(queries)
    assert manifest["now_unix"] == load()["now_unix"]
    on_disk = json.loads((tmp_path / "manifest.json").read_text())
    assert on_disk == manifest
