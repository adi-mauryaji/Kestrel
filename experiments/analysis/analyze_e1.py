"""experiments/analysis/analyze_e1.py -- E1 analysis (PREREG §3, §6; Ch. 38): Table R1, H1, H1c, H7a.

    python experiments/analysis/analyze_e1.py --dataset clinc150 --split test       # after every chunk ran
    python experiments/analysis/analyze_e1.py --dataset intents_v0 --limit 20 --dev   # the dev dry runs

Runs: experiments/logs/E1/<dataset>_<split>[_n<limit>]/. Every chunk of the plan needs exactly one
usable run: valid in its manifest and not listed in that directory's INVALIDATED.json. Only an
invalid chunk is rerun, so two usable runs of one chunk are refused, never chosen between. Rule 3
(PREREG §4) compares the chunks' mean pinned-CPU frequencies with each other, so it is applied here;
`--record-invalid` writes the runs it flags into INVALIDATED.json, to be rerun. `--dev` reads dry runs
(never valid) and labels the output DEV. The chunks' items must be run_e1's plan, and every chunk must
have run the same three configs (FROZEN on test).

Scoring (PREREG §3, as policies.correct): the first timed repetition's decision; an abstention is wrong
in scope and right out of scope; a compound item is right iff not accepted below Tier 4 (ADR-010).
Macro-F1 is over single-intent items (compound items are scored apart, as in the dev scripts).
Latency unit: the per-item median over its repetitions, latency subset only (ADR-020); accuracy
uses every item.

Bootstrap (ADR-029): 10 000 paired resamples, seed 0, of the items on CLINC150 and of the template
families on Kestrel-Intents-v0 (its items are not independent), with the item-level CI and p
alongside there.

Reported: Table R1 (R0 tuned and untuned, R1, R3; RSS comes from the per-config runs, not from here),
H1 (intersection-union: the 95% CI of Δacc = R3 - R0 lies above -δ AND that of Δlat lies below 0;
its outcome supported / not supported / inconclusive), H1c (R3 vs R1, non-inferiority), per language
H7a (Δacc, Δlat) and D11 (R3's Tier-4 share and mean latency), ρ, the routing signal's
false-positive rate on single-intent items, and on CLINC150 test the D1 sensitivity analysis (items
whose text also occurs in another set dropped). Secondary hypotheses carry their p (two-sided scale);
their outcomes come after Holm over the whole secondary family, once every experiment has its p.
H1b (R-rand) needs every tier's output on every item and is not computed here yet.

Out: experiments/logs/E1/analysis_<run name>.json and the tables on stdout.
"""
from __future__ import annotations

import argparse
import json
import math
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "e1"))
from run_e1 import LOG_DIR, REPS, chunk_plan, freq_problems, item_latency, load_items, rep_disagreements  # noqa: E402
from kestrel_stats import (N_BOOT, bootstrap_means, cohens_dz, combine, intersection_union_p,  # noqa: E402
                           mcnemar_exact, outcome, p_below, p_noninferior, percentile_ci, wilcoxon_paired)
from metrics import macro_f1, per_item_correct  # noqa: E402

SYSTEMS = ("R0", "R1", "R3")
REPORTED = ("R0", "R0 untuned", "R1", "R3")
OOS = {"intents_v0": "unsupported", "clinc150": "oos"}
CLUSTERS = {"intents_v0": "family"}  # ADR-029: the bootstrap resamples this field's groups
RHO = 0.1  # D6
INVALIDATED = "INVALIDATED.json"
DELTA_LOG = ROOT / "experiments" / "logs" / "router" / "delta_pilot.json"


def _invalidated(run_dir: Path) -> dict[str, str]:
    path = run_dir / INVALIDATED
    return json.loads(path.read_text()) if path.exists() else {}


def record_invalid(run_dir: Path, problems: dict[str, str]) -> None:
    """Add runs (directory name -> reason) to the run directory's INVALIDATED.json."""
    merged = _invalidated(run_dir) | problems
    (run_dir / INVALIDATED).write_text(json.dumps(merged, indent=1, ensure_ascii=False) + "\n")


def select_runs(run_dir: Path, *, dev: bool = False) -> list[Path]:
    """The one usable run of every chunk, in chunk order (with dev, dry runs are usable too)."""
    skip = _invalidated(run_dir)
    usable, totals = defaultdict(list), set()
    for path in sorted(run_dir.glob("chunk*/manifest.json")):
        m = json.loads(path.read_text())
        totals.add(m["chunks"])
        if path.parent.name not in skip and (dev or m["valid"]):
            usable[m["chunk"]].append(path.parent)
    if len(totals) != 1:
        raise ValueError(f"{run_dir}: no runs, or runs that disagree on the number of chunks {sorted(totals)}")
    n = totals.pop()
    for k in range(1, n + 1):
        if len(usable[k]) != 1:
            raise ValueError(f"chunk {k} of {n}: {len(usable[k])} usable runs {[p.name for p in usable[k]]}, "
                             "need exactly one")
    return [usable[k][0] for k in range(1, n + 1)]


def chunk_problems(manifests: list[dict], names: list[str]) -> dict[str, str]:
    """Rule 3 across the chunks: run name -> problem. A chunk without frequency samples can't be judged."""
    mhz = {n: m["sampling"]["mhz_pinned_mean"] for n, m in zip(names, manifests)}
    blind = {n: "no pinned-CPU frequency samples (rule 3 cannot be checked)" for n, f in mhz.items() if f is None}
    return blind | freq_problems({n: f for n, f in mhz.items() if f is not None})


def check_items(manifests: list[dict], plan: list[list[dict]]) -> None:
    """The chunks' items are the plan's, chunk by chunk, in the plan's order."""
    if sorted(m["chunk"] for m in manifests) != list(range(1, len(plan) + 1)):
        raise ValueError(f"chunks {sorted(m['chunk'] for m in manifests)}, the plan has {len(plan)}")
    for m in manifests:
        if m["item_ids"] != [it["id"] for it in plan[m["chunk"] - 1]]:
            raise ValueError(f"chunk {m['chunk']}: its items are not the plan's")


def check_systems(manifests: list[dict], *, frozen: bool) -> dict:
    """The systems every chunk ran: the same three configs throughout (FROZEN ones on test)."""
    first = manifests[0]["systems"]
    if set(first) != set(SYSTEMS):
        raise ValueError(f"systems {sorted(first)}, expected {list(SYSTEMS)}")
    for m in manifests[1:]:
        for s in SYSTEMS:
            if m["systems"][s]["config_sha256"] != first[s]["config_sha256"]:
                raise ValueError(f"{s}: the chunks ran different configs")
    if frozen and (bad := [s for s in SYSTEMS if not first[s]["status"].startswith("FROZEN")]):
        raise ValueError(f"configs not FROZEN: {bad}")
    return first


def item_correct(row: dict, gold_row: dict, oos_label: str) -> bool:
    if "|" in gold_row["label"]:  # compound (ADR-010)
        return not (row["intent"] is not None and row["tier"] < 4)
    return per_item_correct([gold_row["label"]], [row["intent"]], oos_label)[0]


def untuned(row: dict) -> dict:
    """R0 without its abstention threshold (ADR-019): an abstention becomes Tier 4's best answer."""
    best = row.get("best")
    if row["intent"] is None and best:
        return row | {"intent": best["intent"], "confidence": best.get("confidence")}
    return row


def nearest_rank(x, p: int) -> float:
    """kestrel-bench's percentile: the smallest sample with at least p% of the samples <= it."""
    s = np.sort(np.asarray(x, float))
    return float(s[max(-(-p * s.size // 100), 1) - 1])


def _boot(d, n_boot: int, seed: int, clusters=None):
    boots = bootstrap_means(d, n_boot, seed, clusters)
    lo, hi = percentile_ci(boots)
    return float(np.mean(d)), lo, hi, boots


def noninferiority(ok_a, ok_b, delta: float, n_boot: int, seed: int, clusters=None) -> dict:
    """Accuracy of A vs B: the 95% paired-bootstrap CI of mean(correct_A - correct_B) against -delta;
    with clusters, resampled by cluster and the item-level CI and p alongside."""
    d = ok_a.astype(float) - ok_b
    m, lo, hi, boots = _boot(d, n_boot, seed, clusters)
    n01, n10, p = mcnemar_exact(ok_a, ok_b)
    out = {"diff": m, "ci95": [lo, hi], "margin": delta, "noninferior": lo > -delta,
           "p_noninferior": p_noninferior(boots, delta), "mcnemar": {"n01": n01, "n10": n10, "p": p}}
    if clusters is not None:
        _, lo, hi, boots = _boot(d, n_boot, seed)
        out["item_level"] = {"ci95": [lo, hi], "p_noninferior": p_noninferior(boots, delta)}
    return out


def speed(ms_a, ms_b, n_boot: int, seed: int, clusters=None) -> dict:
    """Latency of A vs B: A is faster iff the 95% CI of mean(lat_A - lat_B) lies below 0."""
    d = ms_a - ms_b
    m, lo, hi, boots = _boot(d, n_boot, seed, clusters)
    out = {"diff_ms": m, "ci95": [lo, hi], "faster": hi < 0, "p_below": p_below(boots),
           "wilcoxon_p": wilcoxon_paired(ms_a, ms_b), "dz": cohens_dz(ms_a, ms_b)}
    if clusters is not None:
        _, lo, hi, boots = _boot(d, n_boot, seed)
        out["item_level"] = {"ci95": [lo, hi], "p_below": p_below(boots)}
    return out


def _h1(ok, ms, keep, timed, delta, n_boot, seed, fam) -> dict:
    sub = lambda mask: None if fam is None else fam[mask]  # noqa: E731
    acc = noninferiority(ok["R3"][keep], ok["R0"][keep], delta, n_boot, seed, sub(keep))
    lat = speed(ms["R3"][keep & timed], ms["R0"][keep & timed], n_boot, seed, sub(keep & timed))
    verdict = combine(outcome(acc["noninferior"], acc["diff"], acc["ci95"], above=True, bound=-delta),
                      outcome(lat["faster"], lat["diff_ms"], lat["ci95"], above=False))
    return {"accuracy": acc, "latency": lat, "outcome": verdict, "supported": verdict == "supported",
            "p_iut": intersection_union_p(acc["p_noninferior"], lat["p_below"])}


def _summary(dec, ok, gold_rows, ms, oos) -> dict:
    answered = np.array([r["intent"] is not None for r in dec])
    compound = np.array(["|" in g["label"] for g in gold_rows])
    single = [k for k, c in enumerate(compound) if not c]
    ms = ms[~np.isnan(ms)]
    return {"n": len(ok), "accuracy": ok.mean(), "coverage": answered.mean(),
            "accuracy_answered": ok[answered].mean() if answered.any() else None,
            "macro_f1": macro_f1([gold_rows[k]["label"] for k in single], [dec[k]["intent"] for k in single],
                                 oos_label=oos),
            "compound_accuracy": ok[compound].mean() if compound.any() else None,
            "tier4_share": np.mean([4 in r["consulted"] for r in dec]),
            "latency_ms": {"n": ms.size, "mean": ms.mean(), "median": np.median(ms), "p95": nearest_rank(ms, 95)}}


def _clean(x):
    """JSON-ready: numpy scalars to Python, NaN to None."""
    if isinstance(x, dict):
        return {k: _clean(v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return [_clean(v) for v in x]
    if isinstance(x, (bool, np.bool_)):
        return bool(x)
    if isinstance(x, np.integer):
        return int(x)
    if isinstance(x, (float, np.floating)):
        return None if math.isnan(x) else float(x)
    return x


def analyze(rows, gold: dict, latency_ids, *, dataset: str, delta: float, dup_ids=frozenset(),
            n_boot: int = N_BOOT, seed: int = 0) -> dict:
    """Everything E1 reports for one dataset and split, from the chunks' rows and the gold items."""
    oos = OOS[dataset]
    timed_rows = [r for r in rows if not r["warmup"]]
    first = {(r["system"], r["id"]): r for r in timed_rows if r["rep"] == 1}
    ids = sorted(gold)
    missing = [f"{s} {i}" for i in ids for s in SYSTEMS if (s, i) not in first]
    if missing:
        raise ValueError(f"no decision for {missing[:5]}{' ...' if len(missing) > 5 else ''}")
    lat = item_latency(timed_rows)
    dec = {s: [first[(s, i)] for i in ids] for s in SYSTEMS}
    dec["R0 untuned"] = [untuned(r) for r in dec["R0"]]
    gold_rows = [gold[i] for i in ids]
    ok = {s: np.array([item_correct(r, g, oos) for r, g in zip(d, gold_rows)]) for s, d in dec.items()}
    timed = np.array([i in latency_ids for i in ids])
    ms = {s: np.array([lat[(i, s.split()[0])] if t else np.nan for i, t in zip(ids, timed)]) for s in dec}
    every = np.ones(len(ids), bool)
    fam = np.array([g[CLUSTERS[dataset]] for g in gold_rows]) if dataset in CLUSTERS else None
    sub = lambda mask: None if fam is None else fam[mask]  # noqa: E731

    report = {"dataset": dataset, "n_items": len(ids), "n_latency_items": int(timed.sum()), "delta": delta,
              "n_boot": n_boot, "seed": seed, "clusters": CLUSTERS.get(dataset),
              "n_clusters": None if fam is None else len(set(fam)),
              "systems": {s: _summary(dec[s], ok[s], gold_rows, ms[s], oos) for s in REPORTED},
              "h1": _h1(ok, ms, every, timed, delta, n_boot, seed, fam)}
    h1c = noninferiority(ok["R3"], ok["R1"], delta, n_boot, seed, fam)
    report["h1c"] = {"accuracy": h1c, "p": h1c["p_noninferior"], "outcome": "after Holm (secondary family)"}

    langs = sorted({g["lang"] for g in gold_rows if "lang" in g})
    single = [i for i in ids if "|" not in gold[i]["label"]]
    fp = lambda sel: float(np.mean([first[("R3", i)]["multi_intent"] for i in sel])) if sel else None  # noqa: E731
    report["signal_false_positive"] = {"all": fp(single)} | {
        lang: fp([i for i in single if gold[i].get("lang") == lang]) for lang in langs}
    if langs:
        report["by_lang"] = {}
        for lang in langs:
            keep = np.array([g.get("lang") == lang for g in gold_rows])
            t = keep & timed
            report["by_lang"][lang] = {
                "n": int(keep.sum()), "n_latency": int(t.sum()),
                "accuracy": noninferiority(ok["R3"][keep], ok["R0"][keep], delta, n_boot, seed, sub(keep)),
                "latency": speed(ms["R3"][t], ms["R0"][t], n_boot, seed, sub(t)) if t.any() else None,
                "R3_tier4_share": np.mean([4 in r["consulted"] for r, k in zip(dec["R3"], keep) if k]),
                "R3_mean_ms": ms["R3"][t].mean() if t.any() else None,
                "R0_mean_ms": ms["R0"][t].mean() if t.any() else None}
        report["h7a"] = report["by_lang"].get("hi-en")

    below = np.array([r["intent"] is not None and r["tier"] < 4 for r in dec["R3"]]) & timed
    r3_med = float(np.median(ms["R3"][below])) if below.any() else None
    r0_med = float(np.median(ms["R0"][timed]))
    report["rho"] = {"target": RHO, "n_below_tier4": int(below.sum()), "r3_median_below_tier4_ms": r3_med,
                     "r0_median_ms": r0_med, "met": r3_med is not None and r3_med <= RHO * r0_med}

    dups = [i for i in ids if i in dup_ids]
    if dups:
        keep = np.array([i not in dup_ids for i in ids])
        report["sensitivity_d1"] = {"n_items": int(keep.sum()), "dropped": dups,
                                    "h1": _h1(ok, ms, keep, timed, delta, n_boot, seed, fam)}
    report["rep_disagreements"] = [list(k) for k in rep_disagreements(timed_rows)]
    return _clean(report)


def dup_ids(dataset: str, split: str, rows) -> set[str]:
    """D1 (test only): ids of the items whose normalized text also occurs in another CLINC150 set."""
    if dataset != "clinc150" or split != "test":
        return set()
    from clinc150 import cross_split_duplicates, kestrel_splits, load_official
    from prep import norm_text
    texts = {t for t, sets in cross_split_duplicates(kestrel_splits(load_official())) if split in sets}
    return {r["id"] for r in rows if norm_text(r["text"]) in texts}


def _f(x, fmt: str) -> str:
    return "—" if x is None else format(x, fmt)


def _ci(c: dict, key: str, fmt: str) -> str:
    return f"{c[key]:{fmt}} [{c['ci95'][0]:{fmt}}, {c['ci95'][1]:{fmt}}]"


def render(rep: dict) -> str:
    out = []
    if rep.get("dev"):
        out.append("**DEV: dry runs (never valid), not E1 results.**\n")
    unit = f"{rep['n_clusters']} {rep['clusters']} clusters" if rep["clusters"] else "items"
    out.append(f"## E1 · {rep['dataset']} {rep['split']} · {rep['n_items']} items, {rep['n_latency_items']} timed"
               f" · δ = {rep['delta']:.4f} · bootstrap over {unit}\n")
    out.append("| System | Accuracy | Macro-F1 | Coverage | Acc. answered | Compound | Mean ms | Median ms | p95 ms "
               "| Reaching Tier 4 |")
    out.append("|---|---|---|---|---|---|---|---|---|---|")
    for s, v in rep["systems"].items():
        lat = v["latency_ms"]
        out.append(f"| {s} | {v['accuracy']:.3f} | {v['macro_f1']:.3f} | {v['coverage']:.3f} | "
                   f"{_f(v['accuracy_answered'], '.3f')} | {_f(v['compound_accuracy'], '.3f')} | {lat['mean']:.1f} | "
                   f"{lat['median']:.1f} | {lat['p95']:.1f} | {v['tier4_share']:.1%} |")
    h = rep["h1"]
    out.append(f"\n**H1** R3 vs tuned R0: Δacc {_ci(h['accuracy'], 'diff', '+.4f')} (> −δ: "
               f"{h['accuracy']['noninferior']}), Δlat {_ci(h['latency'], 'diff_ms', '+.1f')} ms (< 0: "
               f"{h['latency']['faster']}) → **{h['outcome']}** (IUT p {h['p_iut']:.4f})")
    if "item_level" in h["accuracy"]:
        (alo, ahi), (llo, lhi) = h["accuracy"]["item_level"]["ci95"], h["latency"]["item_level"]["ci95"]
        out.append(f"  item-level CIs: Δacc [{alo:+.4f}, {ahi:+.4f}], Δlat [{llo:+.1f}, {lhi:+.1f}] ms")
    c = rep["h1c"]["accuracy"]
    out.append(f"**H1c** R3 vs tuned R1: Δacc {_ci(c, 'diff', '+.4f')} (> −δ: {c['noninferior']}), p "
               f"{rep['h1c']['p']:.4f}; outcome after Holm over the secondary family")
    for lang, v in rep.get("by_lang", {}).items():
        dlat = f"{_ci(v['latency'], 'diff_ms', '+.1f')} ms" if v["latency"] else "— (no timed items)"
        out.append(f"- {lang} (n {v['n']}): Δacc {_ci(v['accuracy'], 'diff', '+.4f')}, Δlat {dlat}; R3 reaching "
                   f"Tier 4 {v['R3_tier4_share']:.1%}, R3 mean {_f(v['R3_mean_ms'], '.1f')} ms vs R0 "
                   f"{_f(v['R0_mean_ms'], '.1f')} ms")
    r = rep["rho"]
    out.append(f"\nρ: R3 median below Tier 4 {_f(r['r3_median_below_tier4_ms'], '.1f')} ms (n {r['n_below_tier4']}) "
               f"vs {RHO} × R0 median {r['r0_median_ms']:.1f} ms → {'met' if r['met'] else 'not met'}")
    out.append("Signal false positives on single-intent items: "
               + ", ".join(f"{k} {_f(v, '.1%')}" for k, v in rep["signal_false_positive"].items()))
    if "sensitivity_d1" in rep:
        d = rep["sensitivity_d1"]["h1"]
        out.append(f"D1 sensitivity ({rep['sensitivity_d1']['n_items']} items): Δacc "
                   f"{_ci(d['accuracy'], 'diff', '+.4f')} → H1 {d['outcome']}")
    out.append(f"Repetitions disagreeing: {len(rep['rep_disagreements'])}")
    return "\n".join(out)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", required=True, choices=sorted(OOS))
    ap.add_argument("--split", default="val", choices=["val", "test"])
    ap.add_argument("--limit", type=int, help="val dry runs: the run directory of `run_e1.py --limit N`")
    ap.add_argument("--dev", action="store_true", help="use dry runs (never valid); the output says DEV")
    ap.add_argument("--record-invalid", action="store_true", help="write the runs rule 3 flags into INVALIDATED.json")
    ap.add_argument("--n-boot", type=int, default=N_BOOT)
    args = ap.parse_args(argv)
    if args.split == "test" and (args.dev or args.limit):
        ap.error("--dev and --limit are for val dry runs")

    name = f"{args.dataset}_{args.split}" + (f"_n{args.limit}" if args.limit else "")
    run_dir = LOG_DIR / name
    dirs = select_runs(run_dir, dev=args.dev)
    manifests = [json.loads((d / "manifest.json").read_text()) for d in dirs]
    rule3 = chunk_problems(manifests, [d.name for d in dirs])
    if rule3 and not args.dev:
        if args.record_invalid:
            record_invalid(run_dir, rule3)
        raise SystemExit("rule 3:\n- " + "\n- ".join(f"{k}: {v}" for k, v in rule3.items())
                         + ("\nrecorded in INVALIDATED.json: rerun those chunks" if args.record_invalid
                            else "\nrecord them with --record-invalid, then rerun those chunks"))
    configs = check_systems(manifests, frozen=args.split == "test")
    used, plan = chunk_plan(load_items(args.dataset, args.split), args.dataset, args.limit)
    check_items(manifests, plan)
    rows = [json.loads(line) for d in dirs for line in (d / "rows.jsonl").read_text().splitlines()]
    delta = json.loads(DELTA_LOG.read_text())["delta"]
    report = analyze(rows, {r["id"]: r for r in used}, {it["id"] for c in plan for it in c if it["reps"] == REPS},
                     dataset=args.dataset, delta=delta, dup_ids=dup_ids(args.dataset, args.split, used),
                     n_boot=args.n_boot)
    problems = {d.name: m["problems"] + ([rule3[d.name]] if d.name in rule3 else [])
                for d, m in zip(dirs, manifests) if m["problems"] or d.name in rule3}
    report = {"experiment": "E1", "split": args.split, "dev": args.dev,
              "valid": all(m["valid"] for m in manifests) and not rule3,
              "runs": [str(d.relative_to(ROOT)) for d in dirs], "configs": configs, "problems": problems,
              "delta_source": str(DELTA_LOG.relative_to(ROOT))} | report
    out = LOG_DIR / f"analysis_{name}.json"
    out.write_text(json.dumps(report, indent=1, ensure_ascii=False) + "\n")
    print(render(report))
    print(f"\n-> {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
