"""experiments/analysis/paper_assets.py -- the paper's numbers, tables and figures, from the committed logs (Week 16).

    python experiments/analysis/paper_assets.py     # -> paper/generated/*.tex, paper/figures/*.pdf and *.png

Every number the paper states is a macro `\\kv{key}` defined in paper/generated/numbers.tex from an analysis log
(E1, E2, E3, E5 test analyses, the Holm family, the failure analysis, the E0 sessions, the privacy and the
reproduction checks); a key the paper uses but this file does not define prints as ??key?? in the PDF. Tables
R1, R2, R3, R6 and the family table are generated the same way; nothing is typed by hand. Figures: the
accuracy-latency plane of the E1 systems and the extra E3 tier sets, the risk-coverage curves of R2 vs R2c, and
E5's accuracy and forgetting. Synthetic data (Kestrel-Intents-v0) is labelled as such.
"""
from __future__ import annotations

import csv
import glob
import json
import re
from functools import lru_cache
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
LOGS = ROOT / "experiments" / "logs"
OUT = ROOT / "paper" / "generated"
FIGS = ROOT / "paper" / "figures"
DS = {"clinc": "clinc150", "intents": "intents_v0"}
SYSTEMS = {"r0": "R0", "r0u": "R0 untuned", "r1": "R1", "r3": "R3"}
BLUE, ORANGE, INK, MUTED, GRID = "#2a78d6", "#eb6834", "#0b0b0b", "#52514e", "#e6e5e1"  # dataviz reference palette


def _load(rel: str):
    return json.loads((LOGS / rel).read_text())


def signed(x: float, nd: int = 3) -> str:
    return f"{x:+.{nd}f}"


def ci(c, nd: int = 3) -> str:
    return f"[{c[0]:+.{nd}f}, {c[1]:+.{nd}f}]"


def fmt_p(p: float | None) -> str:
    """A p-value as LaTeX math: 0 means fewer than 1 in 10 000 bootstrap resamples."""
    if p is None:
        return "\\text{pending}"
    if p == 0:
        return "<10^{-4}"
    if p >= 0.9995:
        return "1"
    if p < 0.001:
        m, e = f"{p:.1e}".split("e")
        return f"{m}\\times 10^{{{int(e)}}}"
    return f"{p:.3f}" if p < 0.01 else f"{p:.2g}" if p < 0.1 else f"{p:.2f}"


def tex_escape(s: str) -> str:
    """Text for LaTeX: the characters that are special in it, escaped."""
    special = {"&": "\\&", "%": "\\%", "#": "\\#", "_": "\\_", "{": "\\{", "}": "\\}"}
    return "".join(special.get(ch, ch) for ch in s)


def _typeset(v: str) -> str:
    """A value as it reads in running text: a commit hash in typewriter, words as words, anything else as math."""
    if re.fullmatch(r"[0-9a-f]{7,40}", v) and re.search(r"[a-f]", v):
        return f"\\texttt{{{v}}}"
    if re.fullmatch(r"[a-z][a-z ()]*", v):
        return v
    return f"\\ensuremath{{{v}}}"


def value_lines(values: dict[str, str]) -> list[str]:
    return [f"\\expandafter\\def\\csname kv@{k}\\endcsname{{{_typeset(v)}}}" for k, v in values.items()]


def _e0_cv() -> float:
    from e0_summary import across_sessions, per_session
    sessions = []
    for s in ("20261011T101743Z", "20261017T132649Z", "20261023T163212Z"):
        with open(LOGS / "E0" / f"{s}.csv", newline="") as f:
            sessions.append(per_session(list(csv.DictReader(f))))
    slm = [v["cv_pct"] for (m, t), v in across_sessions(sessions).items() if t in ("pp128", "tg64")]
    return max(slm)


def _r3_tier4_split(ds: str) -> tuple[float, float]:
    """(answered at Tier 4, abstained) as shares of all items, from R3's first-repetition E1 test rows. Table R1's
    "Tier 4" counts every request that consulted Tier 4, including those that then abstained."""
    rows = [json.loads(line) for f in sorted(glob.glob(str(LOGS / "E1" / f"{ds}_test" / "*" / "rows.jsonl")))
            for line in Path(f).read_text().splitlines()]
    r3 = [r for r in rows if r["system"] == "R3" and not r["warmup"] and r["rep"] == 1]
    answered4 = sum(r["intent"] is not None and r["tier"] == 4 for r in r3)
    return answered4 / len(r3), sum(r["intent"] is None for r in r3) / len(r3)


def _t3_kendall(ds: str, t3: float) -> float:
    """Kendall's tau between Tier 3's top-class confidence at T = 1 and at its fitted T, on the test tier outputs. A
    temperature keeps each request's label, but over many classes it can reorder confidences across requests."""
    from scipy.stats import kendalltau
    from analyze_e2 import load
    recs, _ = load(ds, "test")
    z = np.array([r["t3"]["logits"] for r in recs], dtype=np.float64)

    def top(t):
        e = np.exp(z / t - (z / t).max(axis=1, keepdims=True))
        return e.max(axis=1) / e.sum(axis=1)
    return float(kendalltau(top(1.0), top(t3)).statistic)


@lru_cache(maxsize=None)
def val_operating_points(ds: str) -> dict[str, dict]:
    """Exploratory, run after the test results: Eq. (op)'s per-tier search on validation (routing signal on, tolerance
    0), once on raw and once on calibrated confidences. The calibrated run is the search that chose the frozen R3."""
    import analyze_e2  # noqa: F401  (puts experiments/router on sys.path)
    import policies as P
    tiers, oos = P.DATASETS[ds]
    recs, meta, _ = P.read_tier_outputs(ds)
    raw, cal, *_ = P.calibrated(recs, P.label_classes(meta, ds), meta["tier3"]["temperature_cal"], oos, tiers)
    floor = P.reference_acc(cal)
    return {name: P.best_per_tier(a, {0.0: floor}, "conjunction", tiers)[0][0.0]
            for name, a in (("raw", raw), ("cal", cal))}


def numbers() -> dict[str, str]:
    n: dict[str, str] = {}
    delta = json.loads((LOGS / "router" / "delta_pilot.json").read_text())["delta"]
    n["delta"] = f"{delta:.4f}"
    for short, ds in DS.items():
        e1 = _load(f"E1/analysis_{ds}_test.json")
        n[f"{short}.n"], n[f"{short}.timed"] = str(e1["n_items"]), str(e1["n_latency_items"])
        for k, s in SYSTEMS.items():
            v = e1["systems"][s]
            n[f"{short}.{k}.acc"] = f"{v['accuracy']:.3f}"
            n[f"{short}.{k}.f1"] = f"{v['macro_f1']:.3f}"
            n[f"{short}.{k}.cov"] = f"{v['coverage']:.3f}"
            n[f"{short}.{k}.mean"] = f"{v['latency_ms']['mean']:.0f}"
            n[f"{short}.{k}.median"] = f"{v['latency_ms']['median']:.1f}"
            n[f"{short}.{k}.p95"] = f"{v['latency_ms']['p95']:.0f}"
            n[f"{short}.{k}.rss"] = f"{v['peak_rss_mib']:.0f}"
            n[f"{short}.{k}.tfour"] = f"{100 * v['tier4_share']:.1f}\\%"
        answered4, abstained = _r3_tier4_split(ds)
        n[f"{short}.r3.tfourans"], n[f"{short}.r3.abstain"] = f"{100 * answered4:.1f}\\%", f"{100 * abstained:.1f}\\%"
        h = e1["h1"]
        n[f"{short}.h1.dacc"], n[f"{short}.h1.dacc.ci"] = signed(h["accuracy"]["diff"]), ci(h["accuracy"]["ci95"])
        n[f"{short}.h1.dlat"] = signed(h["latency"]["diff_ms"], 0)
        n[f"{short}.h1.dlat.ci"] = ci(h["latency"]["ci95"], 0)
        n[f"{short}.h1.outcome"] = h["outcome"]
        n[f"{short}.h1.p"] = fmt_p(h["p_iut"])
        b = e1["h1b"]
        n[f"{short}.h1b.dacc"], n[f"{short}.h1b.dacc.ci"] = signed(b["accuracy"]["diff"]), ci(b["accuracy"]["ci95"])
        n[f"{short}.rrand.acc"] = f"{b['rrand']['acc']:.3f}"
        c = e1["h1c"]["accuracy"]
        n[f"{short}.h1c.dacc"], n[f"{short}.h1c.dacc.ci"] = signed(c["diff"]), ci(c["ci95"])
        r = e1["rho"]
        n[f"{short}.rho.rthree"] = f"{r['r3_median_below_tier4_ms']:.1f}"
        n[f"{short}.rho.rzero"] = f"{r['r0_median_ms']:.0f}"
        n[f"{short}.sigfp"] = f"{100 * e1['signal_false_positive']['all']:.1f}\\%"
        if "sensitivity_d1" in e1:
            d = e1["sensitivity_d1"]
            n["clinc.done.n"], n["clinc.done.dacc.ci"] = str(d["n_items"]), ci(d["h1"]["accuracy"]["ci95"])
        if "by_lang" in e1:
            for lang, key in (("en", "en"), ("hi-en", "hi")):
                v = e1["by_lang"][lang]
                n[f"intents.{key}.dacc"], n[f"intents.{key}.dacc.ci"] = (signed(v["accuracy"]["diff"]),
                                                                         ci(v["accuracy"]["ci95"]))
                n[f"intents.{key}.tfour"] = f"{100 * v['R3_tier4_share']:.1f}\\%"
                n[f"intents.{key}.rthree.mean"] = f"{v['R3_mean_ms']:.0f}"
            n["intents.clusters"] = str(e1["n_clusters"])
        e2 = _load(f"E2/analysis_{ds}_test.json")
        h2 = e2["h2"]
        for m in ("risk_at_080", "aurc"):
            k = "risk" if m == "risk_at_080" else "aurc"
            n[f"{short}.{k}.rtwo"], n[f"{short}.{k}.rtwoc"] = f"{h2[m]['R2']:.4f}", f"{h2[m]['R2c']:.4f}"
            n[f"{short}.{k}.diff"], n[f"{short}.{k}.diff.ci"] = signed(h2[m]["diff"], 4), ci(h2[m]["ci95"], 4)
        for tier, t in e2["table_r2"].items():
            n[f"{short}.tthree.T" if tier == "3" else f"{short}.tfour.T"] = f"{t['T']:.3f}"
            k = "tthree" if tier == "3" else "tfourcal"
            n[f"{short}.{k}.ece.before"] = f"{t['ece_before']:.4f}"
            n[f"{short}.{k}.ece.after"] = f"{t['ece_after']:.4f}"
        n[f"{short}.tthree.kendall"] = f"{_t3_kendall(ds, e2['table_r2']['3']['T']):.2f}"
        op = val_operating_points(ds)
        if op["raw"]["acc"] != op["cal"]["acc"]:
            raise ValueError(f"{ds}: the raw and calibrated validation points differ in accuracy; reword the paper")
        n[f"{short}.valop.acc"] = f"{op['cal']['acc']:.3f}"
        n[f"{short}.valop.raw.cost"], n[f"{short}.valop.cal.cost"] = (f"{op['raw']['cost']:.0f}",
                                                                      f"{op['cal']['cost']:.0f}")
        if "h7b" in e2:
            v = e2["h7b"]["tier3"]
            n["intents.hsevenb.en"], n["intents.hsevenb.hi"] = f"{v['ece_en']:.4f}", f"{v['ece_hi_en']:.4f}"
            n["intents.hsevenb.gap"], n["intents.hsevenb.gap.ci"] = signed(v["gap"], 4), ci(v["ci95"], 4)
        e3 = _load(f"E3/analysis_{ds}_test.json")
        for name, s in e3["sets"].items():
            key = "set" + name.strip("{}").replace(",", "")
            n[f"{short}.{key}.acc"], n[f"{short}.{key}.cost"] = f"{s['accuracy']:.3f}", f"{s['cost_ms']['mean']:.0f}"
        h3 = e3["h3"]
        n[f"{short}.hthree.dacc"] = signed(h3["accuracy"]["diff"])
        n[f"{short}.hthree.dacc.ci"] = ci(h3["accuracy"]["ci95"])
        n[f"{short}.hthree.dcost"] = signed(h3["cost"]["diff_ms"], 0)
        n[f"{short}.hthree.dcost.ci"] = ci(h3["cost"]["ci95"], 0)
    e5 = _load("E5/analysis_test.json")
    for m, s in e5["table_r6"].items():
        if s:
            n[f"e5.{m}.acc"] = f"{s['avg_accuracy']['mean']:.3f}"
            n[f"e5.{m}.forget"] = f"{s['avg_forgetting']['mean']:.3f}"
            n[f"e5.{m}.bwt"] = signed(s["bwt"]["mean"])
            n[f"e5.{m}.buffer"] = f"{s['buffer_bytes']['mean'] / 1e6:.1f}"
    n["e5.h5.diff"], n["e5.h5.diff.ci"] = signed(e5["h5"]["diff"]), ci(e5["h5"]["ci95"])
    n["e5.h5.p"] = fmt_p(e5["h5"]["p"])
    n["e5.ewc.diff"] = signed(e5["ewc_vs_naive"]["diff"])
    fam = _load("family_v1.json")
    for m in fam["members"]:
        key = "fam." + m["id"].replace(" ", "-")
        n[f"{key}.p"], n[f"{key}.holm"], n[f"{key}.outcome"] = fmt_p(m["p"]), fmt_p(m["p_holm"]), m["outcome"]
    n["fam.supported"] = str(sum(m["outcome"] == "supported" for m in fam["members"]))
    err = _load("errors_test.json")
    c = err["clinc150"]
    n["err.clinc.errors"], n["err.clinc.ooserrors"] = str(c["r3_errors"]), str(c["oos_errors"])
    n["err.clinc.oosshare"] = f"{100 * c['oos_errors'] / c['r3_errors']:.0f}\\%"
    n["err.clinc.tau.rthree"], n["err.clinc.tau.rone"] = f"{c['tau3']['R3']:.3f}", f"{c['tau3']['R1']:.3f}"
    n["err.clinc.band"] = str(c["oos_accepted_in_band"])
    n["err.clinc.oos.val"] = f"{100 * err['val_oos_share']:.1f}\\%"
    n["err.clinc.oos.test"] = f"{100 * c['n_oos'] / c['n']:.1f}\\%"
    i = err["intents_v0"]
    n["err.intents.errors"] = str(len(i["r3_errors"]))
    n["err.intents.compound"] = str(i["compound_accepted_below_tier4"])
    n["err.intents.valcompounds"] = str(i["val_compounds"])
    n["ezero.cvmax"] = f"{_e0_cv():.1f}\\%"
    priv = json.loads(Path(sorted(glob.glob(str(LOGS / "privacy" / "*.json")))[-1]).read_text())
    n["priv.pass"], n["priv.total"] = str(sum(priv["checks"].values())), str(len(priv["checks"]))
    rep = json.loads(Path(sorted(glob.glob(str(LOGS / "repro" / "*.json")))[-1]).read_text())
    n["repro.identical"] = str(sum(v == "identical" for v in rep["outputs"].values()))
    n["repro.head"] = rep["head"][:7]
    return n


def _row(cells) -> str:
    return " & ".join(cells) + r" \\"


def table_r1(ds: str) -> str:
    e1 = _load(f"E1/analysis_{ds}_test.json")
    head = _row(["System", "Accuracy", "Macro-F1", "Coverage", "Mean ms", "Median ms", "p95 ms", "Peak RSS MiB",
                 "Tier 4"])
    rows = [_row([s, f"{v['accuracy']:.3f}", f"{v['macro_f1']:.3f}", f"{v['coverage']:.3f}",
                  f"{v['latency_ms']['mean']:.0f}", f"{v['latency_ms']['median']:.1f}", f"{v['latency_ms']['p95']:.0f}",
                  f"{v['peak_rss_mib']:.0f}", f"{100 * v['tier4_share']:.0f}\\%"]) for s, v in e1["systems"].items()]
    return "\n".join([r"\begin{tabular}{lrrrrrrrr}", r"\toprule", head, r"\midrule", *rows, r"\bottomrule",
                      r"\end{tabular}"]) + "\n"


def table_r2() -> str:
    rows = []
    for short, ds in DS.items():
        e2 = _load(f"E2/analysis_{ds}_test.json")
        name = "CLINC150" if short == "clinc" else "Intents-v0"
        for tier, t in e2["table_r2"].items():
            rows.append(_row([name, f"Tier {tier}", str(t["n"]), f"{t['accuracy']:.3f}", f"{t['T']:.3f}",
                              f"{t['ece_before']:.4f}", f"{t['ece_after']:.4f}", f"{t['risk_at_080_before']:.4f}",
                              f"{t['risk_at_080_after']:.4f}"]))
    head = _row(["Data", "Tier", "n", "Acc.", "$T$", "ECE raw", "ECE cal.", "Risk@0.8 raw", "Risk@0.8 cal."])
    return "\n".join([r"\begin{tabular}{llrrrrrrr}", r"\toprule", head, r"\midrule", *rows, r"\bottomrule",
                      r"\end{tabular}"]) + "\n"


def table_r3() -> str:
    rows = []
    for short, ds in DS.items():
        e3 = _load(f"E3/analysis_{ds}_test.json")
        name = "CLINC150" if short == "clinc" else "Intents-v0"
        for k, s in e3["sets"].items():
            rates = " / ".join(f"{s['tier_rate'][t]:.2f}" for t in ("1", "3", "4"))
            rows.append(_row([name, "\\{" + k.strip("{}") + "\\}", f"{s['accuracy']:.3f}",
                              f"{s['cost_ms']['mean']:.0f}", f"{s['cost_ms']['p95']:.0f}", rates]))
    head = _row(["Data", "Tier set", "Accuracy", "Mean ms", "p95 ms", "Ended at Tier 1 / 3 / 4"])
    return "\n".join([r"\begin{tabular}{llrrrl}", r"\toprule", head, r"\midrule", *rows, r"\bottomrule",
                      r"\end{tabular}"]) + "\n"


def table_r6() -> str:
    e5 = _load("E5/analysis_test.json")
    ms = lambda s, k: f"{s[k]['mean']:.3f} $\\pm$ {s[k]['sd']:.3f}"  # noqa: E731
    rows = [_row([m, ms(s, "avg_accuracy"), ms(s, "avg_forgetting"), signed(s["bwt"]["mean"]),
                  f"{s['seconds']['mean']:.1f}", f"{s['buffer_bytes']['mean'] / 1e6:.1f}"])
            for m, s in e5["table_r6"].items() if s]
    head = _row(["Method", "Avg. accuracy", "Avg. forgetting", "BWT", "Update s", "Buffer MB"])
    return "\n".join([r"\begin{tabular}{lrrrrr}", r"\toprule", head, r"\midrule", *rows, r"\bottomrule",
                      r"\end{tabular}"]) + "\n"


def table_family() -> str:
    fam = _load("family_v1.json")
    rows = [_row([m["id"], f"${fmt_p(m['p'])}$", f"${fmt_p(m['p_holm'])}$", m["outcome"], m.get("source", "---")])
            for m in fam["members"]]
    head = _row(["Hypothesis", "$p$", "Holm $p$", "Outcome", "Source"])
    return "\n".join([r"\begin{tabular}{lllll}", r"\toprule", head, r"\midrule", *rows, r"\bottomrule",
                      r"\end{tabular}"]) + "\n"


def _style(plt):
    plt.rcParams.update({"font.size": 8.5, "axes.edgecolor": MUTED, "axes.labelcolor": INK, "xtick.color": MUTED,
                         "ytick.color": MUTED, "axes.spines.top": False, "axes.spines.right": False,
                         "axes.grid": True, "grid.color": GRID, "grid.linewidth": 0.6, "legend.frameon": False,
                         "savefig.bbox": "tight", "savefig.dpi": 200, "pdf.fonttype": 42})


def _save(fig, name: str):
    FIGS.mkdir(parents=True, exist_ok=True)
    fig.savefig(FIGS / f"{name}.pdf")
    fig.savefig(FIGS / f"{name}.png")


# Label offsets (points) for the frontier figure, so labels don't collide (the data are the frozen test results).
OFFSETS = {"R0 = R0 untuned": (0, 10, "center"), "tier set {3,4}": (-8, 2, "right"), "R3": (-8, -9, "right"),
           "tier set {1,4}": (8, -9, "left"), "R0 untuned": (-8, -3, "right")}


def frontier_points(ds: str) -> list[tuple[str, float, float, str]]:
    """(label, mean ms, accuracy, source) of the E1 systems and the E3 tier sets that add a new point;
    systems at the same point share one label."""
    e1, e3 = _load(f"E1/analysis_{ds}_test.json"), _load(f"E3/analysis_{ds}_test.json")
    pts: list[list] = []
    for label, v, src in [(s, v, "E1") for s, v in e1["systems"].items()] + \
            [("tier set " + k, s, "E3") for k, s in e3["sets"].items()]:
        lat = v["latency_ms"]["mean"] if src == "E1" else v["cost_ms"]["mean"]
        same = [p for p in pts if abs(p[1] - lat) < 0.5 and abs(p[2] - v["accuracy"]) < 1e-9]
        if same and src == "E1":
            same[0][0] += " = " + label
        elif not same:
            pts.append([label, lat, v["accuracy"], src])
    return [tuple(p) for p in pts]


def non_dominated(pts) -> list:
    """The points no other point beats on both latency (lower) and accuracy (higher)."""
    return [p for p in pts if not any(q[1] <= p[1] and q[2] >= p[2] and (q[1] < p[1] or q[2] > p[2]) for q in pts)]


def fig_frontier(plt):
    fig, axes = plt.subplots(1, 2, figsize=(6.6, 2.9))
    for ax, (ds, title) in zip(axes, (("clinc150", "CLINC150 test"),
                                      ("intents_v0", "Kestrel-Intents-v0 test (synthetic)"))):
        pts = frontier_points(ds)
        best = non_dominated(pts)
        for p in best:
            ax.plot(p[1], p[2], "o", ms=13, mfc="none", mec=MUTED, mew=1, zorder=2)
        for label, x, y, src in pts:
            ax.plot(x, y, "o" if src == "E1" else "s", ms=6, color=BLUE if src == "E1" else ORANGE,
                    markeredgecolor="white", markeredgewidth=1, zorder=3)
            dx, dy, ha = OFFSETS.get(label, (8, 3, "left"))
            ax.annotate(label, (x, y), textcoords="offset points", xytext=(dx, dy), ha=ha, fontsize=7.5, color=INK)
        ax.set_xscale("log")
        ax.set_xlabel("mean latency per request (ms, log scale)")
        ax.set_title(title, fontsize=8.5, color=INK, loc="left")
        ax.set_xlim(2, 4000)
        lo, hi = min(p[2] for p in pts), max(p[2] for p in pts)
        ax.set_ylim(lo - (hi - lo) * 0.15, min(1.04, hi + (hi - lo) * 0.15))
    axes[0].set_ylabel("accuracy (abstention = error)")
    axes[0].plot([], [], "o", color=BLUE, label="E1 system")
    axes[0].plot([], [], "s", color=ORANGE, label="E3 tier set")
    axes[0].plot([], [], "o", ms=11, mfc="none", mec=MUTED, label="non-dominated")
    axes[0].legend(loc="lower left", fontsize=7.5)
    _save(fig, "frontier")
    plt.close(fig)


def fig_riskcov(plt):
    import numpy as np
    from analyze_e2 import answers, load, outcomes
    from policies import DATASETS, label_classes
    fig, axes = plt.subplots(1, 2, figsize=(6.6, 2.5))
    for ax, (ds, title) in zip(axes, (("clinc150", "CLINC150 test"), ("intents_v0", "Kestrel-Intents-v0 test"))):
        cfg = json.loads((ROOT / "experiments" / "configs" / f"router_{ds}.json").read_text())
        recs, meta = load(ds, "test")
        raw, cal = answers(recs, label_classes(meta, ds), cfg, "oos" if ds == "clinc150" else "unsupported")
        for a, name, color in ((raw, "R2 (raw confidences)", BLUE), (cal, "R2c (calibrated)", ORANGE)):
            _, ans, err = outcomes(a, DATASETS[ds][0])
            cov = ans.mean(axis=1)
            with np.errstate(invalid="ignore"):
                risk = err.sum(axis=1) / ans.sum(axis=1)
            keep = cov > 0.05
            order = np.argsort(cov[keep])
            ax.plot(cov[keep][order], risk[keep][order], color=color, lw=1.6, label=name)
        ax.axvline(0.8, color=MUTED, lw=0.8, ls=":")
        ax.set_xlabel("coverage (share of requests answered)")
        ax.set_title(title, fontsize=8.5, color=INK, loc="left")
    axes[0].set_ylabel("selective risk (error among answered)")
    axes[0].legend(loc="upper left", fontsize=7.5)
    _save(fig, "riskcov")
    plt.close(fig)


def fig_e5(plt):
    e5 = _load("E5/analysis_test.json")["table_r6"]
    methods = [m for m in ("naive", "ewc", "replay", "joint", "stage0") if e5.get(m)]
    names = {"naive": "naive", "ewc": "EWC", "replay": "replay",
             "joint": "joint\n(upper)", "stage0": "stage 0\n(lower)"}
    fig, axes = plt.subplots(1, 2, figsize=(6.6, 2.3))
    for ax, key, title in zip(axes, ("avg_accuracy", "avg_forgetting"), ("average accuracy", "average forgetting")):
        vals = [e5[m][key]["mean"] for m in methods]
        sds = [e5[m][key]["sd"] for m in methods]
        ax.set_axisbelow(True)
        ax.bar(range(len(methods)), vals, yerr=sds, color=BLUE, width=0.6, error_kw={"lw": 0.8, "ecolor": MUTED})
        for k, v in enumerate(vals):
            ax.annotate(f"{v:.3f}", (k, v), textcoords="offset points", xytext=(0, 3), ha="center", fontsize=7.5)
        ax.set_xticks(range(len(methods)), [names[m] for m in methods])
        ax.set_title(title + " (CLINC150 test, 15 runs)", fontsize=8.5, color=INK, loc="left")
        ax.set_ylim(min(0, min(vals) - 0.05), 1.08)
        ax.grid(axis="x", visible=False)
    _save(fig, "e5")
    plt.close(fig)


def main() -> int:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    _style(plt)
    OUT.mkdir(parents=True, exist_ok=True)
    n = numbers()
    (OUT / "numbers.tex").write_text("% Generated by experiments/analysis/paper_assets.py from the committed logs.\n"
                                     "\\makeatletter\n" + "\n".join(value_lines(n)) + "\n\\makeatother\n")
    for name, text in (("table_r1_clinc150", table_r1("clinc150")), ("table_r1_intents_v0", table_r1("intents_v0")),
                       ("table_r2", table_r2()), ("table_r3", table_r3()), ("table_r6", table_r6()),
                       ("table_family", table_family())):
        (OUT / f"{name}.tex").write_text("% Generated by experiments/analysis/paper_assets.py.\n" + text)
    fig_frontier(plt)
    fig_riskcov(plt)
    fig_e5(plt)
    print(f"{len(n)} numbers, 6 tables, 3 figures -> paper/generated, paper/figures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
