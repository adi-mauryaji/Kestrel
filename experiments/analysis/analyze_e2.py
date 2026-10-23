"""experiments/analysis/analyze_e2.py -- E2 (H2, H7b; PREREG §3, §6, §9, ADR-029, ADR-031): Table R2, H2, AURC.

    python experiments/analysis/analyze_e2.py --dataset clinc150 --split test   # after tier_outputs.py --split test
    python experiments/analysis/analyze_e2.py --dataset intents_v0 --split val  # dev check against policies' log

From every tier's logged output (router/tier_outputs.py) with the frozen R3 config's calibration (rule
confidence, Tier-3 and Tier-4 temperatures; nothing is refitted): R2 = raw (temperatures 1, rule
confidence 1), R2c = calibrated. H2: one shared tau for every tier, Tier 4 included, no routing signal (D9);
selective risk at coverage 0.80 by D4's rule; claim R2c < R2; p = bootstrap p_below of the paired difference,
resampling families on Intents-v0 and items on CLINC150 (ADR-029). AURC (§9): the mean over c = 0.05 ... 1.00
of the risk at coverage c (D4), with its paired CI. Every resample re-evaluates the whole curve as weighted
sums (kestrel_stats.resample_weights). Table R2, per tier (3 and 4) on single-intent items whose gold label is
a head class: ECE before / after T (15 right-closed bins, D5), T, and the tier's own selective risk at 80%
coverage before / after (close but not always equal: a temperature can reorder items whose logit gaps differ,
and Tier 4's confidence is a product over decoding steps). H7b
(Intents-v0): Tier 3's calibrated ECE(hi-en) - ECE(en) on those items, claim > 0, p_above over families;
Tier 4's reported. Out: experiments/logs/E2/analysis_<dataset>_<split>.json.
"""
from __future__ import annotations

import argparse
import gzip
import json
from pathlib import Path

import numpy as np

from analyze_e1 import CLUSTERS, OOS, _clean
from kestrel_stats import N_BOOT, bootstrap, p_above, p_below, percentile_ci, resample_weights
from metrics import ece
from policies import COVERAGE, DATASETS, ECE_BINS, Answers, _grid, correct, label_classes, read_tier_outputs, route

ROOT = Path(__file__).resolve().parents[2]
LOG_DIR = ROOT / "experiments" / "logs" / "router"
OUT_DIR = ROOT / "experiments" / "logs" / "E2"
LEVELS = np.round(np.arange(1, 21) * 0.05, 2)  # AURC's coverage grid (§9); 0.80 is one of them
BLOCK = 250


def load(dataset: str, split: str) -> tuple[list[dict], dict]:
    if split == "val":
        recs, meta, _ = read_tier_outputs(dataset)
        return [r for r in recs if r["split"] == "val"], meta
    plain = LOG_DIR / f"tier_outputs_{dataset}_test.jsonl"
    meta = json.loads(plain.with_suffix(".meta.json").read_text())
    for path, opener in ((plain, open), (plain.with_name(plain.name + ".gz"), gzip.open)):
        if path.exists():
            with opener(path, "rt") as f:
                return [json.loads(line) for line in f], meta
    raise FileNotFoundError(f"{plain}[.gz]: run router/tier_outputs.py --dataset {dataset} --split test")


def answers(recs, classes, cfg: dict, oos: str) -> tuple[Answers, Answers]:
    """(raw, calibrated) answers with the frozen config's calibration."""
    cal = cfg["calibration"]
    raw = Answers.build(recs, classes, rule_conf=1.0, t3=1.0, t4=1.0, oos_label=oos)
    rule = cal["tier1_confidence"] if cal["tier1_confidence"] is not None else 1.0
    return raw, Answers.build(recs, classes, rule_conf=rule, t3=cal["tier3_temperature"],
                              t4=cal["tier4_temperature"], oos_label=oos)


def outcomes(a: Answers, tiers) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """(taus, answered, wrong-and-answered) at every shared tau where the outcome can change (as
    policies.sweep_shared: Tier 4 included, no signal); the last two are taus x items."""
    c1 = a.c1[a.m1] if 1 in tiers else np.array([])
    taus = _grid(np.r_[c1, a.c3, a.c4[a.m4]])
    ans, err = [], []
    for t in taus:
        r = route(a, (t, t, t), tiers)
        answered = r.pred != -1
        ans.append(answered)
        err.append(answered & ~correct(a, r.pred, r.tier))
    return taus, np.array(ans), np.array(err)


def risk_levels(cov: np.ndarray, risk: np.ndarray, levels) -> np.ndarray:
    """Per row (resample), the risk at each coverage level by D4: the point with the smallest coverage
    >= the level (ties: the lower risk); if none reaches it, the point with the largest coverage."""
    risk = np.where(np.isnan(risk), np.inf, risk)
    out = np.empty((cov.shape[0], len(levels)))
    top = cov.max(axis=1, keepdims=True)
    fallback = np.where(cov == top, risk, np.inf).min(axis=1)
    for j, c in enumerate(levels):
        reach = cov >= c - 1e-9
        m = np.where(reach, cov, np.inf).min(axis=1, keepdims=True)
        out[:, j] = np.where(reach.any(axis=1), np.where(reach & (cov == m), risk, np.inf).min(axis=1), fallback)
    return out


def curve_stats(w: np.ndarray, ans: np.ndarray, err: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """(risk at 0.80, AURC) for each row of resample weights w (resamples x items)."""
    # Counts are exact in float32 (whole numbers far below 2**24); the ratios are taken in float64, so a
    # coverage of 21/60 is not read as just under the 0.35 level.
    n_ans = (w @ ans.T.astype(np.float32)).astype(np.float64)
    cov = n_ans / w.sum(axis=1, keepdims=True, dtype=np.float64)
    with np.errstate(invalid="ignore", divide="ignore"):
        risk = (w @ err.T.astype(np.float32)).astype(np.float64) / n_ans
    levels = risk_levels(cov, risk, LEVELS)
    return levels[:, list(LEVELS).index(COVERAGE)], levels.mean(axis=1)


def tier_risk_at(conf, ok, coverage: float = COVERAGE) -> float:
    """One tier's own selective risk at `coverage` (D4) over its confidence thresholds."""
    conf, ok = np.asarray(conf, float), np.asarray(ok, bool)
    cov, risk = [], []
    for t in _grid(conf):
        answered = conf >= t
        cov.append(answered.mean())
        risk.append(1 - ok[answered].mean() if answered.any() else np.nan)
    return float(risk_levels(np.array([cov]), np.array([risk]), [coverage])[0, 0])


def ece_gap(conf, ok, lang) -> float:
    """ECE(hi-en) - ECE(en), 15 bins."""
    conf, ok, lang = np.asarray(conf, float), np.asarray(ok, bool), np.asarray(lang)
    e = {g: ece(conf[lang == g].tolist(), ok[lang == g].tolist(), n_bins=ECE_BINS) for g in ("hi-en", "en")}
    return e["hi-en"] - e["en"]


def h2(raw: Answers, cal: Answers, tiers, fam, n_boot: int, seed: int) -> dict:
    _, ans_r, err_r = outcomes(raw, tiers)
    _, ans_c, err_c = outcomes(cal, tiers)
    one = np.ones((1, len(raw.gold)), np.float32)
    full = {"R2": curve_stats(one, ans_r, err_r), "R2c": curve_stats(one, ans_c, err_c)}
    d_risk, d_aurc = [], []
    for w in resample_weights(len(raw.gold), n_boot, seed, fam):
        for lo in range(0, w.shape[0], BLOCK):
            wb = w[lo:lo + BLOCK]
            (r_r, a_r), (r_c, a_c) = curve_stats(wb, ans_r, err_r), curve_stats(wb, ans_c, err_c)
            d_risk.append(r_c - r_r)
            d_aurc.append(a_c - a_r)
    d_risk, d_aurc = np.concatenate(d_risk), np.concatenate(d_aurc)
    risk = {"R2": full["R2"][0][0], "R2c": full["R2c"][0][0]}
    aurc = {"R2": full["R2"][1][0], "R2c": full["R2c"][1][0]}
    return {"risk_at_080": risk | {"diff": risk["R2c"] - risk["R2"], "ci95": list(percentile_ci(d_risk)),
                                   "p_below": p_below(d_risk)},
            "aurc": aurc | {"diff": aurc["R2c"] - aurc["R2"], "ci95": list(percentile_ci(d_aurc)),
                            "p_below": p_below(d_aurc)},
            "points": {"R2": int(ans_r.shape[0]), "R2c": int(ans_c.shape[0])},
            "p": p_below(d_risk), "claim": "risk at coverage 0.80: R2c < R2"}


def tier_table(raw: Answers, cal: Answers, single, cfg: dict) -> dict:
    c = cfg["calibration"]
    out = {}
    for tier, conf_r, conf_c, y, mask, t in (
            (3, raw.c3, cal.c3, raw.y3, single, c["tier3_temperature"]),
            (4, raw.c4, cal.c4, raw.y4, single & raw.m4, c["tier4_temperature"])):
        ok = y[mask] == raw.gold[mask]
        out[str(tier)] = {"n": int(mask.sum()), "accuracy": ok.mean(), "T": t,
                          "ece_before": ece(conf_r[mask].tolist(), ok.tolist(), n_bins=ECE_BINS),
                          "ece_after": ece(conf_c[mask].tolist(), ok.tolist(), n_bins=ECE_BINS),
                          "risk_at_080_before": tier_risk_at(conf_r[mask], ok),
                          "risk_at_080_after": tier_risk_at(conf_c[mask], ok)}
    return out


def h7b(raw: Answers, cal: Answers, single, fam, n_boot: int, seed: int) -> dict:
    m = single
    out = {}
    for tier, conf, y, mask in ((3, cal.c3, raw.y3, m), (4, cal.c4, raw.y4, m & raw.m4)):
        ok, lang = y[mask] == raw.gold[mask], raw.lang[mask]
        gap = ece_gap(conf[mask], ok, lang)
        boots = bootstrap(ece_gap, conf[mask], ok, lang, n_boot=n_boot, seed=seed, clusters=fam[mask])
        out[str(tier)] = {"ece_en": ece(conf[mask][lang == "en"].tolist(), ok[lang == "en"].tolist(), n_bins=ECE_BINS),
                          "ece_hi_en": ece(conf[mask][lang == "hi-en"].tolist(), ok[lang == "hi-en"].tolist(),
                                           n_bins=ECE_BINS),
                          "gap": gap, "ci95": list(percentile_ci(boots)), "p_above": p_above(boots),
                          "n": {"en": int((lang == "en").sum()), "hi-en": int((lang == "hi-en").sum())}}
    return {"tier3": out["3"] | {"p": out["3"]["p_above"], "family": "secondary (Holm)",
                                 "claim": "Tier 3 ECE(hi-en) > ECE(en)"},
            "tier4": out["4"] | {"family": "reported, not tested"}}


def analyze(recs, meta: dict, cfg: dict, *, dataset: str, n_boot: int = N_BOOT, seed: int = 0) -> dict:
    tiers, oos = DATASETS[dataset][0], OOS[dataset]
    classes = label_classes(meta, dataset)
    raw, cal = answers(recs, classes, cfg, oos)
    fam = raw.family if dataset in CLUSTERS else None
    n_head = len(meta["tier3"]["classes"])
    single = ~raw.compound & (raw.gold >= 0) & (raw.gold < n_head)
    out = {"dataset": dataset, "n_items": len(recs), "n_single": int(single.sum()), "tiers": list(tiers),
           "clusters": CLUSTERS.get(dataset), "calibration": cfg["calibration"],
           "h2": h2(raw, cal, tiers, fam, n_boot, seed), "table_r2": tier_table(raw, cal, single, cfg)}
    if dataset in CLUSTERS:
        out["h7b"] = h7b(raw, cal, single, raw.family, n_boot, seed)
    return _clean(out)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", required=True, choices=sorted(OOS))
    ap.add_argument("--split", default="val", choices=["val", "test"])
    ap.add_argument("--n-boot", type=int, default=N_BOOT)
    args = ap.parse_args(argv)
    cfg_path = ROOT / "experiments" / "configs" / f"router_{args.dataset}.json"
    cfg = json.loads(cfg_path.read_text())
    recs, meta = load(args.dataset, args.split)
    if args.split == "test" and meta.get("config_sha256") != cfg["config_sha256"]:
        raise SystemExit("the test tier outputs were not made with the frozen config")
    out = {"experiment": "E2", "split": args.split, "config": str(cfg_path.relative_to(ROOT)),
           "config_sha256": cfg["config_sha256"]} | analyze(recs, meta, cfg, dataset=args.dataset, n_boot=args.n_boot)
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    path = OUT_DIR / f"analysis_{args.dataset}_{args.split}.json"
    path.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
    h = out["h2"]
    for k in ("risk_at_080", "aurc"):
        v = h[k]
        print(f"{k}: R2 {v['R2']:.4f}  R2c {v['R2c']:.4f}  diff {v['diff']:+.4f} [{v['ci95'][0]:+.4f}, "
              f"{v['ci95'][1]:+.4f}]  p_below {v['p_below']:.4f}")
    for t, v in out["table_r2"].items():
        print(f"Tier {t}: n {v['n']} acc {v['accuracy']:.3f} T {v['T']:.3f} ECE {v['ece_before']:.4f} -> "
              f"{v['ece_after']:.4f}  risk@0.8 {v['risk_at_080_before']:.4f} -> {v['risk_at_080_after']:.4f}")
    if "h7b" in out:
        v = out["h7b"]["tier3"]
        print(f"H7b Tier 3: ECE en {v['ece_en']:.4f} hi-en {v['ece_hi_en']:.4f} gap {v['gap']:+.4f} "
              f"[{v['ci95'][0]:+.4f}, {v['ci95'][1]:+.4f}] p_above {v['p_above']:.4f}")
    print(f"-> {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
