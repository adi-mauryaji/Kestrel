import pytest

from e0_summary import across_sessions, per_session


def row(session, phase, rnd, model, test, tok_s, wall=1.0, rss=100.0):
    return {"session": session, "phase": phase, "round": str(rnd), "model": model, "test": test,
            "avg_tok_s": str(tok_s), "wall_s": str(wall), "peak_rss_mib": str(rss)}


def test_per_session_uses_measured_rounds_only():
    rows = [row("s1", "warmup", 0, "m", "pp128", 999.0),
            row("s1", "measure", 1, "m", "pp128", 90.0),
            row("s1", "measure", 2, "m", "pp128", 100.0),
            row("s1", "measure", 3, "m", "pp128", 110.0)]
    s = per_session(rows)[("m", "pp128")]
    assert s["n"] == 3 and s["mean"] == pytest.approx(100.0)
    assert s["cv_pct"] == pytest.approx(10.0)  # sample SD 10 / mean 100


def test_startup_is_summarized_by_wall_time_and_rss_by_max():
    rows = [row("s1", "measure", r, "m", "startup", 0.0, wall=w, rss=rss)
            for r, w, rss in ((1, 1.0, 50.0), (2, 2.0, 70.0), (3, 3.0, 60.0))]
    s = per_session(rows)[("m", "startup")]
    assert s["mean"] == pytest.approx(2.0) and s["unit"] == "s"
    assert s["rss_max_mib"] == 70.0


def test_per_session_rejects_mixed_sessions():
    with pytest.raises(ValueError, match="one session"):
        per_session([row("s1", "measure", 1, "m", "pp128", 1.0), row("s2", "measure", 1, "m", "pp128", 1.0)])


def test_across_sessions_cv_of_session_means():
    a = {("m", "tg64"): {"mean": 9.0}}
    b = {("m", "tg64"): {"mean": 10.0}}
    c = {("m", "tg64"): {"mean": 11.0}}
    x = across_sessions([a, b, c])[("m", "tg64")]
    assert x["n_sessions"] == 3 and x["mean"] == pytest.approx(10.0)
    assert x["cv_pct"] == pytest.approx(10.0)
