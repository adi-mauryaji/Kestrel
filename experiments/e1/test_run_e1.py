import pytest

from run_e1 import (CHUNKS, LATENCY_FRAC, REPS, chunk_plan, freq_problems, guard_test, item_latency, per_cpu_times, plan,
                    rep_disagreements, stop_reason, busy_outside, warmup_items, window_problems)


def test_plan_cuts_a_seeded_order_into_chunks_and_marks_the_latency_subset():
    ids = [f"test/{i}" for i in range(1000)]
    chunks = plan(ids, "clinc150")
    assert len(chunks) == CHUNKS["clinc150"] == 4
    flat = [it["id"] for c in chunks for it in c]
    assert sorted(flat) == sorted(ids) and len(set(flat)) == 1000
    assert flat != ids  # a seeded order, not the file order
    assert {len(c) for c in chunks} == {250}
    reps = [it["reps"] for c in chunks for it in c]
    assert reps.count(REPS) == round(LATENCY_FRAC["clinc150"] * 1000) == 200 and set(reps) == {1, REPS}
    assert plan(list(reversed(ids)), "clinc150") == chunks  # depends on the ids only


def test_intents_v0_runs_in_one_chunk_with_every_item_repeated():
    chunks = plan([f"t{i}" for i in range(252)], "intents_v0")
    assert len(chunks) == 1 and {it["reps"] for it in chunks[0]} == {REPS}


def test_chunk_plan_is_the_plan_or_a_limited_dry_run_in_one_chunk():
    rows = [{"id": f"v{i}"} for i in range(300)]
    used, chunks = chunk_plan(rows, "clinc150")
    assert used == rows and chunks == plan([r["id"] for r in rows], "clinc150")
    used, chunks = chunk_plan(rows, "clinc150", limit=30)
    assert len(used) == 30 and len(chunks) == 1
    assert sorted(it["id"] for it in chunks[0]) == sorted(r["id"] for r in used)
    assert chunk_plan(list(reversed(rows)), "clinc150", limit=30)[1] == chunks


def test_warmup_items_are_fixed_train_items():
    rows = [{"id": f"tr{i}", "text": f"t {i}"} for i in range(100)]
    w = warmup_items(rows)
    assert len(w) == 5 and w == warmup_items(list(reversed(rows)))
    assert all(it["warmup"] and it["reps"] == 1 for it in w)
    assert [it["id"] for it in w] != [f"tr{i}" for i in range(5)]


STAT_A = "cpu  10 0 10 80 0 0 0 0 0 0\ncpu0 5 0 5 40 0 0 0 0 0 0\ncpu1 1 0 1 48 0 0 0 0 0 0\nintr 1 2\n"
STAT_B = "cpu  40 0 20 90 0 0 0 0 0 0\ncpu0 30 0 15 40 0 0 0 0 0 0\ncpu1 2 0 2 56 0 1 0 0 0 0\nintr 1 2\n"


def test_busy_outside_the_mask_comes_from_the_per_cpu_lines():
    a, b = per_cpu_times(STAT_A), per_cpu_times(STAT_B)
    assert set(a) == {0, 1} and a[0] == (10, 50)  # (busy, total); idle and iowait are not busy
    # cpu1: busy 2 -> 5 (irq counts), total 50 -> 61: 3 of 11 jiffies busy.
    assert busy_outside(a, b, [1]) == pytest.approx(100 * 3 / 11)
    assert busy_outside(a, a, [1]) == 0.0


def test_a_chunk_stops_on_ac_loss_or_boost_unless_allowed():
    ok = {"ac_online": True, "boost": "0"}
    assert stop_reason(ok, set()) is None
    assert "AC" in stop_reason({**ok, "ac_online": False}, set())
    assert "boost" in stop_reason({**ok, "boost": "1"}, set())
    assert stop_reason({"ac_online": False, "boost": "1"}, {"battery", "boost"}) is None


def test_background_load_is_judged_over_one_minute_windows():
    s = [{"t": 10 * i, "busy_outside_pct": 0.0, "temp_c": 60.0} for i in range(12)]
    s[3]["busy_outside_pct"] = 30.0  # one spike: the window mean stays at 5%
    assert window_problems(s) == []
    for i in range(4, 10):
        s[i]["busy_outside_pct"] = 15.0
    assert any("background" in p for p in window_problems(s))
    hot = [dict(x, temp_c=96.0) if i == 5 else dict(x, busy_outside_pct=0.0) for i, x in enumerate(s)]
    assert [p for p in window_problems(hot) if "°C" in p]


def test_a_chunk_whose_frequency_strays_from_the_median_is_invalid():
    assert freq_problems({"1": 2100.0, "2": 2095.0, "3": 2105.0}) == {}
    bad = freq_problems({"1": 2100.0, "2": 2095.0, "3": 1700.0, "4": 2102.0})
    assert list(bad) == ["3"]


def _row(i, system, rep, intent, tier=4, ms=100.0, warmup=False):
    return {"id": i, "system": system, "rep": rep, "intent": intent, "tier": tier, "ms": ms, "warmup": warmup}


def test_repetitions_must_decide_the_same_way():
    rows = [_row("a", "R0", 1, "x"), _row("a", "R0", 2, "x"), _row("b", "R0", 1, "x"), _row("b", "R0", 2, None),
            _row("w", "R0", 1, "x", warmup=True), _row("w", "R0", 2, "y", warmup=True)]
    assert rep_disagreements(rows) == [("b", "R0")]  # warm-ups are not judged


def test_the_latency_unit_is_the_per_item_median_over_repetitions():
    rows = [_row("a", "R3", r, "x", ms=ms) for r, ms in enumerate([5.0, 7.0, 6.0, 100.0, 6.5], 1)]
    rows += [_row("a", "R0", 1, "x", ms=900.0), _row("w", "R3", 1, "x", ms=50.0, warmup=True)]
    assert item_latency(rows) == {("a", "R3"): 6.5, ("a", "R0"): 900.0}


class _Git:
    def __init__(self, tag=True, ancestor=True, clean=True):
        self.tag, self.ancestor, self.clean = tag, ancestor, clean

    def __call__(self, *args):
        if args[0] == "tag":
            return 0, "exp-E1-frozen\n" if self.tag else ""
        if args[0] == "merge-base":
            return (0 if self.ancestor else 1), ""
        if args[0] == "status":
            return 0, "" if self.clean else " M experiments/PREREG.md\n"
        raise AssertionError(args)


def test_the_test_split_needs_the_freeze_tag_frozen_configs_and_a_clean_tree():
    frozen = [{"status": "FROZEN at exp-E1-frozen"}]
    guard_test(frozen, git=_Git())
    for status in ("DEV: chosen on val", "CANDIDATE: chosen on val at tolerance 0"):
        with pytest.raises(RuntimeError, match="not FROZEN"):
            guard_test([{"status": status}], git=_Git())
    for git, msg in ((_Git(tag=False), "tag"), (_Git(ancestor=False), "ancestor"), (_Git(clean=False), "clean")):
        with pytest.raises(RuntimeError, match=msg):
            guard_test(frozen, git=git)
