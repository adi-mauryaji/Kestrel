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
