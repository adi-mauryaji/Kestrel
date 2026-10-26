import pytest

from writer_lang import is_hinglish, passes, script


@pytest.mark.parametrize("text, want", [
    ("Could you please send the file now?", "latin"),
    ("मीटिंग 5 बजे है।", "devanagari"),
    ("Meeting शाम 5 बजे है", "devanagari"),
    ("", "latin"),
])
def test_script_is_the_one_most_letters_are_in(text, want):
    assert script(text) == want


@pytest.mark.parametrize("text, want", [
    ("Kya aap please abhi file bhej sakte hain?", True),
    ("Meeting shaam 5 baje hai.", True),
    ("Apna id card lena mat bhoolna.", True),
    ("Main traffic mein phans gaya hoon.", True),
    ("Could you please send the file now?", False),
    ("The main issue is the date.", False),  # "main" and "the" are English here
    ("Thank you, see you tomorrow.", False),
])
def test_hinglish_is_told_by_its_hindi_words(text, want):
    assert is_hinglish(text) is want


def test_an_answer_passes_when_its_script_and_language_are_the_ones_asked_for():
    assert passes("मुझे कल देर हो जाएगी।", {"script": "devanagari"})
    assert not passes("Mujhe kal der ho jayegi.", {"script": "devanagari"})
    assert passes("Kya aap please abhi file bhej sakte hain?", {"script": "latin", "lang": "hinglish"})
    assert not passes("Could you please send the file now?", {"script": "latin", "lang": "hinglish"})
    assert passes("Could you please send the file now?", {"script": "latin", "lang": "english"})
    assert not passes("Kya aap please abhi file bhej sakte hain?", {"script": "latin", "lang": "english"})


def test_a_logged_run_is_scored_again_without_the_model(tmp_path):
    import json
    from writer_lang import rescore
    log = tmp_path / "lang_x.json"
    log.write_text(json.dumps({"tag": "x", "items": [
        {"group": "g", "answer": "Apna id card lena mat bhoolna.", "want": {"script": "latin", "lang": "hinglish"}, "pass": False},
        {"group": "g", "answer": None, "want": {"script": "devanagari"}, "pass": False}]}))
    out = rescore(log)
    assert (out["pass"], out["groups"]) == (1, {"g": {"n": 2, "pass": 1}})
    assert json.loads(log.read_text())["items"][0]["pass"] is True
