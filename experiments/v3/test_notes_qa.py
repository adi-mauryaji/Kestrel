from notes_qa import grade, has_facts, outcome


def test_facts_are_found_in_any_of_their_ways_and_case():
    assert has_facts("Your rent is 12,000.", [["12000", "12,000"]])
    assert has_facts("Squats on Monday, Wednesday and Friday.", [["monday"], ["wednesday"], ["friday"]])
    assert not has_facts("Squats on Monday and Friday.", [["monday"], ["wednesday"], ["friday"]])


def test_the_outcome_is_what_the_user_would_see():
    assert outcome("Rent 12000 hai.", None, gated=False) == "answered"
    assert outcome("Rent 12000 hai.", None, gated=True) == "not in notes"
    assert outcome("NOT IN NOTES", None, gated=False) == "not in notes"
    assert outcome("Rent 15000.", "15000 is not in the notes", gated=False) == "withheld"


def test_grades_reward_the_right_answer_and_the_honest_no():
    answerable = {"notes": ["a.md"], "facts": [["12000"]]}
    unanswerable = {"notes": [], "facts": []}
    assert grade(answerable, "answered", "Rent 12000.") == "right"
    assert grade(answerable, "answered", "Rent 15000.") == "wrong"
    assert grade(answerable, "not in notes", "") == "abstained"
    assert grade(answerable, "withheld", "x") == "abstained"
    assert grade(unanswerable, "not in notes", "") == "right"
    assert grade(unanswerable, "withheld", "x") == "right"
    assert grade(unanswerable, "answered", "Your passport expires in 2030.") == "wrong"


def test_each_split_has_its_own_log():
    from notes_qa import LOG, log_path
    assert log_path("dev") == LOG
    assert log_path("test") == LOG.with_name("qa_test.json")
