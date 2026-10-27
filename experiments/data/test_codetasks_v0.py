import json

from codetasks_v0 import TYPES, build, load, rows, split, validate


def fam(**kw):
    base = {"id": "fn-x", "type": "function", "entry": "double",
            "en": ["write a python function double(x) that doubles x"],
            "hi-en": ["python function double(x) likho jo x ko double kare"],
            "solution": "def double(x):\n    return 2 * x\n", "tests": "assert double(2) == 4\n"}
    return {**base, **kw}


def test_validate_refuses_what_a_task_needs_and_lacks():
    assert validate([fam()]) == []
    assert validate([fam(type="poem")]) == [f"fn-x: type 'poem' is not one of {', '.join(TYPES)}"]
    assert validate([fam(**{"hi-en": []})]) == ["fn-x: needs at least one en and one hi-en request"]
    assert validate([fam(en=["write a doubling function"])]) == [
        "fn-x: request 'write a doubling function' does not name double"]
    assert validate([fam(tests="")]) == ["fn-x: a function task needs its solution and tests"]
    assert validate([fam(), fam()]) == ["fn-x: id used twice"]
    fix = fam(id="fix-x", type="fix", code="def double(x):\n    return x\n", en=["fix double:\n{code}"],
              **{"hi-en": ["double theek karo:\n{code}"]})
    assert validate([fix]) == []
    assert validate([{**fix, "en": ["fix double"]}]) == ["fix-x: request 'fix double' does not show {code}"]
    ex = {"id": "ex-x", "type": "explain", "code": "print(1)", "facts": [["print"]],
          "en": ["explain:\n{code}"], "hi-en": ["samjhao:\n{code}"]}
    assert validate([ex]) == []
    assert validate([{**ex, "facts": []}]) == ["ex-x: an explain task needs its facts"]
    sc = {"id": "sc-x", "type": "script", "solution": "print(input())", "cases": [{"stdin": "a\n", "stdout": "a"}],
          "en": ["echo a line"], "hi-en": ["ek line wapas print karo"]}
    assert validate([sc]) == []
    assert validate([{**sc, "cases": []}]) == ["sc-x: a script task needs its solution and cases"]


def test_the_draft_is_valid_with_every_type_in_both_languages():
    families = load()
    assert validate(families) == []
    for t in TYPES:
        assert sum(f["type"] == t for f in families) >= 5, t


def test_rows_fill_in_the_code_and_carry_no_solution():
    fix = fam(id="fix-x", type="fix", code="def double(x):\n    return x\n", en=["fix double:\n{code}"],
              **{"hi-en": ["double theek karo:\n{code}"]})
    r = rows([fix])[0]
    assert r["text"] == "fix double:\ndef double(x):\n    return x\n"
    assert (r["id"], r["family"], r["lang"], r["type"], r["entry"]) == ("fix-x/en/0", "fix-x", "en", "fix", "double")
    assert "solution" not in r and r["tests"] == "assert double(2) == 4\n"


def test_families_never_straddle_and_both_splits_hold_every_type():
    parts = split(rows(load()))
    home = {}
    for name, rs in parts.items():
        assert {r["type"] for r in rs} == set(TYPES), name
        for r in rs:
            assert home.setdefault(r["family"], name) == name
    assert set(parts) == {"dev", "test"}


def test_the_build_is_byte_for_byte_deterministic_and_says_claude_reviewed(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    build(a)
    build(b)
    names = sorted(p.name for p in a.iterdir())
    assert names == ["dev.jsonl", "manifest.json", "test.jsonl"]
    assert all((a / n).read_bytes() == (b / n).read_bytes() for n in names)
    # ADR-045: the human waived the review; never "reviewed", which means the human's.
    assert json.loads((a / "manifest.json").read_text())["status"] == "claude-reviewed"
