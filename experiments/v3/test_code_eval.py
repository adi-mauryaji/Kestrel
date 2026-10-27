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
