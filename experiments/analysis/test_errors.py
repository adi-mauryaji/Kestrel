from errors import breakdown, discordant, error_rows
from test_analyze_e1 import scenario
from analyze_e1 import rep1


def test_error_rows_list_every_wrong_item_with_its_tier_and_answer():
    rows, gold, _ = scenario(r3_wrong=8)
    ids, dec, gold_rows, _ = rep1(rows, gold)
    errs = error_rows(dec["R3"], gold_rows, "unsupported")
    assert len(errs) == 8 and all(e["tier"] == 3 and e["pred"] == "b" and e["gold"] == "a" for e in errs)
    assert {"id", "lang", "text", "confidence"} <= set(errs[0])


def test_breakdown_counts_errors_by_scope_tier_and_answer():
    rows, gold, _ = scenario(r3_wrong=8)
    _, dec, gold_rows, _ = rep1(rows, gold)
    b = breakdown(error_rows(dec["R3"], gold_rows, "unsupported"))
    assert b == [{"scope": "in scope", "tier": 3, "answered": True, "n": 8}]


def test_discordant_pairs_say_which_system_is_right():
    rows, gold, _ = scenario(r3_wrong=8)
    _, dec, gold_rows, _ = rep1(rows, gold)
    d = discordant(dec["R1"], dec["R3"], gold_rows, "unsupported")
    # Both see the same "b" on the 8 items; R1 is wrong on the 4 compounds R3 sends to Tier 4.
    assert d["a_right_b_wrong"] == 0 and d["a_wrong_b_right"] == 4
