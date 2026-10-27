from kinds_v1 import KINDS, load, validate


def fam(**kw):
    base = {"id": "notes-x", "kind": "notes", "en": ["when is my flight?"], "hi-en": ["meri flight kab hai?"]}
    return {**base, **kw}


def test_validate_refuses_unknown_kinds_v0_ids_known_texts_and_missing_languages():
    assert validate([fam()]) == []
    assert validate([fam(kind="code")]) == ["notes-x: kind 'code' is not one of " + ", ".join(KINDS)]
    assert validate([fam(id="chat-01", kind="chat")]) == ["chat-01: id already in Kinds-v0"]
    assert validate([fam(en=["send this email to asha now"])]) == [
        "notes-x: 'send this email to asha now' is already in Kinds-v0 or Intents-v0"]
    assert validate([fam(**{"hi-en": []})]) == ["notes-x: needs at least one en and one hi-en request"]
    assert validate([fam(), fam()]) == ["notes-x: id used twice"]


def test_the_draft_is_valid_and_mostly_notes():
    families = load()
    assert validate(families) == []
    assert sum(f["kind"] == "notes" for f in families) >= 12
    assert {f["kind"] for f in families} == {"notes", "app", "chat"}


def test_every_kinds_v0_row_keeps_its_frozen_split():
    import kinds_v0
    from kinds_v1 import rows, split
    v0 = kinds_v0.split(kinds_v0.rows(kinds_v0.load())[0])
    v1 = split(rows(load())[0])
    for name in v0:
        assert [r for r in v1[name] if r["source"] != "kinds_v1"] == v0[name], name


def test_families_never_straddle_and_every_split_has_every_kind_with_notes():
    from kinds_v1 import LABELS, rows, split
    assert "notes" in LABELS
    parts = split(rows(load())[0])
    home = {}
    for name, rs in parts.items():
        assert {r["label"] for r in rs} == set(LABELS), name
        for r in rs:
            assert home.setdefault(r["group"], name) == name, r["group"]


def test_the_build_is_byte_for_byte_deterministic_and_says_reviewed(tmp_path):
    import json
    from kinds_v1 import build
    a, b = tmp_path / "a", tmp_path / "b"
    build(a)
    build(b)
    names = sorted(p.name for p in a.iterdir())
    assert names == ["cal.jsonl", "manifest.json", "test.jsonl", "train.jsonl", "val.jsonl"]
    assert all((a / n).read_bytes() == (b / n).read_bytes() for n in names)
    m = json.loads((a / "manifest.json").read_text())
    assert m["status"] == "reviewed" and m["conflicts_dropped"] == []
