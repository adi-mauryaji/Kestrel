import json

import pytest

from pilot_summary import main, read_folder, summarize, wilson


def counts(days, os="linux", days_used=None):
    used = sum(1 for d in days if "request" in d or "task_added" in d)
    return {"kestrel": "0.1.0", "os": os, "days_used": used if days_used is None else days_used,
            "days": [{"day": f"2026-11-{i + 1:02d}", "counts": d} for i, d in enumerate(days)],
            "capabilities": {"chat": "ready"}}


def write(folder, code, value):
    (folder / f"{code}.json").write_text(json.dumps(value) if isinstance(value, dict) else value)


def test_wilson_matches_what_the_plan_says():
    lo, hi = wilson(8, 20)
    assert (round(lo, 2), round(hi, 2)) == (0.22, 0.61)
    assert wilson(0, 10)[0] == 0.0
    assert wilson(10, 10)[1] == pytest.approx(1.0)


def test_each_person_is_added_up_and_days_per_week_come_from_the_weeks(tmp_path):
    write(tmp_path, "P01", counts([
        {"request": 3, "chat": 2, "not_understood": 1, "task_added": 1, "reminder_rang": 2},
        {"reminder_rang": 1},
        {"request": 1, "documents": 1, "document_added": 2, "brief_rang": 1},
    ]))
    people, bad = read_folder(tmp_path, weeks=2)
    assert bad == []
    p = people[0]
    assert (p["code"], p["os"], p["days_used"]) == ("P01", "linux", 2)
    assert p["days_per_week"] == 1.0
    assert (p["requests"], p["not_understood"]) == (4, 1)
    assert (p["tasks"], p["documents"], p["reminders"]) == (1, 2, 4)


def test_the_summary_says_how_many_on_each_system_and_what_was_used_most(tmp_path):
    write(tmp_path, "P01", counts([{"request": 4, "chat": 3, "not_understood": 1}], os="windows"))
    write(tmp_path, "P02", counts([{"request": 2, "chat": 1, "task_command": 1},
                                   {"request": 2, "task_command": 2}]))
    write(tmp_path, "P03", counts([{"reminder_rang": 1}], os="windows"))
    people, _ = read_folder(tmp_path, weeks=2)
    s = summarize(people)
    assert s["people"] == 3
    assert s["systems"] == {"windows": 2, "linux": 1}
    assert s["used_at_all"] == 2
    assert s["median_days_per_week"] == 0.5
    assert s["not_understood_share"] == pytest.approx(1 / 8)
    assert s["ways"][:2] == [("chat", 4), ("task_command", 3)]
    assert all(name not in dict(s["ways"]) for name in ("request", "reminder_rang", "not_understood"))


def test_a_file_that_cannot_be_read_is_named_and_the_run_says_it_failed(tmp_path, capsys):
    write(tmp_path, "P01", counts([{"request": 1}]))
    write(tmp_path, "P02", "this is not the counts text")
    write(tmp_path, "P03", {"days": "no"})
    assert main([str(tmp_path)]) == 1
    out = capsys.readouterr().out
    assert "P01" in out and "P02" in out and "P03" in out
    assert "could not be read" in out


def test_the_very_disappointed_share_comes_with_its_interval(tmp_path, capsys):
    write(tmp_path, "P01", counts([{"request": 1}]))
    assert main([str(tmp_path), "--very", "8", "--of", "20"]) == 0
    out = capsys.readouterr().out
    assert "8 of 20" in out and "40%" in out and "22%" in out and "61%" in out
    assert "bar" in out
    assert "1 person (" in out


@pytest.mark.parametrize("args", [["--very", "8"], ["--of", "20"], ["--very", "9", "--of", "8"],
                                  ["--very", "0", "--of", "0"]])
def test_a_share_needs_both_numbers_and_sense(tmp_path, args):
    write(tmp_path, "P01", counts([{"request": 1}]))
    with pytest.raises(SystemExit) as e:
        main([str(tmp_path), *args])
    assert e.value.code == 2


def test_an_empty_folder_is_not_a_result(tmp_path, capsys):
    assert main([str(tmp_path)]) == 1
    assert "no counts" in capsys.readouterr().out
