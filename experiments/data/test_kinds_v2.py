import json

from kinds_v2 import KINDS, load, validate


def fam(**kw):
    base = {"id": "code-x", "kind": "code", "en": ["write a python function that adds two numbers"],
            "hi-en": ["do numbers jodne ka python function likho"]}
    return {**base, **kw}


def test_validate_refuses_old_ids_known_texts_unknown_kinds_and_missing_languages():
    assert validate([fam()]) == []
    assert validate([fam(kind="sql")]) == ["code-x: kind 'sql' is not one of " + ", ".join(KINDS)]
    assert validate([fam(id="notes-01")]) == ["code-x: id already in Kinds-v0 or Kinds-v1".replace("code-x", "notes-01")]
    assert validate([fam(en=["who is handling the dashboard?"])]) == [
        "code-x: 'who is handling the dashboard?' is already in Kinds-v0, Kinds-v1 or Intents-v0"]
    assert validate([fam(**{"hi-en": []})]) == ["code-x: needs at least one en and one hi-en request"]
    assert validate([fam(), fam()]) == ["code-x: id used twice"]


def test_the_draft_is_valid_and_code_is_its_largest_kind():
    families = load()
    assert validate(families) == []
    by = {k: sum(f["kind"] == k for f in families) for k in KINDS}
    assert by["code"] >= 10 and by["code"] == max(by.values())
    assert {"code", "chat", "app", "notes", "rewrite"} <= {f["kind"] for f in families}


def test_every_kinds_v1_row_keeps_its_frozen_split():
    import kinds_v1
    from kinds_v2 import rows, split
    v1 = kinds_v1.split(kinds_v1.rows(kinds_v1.load())[0])
    v2 = split(rows(load())[0])
    for name in v1:
        assert [r for r in v2[name] if r["source"] != "kinds_v2"] == v1[name], name


def test_families_never_straddle_and_every_split_has_every_kind_with_code():
    from kinds_v2 import LABELS, rows, split
    assert "code" in LABELS
    parts = split(rows(load())[0])
    home = {}
    for name, rs in parts.items():
        assert {r["label"] for r in rs} == set(LABELS), name
        assert any(r["label"] == "code" and r["source"] == "kinds_v2" for r in rs), name
        for r in rs:
            assert home.setdefault(r["group"], name) == name, r["group"]


def test_the_build_is_deterministic_and_counts_its_fresh_test_items(tmp_path):
    from kinds_v2 import build
    a, b = tmp_path / "a", tmp_path / "b"
    build(a)
    build(b)
    names = sorted(p.name for p in a.iterdir())
    assert names == ["cal.jsonl", "manifest.json", "test.jsonl", "train.jsonl", "val.jsonl"]
    assert all((a / n).read_bytes() == (b / n).read_bytes() for n in names)
    m = json.loads((a / "manifest.json").read_text())
    assert m["status"] == "draft" and m["conflicts_dropped"] == []
    fresh = [json.loads(l) for l in (a / "test.jsonl").open() if json.loads(l)["source"] == "kinds_v2"]
    assert m["splits"]["test"]["fresh_items"] == len(fresh) > 0
