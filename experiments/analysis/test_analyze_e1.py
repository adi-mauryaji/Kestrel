import json

import numpy as np
import pytest

from analyze_e1 import (analyze, check_items, check_systems, chunk_problems, item_correct, nearest_rank, record_invalid,
                        render, select_runs, untuned)
from kestrel_stats import paired_bootstrap_ci

SYS = {s: {"config_sha256": f"{s}-sha", "status": "FROZEN 2026-11-01"} for s in ("R0", "R1", "R3")}
JITTER = (0.0, 10.0, -10.0, 5.0, -5.0)  # repetitions 1..5: the per-item median is the base time


def write_run(run_dir, name, chunk, chunks, valid=True, mhz=2100.0, items=(), systems=SYS):
    d = run_dir / name
    d.mkdir(parents=True)
    manifest = {"chunk": chunk, "chunks": chunks, "valid": valid, "problems": [] if valid else ["rule 2"],
                "item_ids": list(items), "sampling": {"mhz_pinned_mean": mhz}, "systems": systems,
                "rep_disagreements": []}
    (d / "manifest.json").write_text(json.dumps(manifest))
    return d


def test_one_usable_run_per_chunk(tmp_path):
    a = write_run(tmp_path, "chunk1of2_A", 1, 2)
    write_run(tmp_path, "chunk2of2_B", 2, 2, valid=False)  # invalid, rerun as C
    c = write_run(tmp_path, "chunk2of2_C", 2, 2)
    assert select_runs(tmp_path) == [a, c]
    write_run(tmp_path, "chunk2of2_D", 2, 2)  # a second valid run of chunk 2: which one counts?
    with pytest.raises(ValueError, match="chunk 2"):
        select_runs(tmp_path)


def test_a_missing_chunk_is_refused(tmp_path):
    write_run(tmp_path, "chunk1of2_A", 1, 2)
    with pytest.raises(ValueError, match="chunk 2"):
        select_runs(tmp_path)


def test_recorded_invalid_runs_are_skipped(tmp_path):
    a = write_run(tmp_path, "chunk1of2_A", 1, 2)
    write_run(tmp_path, "chunk2of2_C", 2, 2)
    d = write_run(tmp_path, "chunk2of2_D", 2, 2)
    record_invalid(tmp_path, {"chunk2of2_C": "rule 3"})
    record_invalid(tmp_path, {"chunk1of2_Z": "kept"})
    assert json.loads((tmp_path / "INVALIDATED.json").read_text()) == {"chunk2of2_C": "rule 3", "chunk1of2_Z": "kept"}
    assert select_runs(tmp_path) == [a, d]


def test_dry_runs_count_only_in_dev_mode(tmp_path):
    dry = write_run(tmp_path, "chunk1of1_A", 1, 1, valid=False)
    assert select_runs(tmp_path, dev=True) == [dry]
    with pytest.raises(ValueError):
        select_runs(tmp_path)


def test_rule_3_compares_the_chunks_frequencies():
    ms = [{"sampling": {"mhz_pinned_mean": f}} for f in (2100.0, 2095.0, 1700.0, 2102.0)]
    bad = chunk_problems(ms, ["c1", "c2", "c3", "c4"])
    assert list(bad) == ["c3"] and "rule 3" in bad["c3"]
    assert chunk_problems(ms[:1], ["c1"]) == {}


def test_chunk_items_must_be_the_plan():
    plan = [[{"id": "a", "reps": 5}, {"id": "b", "reps": 1}], [{"id": "c", "reps": 1}]]
    ms = [{"chunk": 1, "item_ids": ["a", "b"]}, {"chunk": 2, "item_ids": ["c"]}]
    check_items(ms, plan)
    with pytest.raises(ValueError, match="chunk 2"):
        check_items([ms[0], {"chunk": 2, "item_ids": ["d"]}], plan)
    with pytest.raises(ValueError):
        check_items(ms[:1], plan)


def test_systems_must_agree_across_chunks_and_be_frozen_on_test():
    other = {**SYS, "R3": {"config_sha256": "changed", "status": "FROZEN 2026-11-01"}}
    assert check_systems([{"systems": SYS}, {"systems": SYS}], frozen=True) == SYS
    with pytest.raises(ValueError, match="R3"):
        check_systems([{"systems": SYS}, {"systems": other}], frozen=False)
    cand = {s: {**v, "status": "CANDIDATE"} for s, v in SYS.items()}
    check_systems([{"systems": cand}], frozen=False)
    with pytest.raises(ValueError, match="FROZEN"):
        check_systems([{"systems": cand}], frozen=True)


def test_scoring_rules():
    oos = "unsupported"
    row = lambda intent, tier=3: {"intent": intent, "tier": tier}  # noqa: E731
    g = lambda label: {"label": label}  # noqa: E731
    # In scope: only the gold label is right; an abstention is wrong.
    assert item_correct(row("a"), g("a"), oos) and not item_correct(row(None), g("a"), oos)
    assert not item_correct(row("b"), g("a"), oos)
    # Out of scope: abstaining or saying "unsupported" is right.
    assert item_correct(row(None), g(oos), oos) and item_correct(row(oos), g(oos), oos)
    assert not item_correct(row("a"), g(oos), oos)
    # Compound (ADR-010): right iff not accepted as a single intent below Tier 4.
    assert not item_correct(row("a", 3), g("a|b"), oos)
    assert item_correct(row("a", 4), g("a|b"), oos) and item_correct(row(None, 3), g("a|b"), oos)


def test_untuned_r0_answers_with_its_best_guess():
    r = {"intent": None, "tier": 4, "best": {"intent": "x", "confidence": 0.3, "tier": 4}, "confidence": None}
    assert untuned(r)["intent"] == "x" and r["intent"] is None
    assert untuned({"intent": None, "tier": 4})["intent"] is None
    assert untuned({"intent": "y", "tier": 4, "best": None})["intent"] == "y"


def test_nearest_rank_matches_kestrel_bench():
    s = np.arange(1, 101, dtype=float)
    assert (nearest_rank(s, 50), nearest_rank(s, 95), nearest_rank(s, 100)) == (50.0, 95.0, 100.0)
    assert nearest_rank(np.array([7.0]), 95) == 7.0
    assert nearest_rank(np.arange(1, 21, dtype=float)[::-1], 95) == 19.0  # rank ceil(0.95 * 20) = 19


def rows_for(system, i, intent, tier, base_ms, steps, reps=5, best=None, multi=False):
    """`steps`: (tier, label, ms) per consulted tier, as kestrel bench-items logs them."""
    steps = [{"tier": t, "label": lab, "confidence": None if lab is None else 0.9, "ms": ms} for t, lab, ms in steps]
    return [{"system": system, "id": i, "intent": intent, "tier": tier, "ms": base_ms + JITTER[r - 1], "rep": r,
             "confidence": 0.9 if intent else None, "consulted": [st["tier"] for st in steps], "steps": steps,
             "best": best, "multi_intent": multi, "warmup": False} for r in range(1, reps + 1)]


def scenario(n=40, r3_wrong=0, latency_every=1):
    """Intents-like items: k % 10 == 0 out of scope (hi-en), k % 10 == 5 compound (en), else "a"; even k hi-en.
    Tier 4 says "a" everywhere; Tier 3 says "a", or "b" on the first `r3_wrong` in-scope items R3 answers
    there, and R1 and R3 see the same Tier 3. R0: Tier 4 at 800 ms, abstains on out-of-scope (best guess
    "a"). R1: Tier 3 at 5 ms, accepts compounds. R3: Tier 3 at 6 ms; out-of-scope escalates and abstains at
    806 ms; compounds (and item 1, a signal false positive) go straight to Tier 4 at 800 ms."""
    gold, rows, lat_ids, wrong = {}, [], set(), 0
    for k in range(n):
        i = f"it{k:02d}"
        lang = "hi-en" if k % 2 == 0 else "en"
        label = "unsupported" if k % 10 == 0 else "a|b" if k % 10 == 5 else "a"
        gold[i] = {"id": i, "label": label, "lang": lang, "family": f"fam{k // 5}"}  # 8 families of 5
        timed = k % latency_every == 0
        reps = 5 if timed else 1
        if timed:
            lat_ids.add(i)
        oos, comp = label == "unsupported", label == "a|b"
        bad = not (oos or comp or k == 1) and wrong < r3_wrong
        wrong += bad
        t3 = "b" if bad else "a"
        rows += rows_for("R0", i, None if oos else "a", 4, 800, [(4, "a", 800)], reps,
                         best={"intent": "a"} if oos else None)
        rows += rows_for("R1", i, None if oos else t3, 3, 5, [(3, t3, 5)], reps)
        if comp or k == 1:
            rows += rows_for("R3", i, "a", 4, 800, [(4, "a", 800)], reps, multi=True)
        elif oos:
            rows += rows_for("R3", i, None, 4, 806, [(1, None, 0.0), (3, "a", 6), (4, "a", 800)], reps)
        else:
            rows += rows_for("R3", i, t3, 3, 6, [(1, None, 0.0), (3, t3, 6)], reps)
    if latency_every > 1:  # untimed items: one wild repetition that must not enter any latency number
        for r in rows:
            if r["id"] not in lat_ids:
                r["ms"] = 99999.0
    return rows, gold, lat_ids


def run(rows, gold, lat_ids, **kw):
    return analyze(rows, gold, lat_ids, dataset="intents_v0", delta=0.0263, n_boot=500, **kw)


def test_table_r1_per_system():
    rows, gold, lat = scenario()
    rep = run(rows, gold, lat)
    s = rep["systems"]
    # 40 items: 4 out of scope, 4 compound, 32 in scope.
    assert s["R0"]["accuracy"] == 1.0 and s["R0 untuned"]["accuracy"] == pytest.approx(36 / 40)
    assert s["R1"]["accuracy"] == pytest.approx(36 / 40) and s["R3"]["accuracy"] == 1.0
    assert s["R0"]["coverage"] == pytest.approx(36 / 40) and s["R0 untuned"]["coverage"] == 1.0
    assert s["R1"]["compound_accuracy"] == 0.0 and s["R3"]["compound_accuracy"] == 1.0
    assert s["R0"]["tier4_share"] == 1.0 and s["R1"]["tier4_share"] == 0.0
    assert s["R3"]["tier4_share"] == pytest.approx(9 / 40)
    # R3: 31 items at 6 ms, 4 at 806, 5 at 800 (per-item medians over the jittered repetitions).
    assert s["R3"]["latency_ms"]["mean"] == pytest.approx((31 * 6 + 4 * 806 + 5 * 800) / 40)
    assert s["R3"]["latency_ms"]["median"] == 6.0 and s["R3"]["latency_ms"]["p95"] == 806.0
    assert s["R0"]["latency_ms"]["n"] == 40
    assert s["R3"]["macro_f1"] == 1.0


def test_h1_is_the_intersection_union_of_noninferiority_and_speed():
    rows, gold, lat = scenario()
    h1 = run(rows, gold, lat)["h1"]
    assert h1["accuracy"]["diff"] == 0.0 and h1["accuracy"]["noninferior"]
    assert h1["latency"]["diff_ms"] == pytest.approx((31 * 6 + 4 * 806 + 5 * 800) / 40 - 800)
    assert h1["latency"]["faster"] and h1["supported"] and h1["outcome"] == "supported"
    assert h1["p_iut"] == max(h1["accuracy"]["p_noninferior"], h1["latency"]["p_below"])

    rows, gold, lat = scenario(r3_wrong=32)  # R3 wrong on every in-scope item it answers at Tier 3
    h1 = run(rows, gold, lat)["h1"]
    assert h1["accuracy"]["ci95"][1] < -0.0263 and h1["latency"]["faster"]
    assert h1["outcome"] == "not supported" and not h1["supported"]


def ok_vector(rows, gold, system):
    return [item_correct(r, gold[r["id"]], "unsupported")
            for r in sorted(rows, key=lambda r: r["id"]) if r["system"] == system and r["rep"] == 1]


def test_intents_v0_resamples_template_families_with_items_alongside():
    rows, gold, lat = scenario(r3_wrong=8)
    rep = run(rows, gold, lat)
    acc = rep["h1"]["accuracy"]
    assert rep["clusters"] == "family" and acc["diff"] == pytest.approx(-8 / 40)
    a, b = ok_vector(rows, gold, "R3"), ok_vector(rows, gold, "R0")
    fam = [gold[i]["family"] for i in sorted(gold)]
    _, lo, hi = paired_bootstrap_ci(a, b, n_boot=500, seed=0, clusters=fam)
    assert acc["ci95"] == [pytest.approx(lo), pytest.approx(hi)]
    _, lo, hi = paired_bootstrap_ci(a, b, n_boot=500, seed=0)
    assert acc["item_level"]["ci95"] == [pytest.approx(lo), pytest.approx(hi)]
    # The 8 errors sit in 3 of the 8 families: resampling families widens the interval.
    assert np.diff(acc["ci95"])[0] > np.diff(acc["item_level"]["ci95"])[0]
    assert "item_level" in rep["h1"]["latency"] and "item_level" in rep["by_lang"]["hi-en"]["accuracy"]
    assert rep["h1c"]["accuracy"]["diff"] == pytest.approx(32 / 40 - 28 / 40)  # R3 - R1 (both see the 8 "b")
    assert rep["h1c"]["p"] == rep["h1c"]["accuracy"]["p_noninferior"]


def test_clinc150_resamples_items():
    rows, gold, lat = scenario()
    for g in gold.values():
        g["label"] = "oos" if g["label"] == "unsupported" else g["label"]
        del g["family"]
    rep = analyze(rows, gold, lat, dataset="clinc150", delta=0.0263, n_boot=500)
    assert rep["clusters"] is None and "item_level" not in rep["h1"]["accuracy"]
    assert rep["systems"]["R0"]["accuracy"] == 1.0


def test_latency_uses_only_the_latency_subset():
    rows, gold, lat = scenario(latency_every=2)
    rep = run(rows, gold, lat)
    assert rep["n_items"] == 40 and rep["n_latency_items"] == 20
    assert rep["systems"]["R0"]["latency_ms"]["mean"] == 800.0  # the 99999 ms single runs are not in it
    assert rep["systems"]["R3"]["accuracy"] == 1.0  # accuracy still uses every item


def test_the_first_repetition_is_scored_and_disagreements_are_reported():
    rows, gold, lat = scenario()
    for r in rows:
        if r["system"] == "R3" and r["id"] == "it02" and r["rep"] == 3:
            r["intent"] = "b"
    rep = run(rows, gold, lat)
    assert rep["systems"]["R3"]["accuracy"] == 1.0
    assert rep["rep_disagreements"] == [["it02", "R3"]]


def test_per_language_results_d11_rho_and_the_signal():
    rows, gold, lat = scenario()
    rep = run(rows, gold, lat)
    en, hi = rep["by_lang"]["en"], rep["by_lang"]["hi-en"]
    assert en["n"] == hi["n"] == 20
    assert en["R3_tier4_share"] == pytest.approx(5 / 20) and hi["R3_tier4_share"] == pytest.approx(4 / 20)
    assert hi["accuracy"]["noninferior"] and rep["h7a"] == hi
    assert hi["R3_mean_ms"] == pytest.approx((16 * 6 + 4 * 806) / 20)
    assert rep["signal_false_positive"] == {"all": pytest.approx(1 / 36), "en": pytest.approx(1 / 16),
                                            "hi-en": 0.0}
    assert rep["rho"]["r3_median_below_tier4_ms"] == 6.0 and rep["rho"]["r0_median_ms"] == 800.0
    assert rep["rho"]["met"]


def test_d1_sensitivity_drops_duplicate_texts():
    rows, gold, lat = scenario()
    rep = run(rows, gold, lat, dup_ids={"it01", "it02"})
    assert rep["sensitivity_d1"]["n_items"] == 38
    assert rep["sensitivity_d1"]["h1"]["accuracy"]["diff"] == 0.0
    assert "sensitivity_d1" not in run(rows, gold, lat)


def test_every_item_needs_a_decision_from_every_system():
    rows, gold, lat = scenario()
    rows = [r for r in rows if not (r["system"] == "R1" and r["id"] == "it03")]
    with pytest.raises(ValueError, match="it03"):
        run(rows, gold, lat)


def test_render_names_the_systems_and_h1():
    rows, gold, lat = scenario()
    text = render(run(rows, gold, lat) | {"dataset": "intents_v0", "split": "val", "dev": True, "valid": False})
    assert "R0 untuned" in text and "H1" in text and "DEV" in text


def test_h1b_random_escalation_at_r3s_acceptance_counts():
    rows, gold, lat = scenario()
    h = run(rows, gold, lat)["h1b"]
    # Tier 3 accepts 31 of the 35 items reaching it at random (X of the 4 out-of-scope ones, E[X] = 124/35, all
    # wrong); the 9 left go to Tier 4, which accepts 5 at random: E[wrong] = E[X] * 8/9 + 24/9.
    assert h["r3_acc"] == 1.0
    assert h["rrand"]["acc"] == pytest.approx(1 - (124 / 35 * 8 / 9 + 24 / 9) / 40, abs=0.01)
    assert h["accuracy"]["diff"] == pytest.approx(1 - h["rrand"]["acc"])
    for part in ("all_items", "latency_subset"):  # every item is timed here, and every Tier 4 call costs 800 ms
        assert h["cost_model_ms"][part]["r3"] == pytest.approx(h["cost_model_ms"][part]["rrand"])
    assert h["p"] == h["accuracy"]["p_above"] and "item_level" in h["accuracy"]


def test_h1b_needs_tier_answers_that_reproduce_r3():
    rows, gold, lat = scenario()
    for r in rows:
        if r["system"] == "R1" and r["id"] == "it02":
            r["steps"][0]["label"] = "b"  # R3 accepted "a" from the same Tier 3
    with pytest.raises(ValueError, match="it02"):
        run(rows, gold, lat)
