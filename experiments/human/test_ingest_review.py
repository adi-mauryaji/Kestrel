import json

import pytest

from ingest_review import ingest, merge, parse, status
from review_data import unit

U = [unit("tasks", "family", "a", "A", [{"k": "en", "v": "make a list"}]),
     unit("tasks", "family", "b", "B", [{"k": "en", "v": "remind me"}]),
     unit("words", "alias", "SAY", "SAY", [{"k": "Words", "v": "bolun"}])]


def rec(u, decision="ok", note="", at="2026-10-24T10:00:00Z", h=None):
    return {"kind": "kestrel-review", "version": 1, "unit": u["id"], "hash": h or u["hash"], "decision": decision,
            "note": note, "at": at}


def review_set(*recs):
    return {"kind": "kestrel-review-set", "version": 1, "reviews": list(recs)}


def test_parse_reads_store_documents_and_copied_sets():
    assert [r["unit"] for r in parse(json.dumps(rec(U[0])))] == ["tasks:family:a"]  # an export: the bare body
    assert [r["unit"] for r in parse(json.dumps({"id": "tasks:family:a", "data": rec(U[0])}))] == ["tasks:family:a"]
    pasted = "Here is my review:\n" + json.dumps(review_set(rec(U[1], "fix", "the plan is wrong"))) + "\nthanks"
    assert parse(pasted)[0]["decision"] == "fix"
    with pytest.raises(ValueError):
        parse(json.dumps({"kind": "kestrel-annotation", "version": 1}))


def test_unknown_units_and_decisions_are_refused():
    with pytest.raises(ValueError, match="tasks:family:zz"):
        merge([rec({"id": "tasks:family:zz", "hash": "0" * 16})], U)
    with pytest.raises(ValueError, match="maybe"):
        merge([rec(U[0], "maybe")], U)


def test_the_latest_decision_per_unit_wins_in_any_order():
    early, late = rec(U[0], "fix", "x", at="2026-10-24T09:00:00Z"), rec(U[0], "ok", at="2026-10-24T10:00:00Z")
    assert merge([early, late], U)[U[0]["id"]]["decision"] == "ok"
    assert merge([late, early], U)[U[0]["id"]]["decision"] == "ok"


def test_status_counts_and_readiness():
    s = status(merge([rec(U[0]), rec(U[1], "fix", "wrong title")], U), U)
    assert s["tasks"] == {"units": 2, "ok": 1, "fix": 1, "stale": 0, "missing": 0, "ready": False}
    assert s["words"]["missing"] == 1 and not s["words"]["ready"]
    assert status(merge([rec(U[0]), rec(U[1])], U), U)["tasks"]["ready"]


def test_a_decision_on_changed_content_is_stale():
    s = status(merge([rec(U[0], h="0" * 16), rec(U[1])], U), U)
    assert s["tasks"]["stale"] == 1 and s["tasks"]["ok"] == 1 and not s["tasks"]["ready"]


def test_ingest_adds_to_the_log_and_lists_what_to_fix(tmp_path):
    inbox = tmp_path / "in"
    inbox.mkdir()
    (inbox / "a.json").write_text(json.dumps({"id": "tasks:family:a", "data": rec(U[0])}))
    (inbox / "b.txt").write_text(json.dumps(review_set(rec(U[1], "fix", "keep 'kal' in the title"))))
    log = tmp_path / "review.json"
    out = ingest([inbox], U, log)
    assert out["status"]["tasks"]["fix"] == 1
    assert json.loads(log.read_text())["fixes"] == [{"unit": "tasks:family:b", "note": "keep 'kal' in the title"}]
    # A later decision replaces the earlier one; decisions already in the log are kept.
    later = tmp_path / "later"
    later.mkdir()
    (later / "c.json").write_text(json.dumps(review_set(rec(U[1], at="2026-10-25T08:00:00Z"), rec(U[2]))))
    out = ingest([later], U, log)
    assert out["fixes"] == [] and out["status"]["tasks"]["ready"] and out["status"]["words"]["ready"]
