import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "experiments" / "data"))
from codetasks_v0 import load, rows  # noqa: E402

from code_eval import blocks, facts_hold, passes  # noqa: E402


def row_of(family):
    return next(r for r in rows([family]) if r["lang"] == "en")


@pytest.mark.parametrize("family", [f for f in load() if f["type"] != "explain"], ids=lambda f: f["id"])
def test_every_reference_solution_passes_and_every_broken_code_fails(family):
    r = row_of(family)
    assert passes(r, family["solution"]), family["id"]
    if family["type"] == "fix":
        assert not passes(r, family["code"]), f"{family['id']}: the code to fix already passes"


def test_a_script_is_judged_by_its_output_whatever_the_case_and_spacing():
    r = {"type": "script", "cases": [{"stdin": "4\n", "stdout": "even"}]}
    assert passes(r, "print('  Even ')")
    assert not passes(r, "print('Enter a number:', 'even')")


def test_facts_hold_in_any_of_their_ways():
    r = {"type": "explain", "facts": [["square"], ["even"]]}
    assert facts_hold(r, "It makes a list of the squares of the even numbers below 10.")
    assert not facts_hold(r, "It makes a list of squares.")


def test_blocks_take_the_code_and_the_self_check_apart():
    text = "Here:\n```python\ndef f():\n    return 1\n```\nChecks:\n```tests\nassert f() == 1\n```\n"
    assert blocks(text) == ("def f():\n    return 1\n", "assert f() == 1\n")
    assert blocks("```\nprint(1)\n```") == ("print(1)\n", "")
    assert blocks("print(1)") == ("print(1)", "")
    assert blocks("```python\nx = 1\n```\n```input\n5\n```")[1] == "5\n"


def test_the_self_check_runs_the_models_own_asserts_or_sample_input():
    from code_eval import self_check
    fn = {"type": "function"}
    assert self_check(fn, "def f():\n    return 1\n", "assert f() == 1\n") == (True, "")
    ok, err = self_check(fn, "def f():\n    return 2\n", "assert f() == 1\n")
    assert not ok and "AssertionError" in err
    ok, err = self_check(fn, "def f(:\n", "")
    assert not ok and "SyntaxError" in err, "no asserts still imports the code"
    sc = {"type": "script"}
    assert self_check(sc, "print(int(input()) * 2)", "4\n") == (True, "")
    # No sample input: the script is only compiled (run on empty input, a reader fails, and the
    # feedback taught the model to stop reading input: the smoke run, 2026-10-27).
    assert self_check(sc, "print(int(input()) * 2)", "") == (True, "")
    ok, err = self_check(sc, "print(int(input() * 2)", "")
    assert not ok and "SyntaxError" in err


def test_feedback_shows_the_code_and_the_end_of_the_error():
    from code_eval import feedback
    msg = feedback("write f", "def f():\n    pass\n", "x" * 3000 + "\nNameError: y")
    assert msg.startswith("write f\n\nYour code:\n```python\ndef f():\n    pass\n```\n")
    assert msg.rstrip().endswith("Fix the code. Reply in the same form as before.")
    assert "NameError: y" in msg and "x" * 1600 not in msg
