"""experiments/router/policies.py -- routing policies simulated on the logged tier outputs (Week 7, Ch. 13.2-13.4).

    python experiments/router/policies.py [--dataset clinc150]   # reads tier_outputs_<dataset>.jsonl

Every policy is a function of the per-item tier outputs logged once by tier_outputs.py:
- R0 always Tier 4; R1 Tier 3 alone with a threshold;
- R2 cascade 1 -> 3 -> 4 on raw confidences (rule match = 1.0, T = 1) with one shared threshold;
- R2c the same cascade on calibrated confidences with one shared threshold (D9);
- R3 calibrated, per-tier thresholds by grid search (Ch. 13.4); R-rand random escalation at R3's
  per-tier acceptance counts, expectation over seeds (D10).
Calibration is fitted on cal: Tier-1 confidence = the rules' precision on cal, Tier-3 T = the head's
(NLL on cal), Tier-4 T by BCE on cal. Thresholds are chosen on val, and the numbers reported on val
are therefore optimistic; `crossfit` estimates by how much (2 folds by template family).

Scoring (PREREG §3): in scope, right iff the label matches (abstaining is wrong); `unsupported`
(the OOS label), right iff abstained or labelled unsupported; compound (ADR-010), right iff not
accepted as a single intent below Tier 4. With no budget cut-off, every request that is not
accepted below Tier 4 reaches Tier 4, so for compounds "not accepted below Tier 4" = "reached Tier 4".

Cost = the sum of the measured dev times of the tiers a request consulted (Tier 1 us, Tier-3
embedding, Tier-4 classify with the prefix cached). Dev timing as found, not E1.
"""
from __future__ import annotations

import argparse
import gzip
import json
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
for sub in ("analysis", "tier3", "tier4"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from b3_dev import fit_conf_temperature, step_confidence  # noqa: E402
from metrics import ece  # noqa: E402

LOG_DIR = ROOT / "experiments" / "logs" / "router"
TIER_OUTPUTS = LOG_DIR / "tier_outputs_intents_v0.jsonl"
# Per dataset: the tiers the cascade uses and the OOS label. The Kestrel rules cannot answer with a
# CLINC150 label, so the CLINC150 cascade is Tier 3 -> Tier 4 (its conjunction flag is still read).
DATASETS = {"intents_v0": ((1, 3, 4), "unsupported"), "clinc150": ((3, 4), "oos")}
DELTAS = (0.0, 0.01, 0.02, 0.03, 0.05)
SIGNALS = (None, "conjunction", "ambiguous")
COVERAGE = 0.80  # D4
ECE_BINS = 15
OFF = np.inf  # a threshold no confidence reaches: the tier never accepts


def read_tier_outputs(dataset: str, log_dir: Path = LOG_DIR):
    """(records, meta, path) of tier_outputs_<dataset>, plain .jsonl or gzipped .jsonl.gz. A large
    log (CLINC150: 21 MB) is committed gzipped; the plain file stays local."""
    meta = json.loads((log_dir / f"tier_outputs_{dataset}.meta.json").read_text())
    plain = log_dir / f"tier_outputs_{dataset}.jsonl"
    for path, opener in ((plain, open), (plain.with_name(plain.name + ".gz"), gzip.open)):
        if path.exists():
            with opener(path, "rt") as f:
                return [json.loads(l) for l in f], meta, path
    raise FileNotFoundError(f"{plain}[.gz]")


@dataclass
class Answers:
    """Per-item arrays: each tier's top label (class index, -1 = none) and confidence, the measured
    costs (ms), the gold label (-2 for compound) and the routing signals."""
    gold: np.ndarray
    compound: np.ndarray
    oos: np.ndarray
    conj: np.ndarray
    amb: np.ndarray
    lang: np.ndarray
    family: np.ndarray
    m1: np.ndarray
    y1: np.ndarray
    c1: np.ndarray
    y3: np.ndarray
    c3: np.ndarray
    m4: np.ndarray
    y4: np.ndarray
    c4: np.ndarray
    k1: np.ndarray
    k3: np.ndarray
    k4: np.ndarray
    oos_idx: int

    @classmethod
    def build(cls, recs, classes, *, rule_conf: float, t3: float, t4: float,
              oos_label: str = "unsupported") -> "Answers":
        """`classes`: the Tier-3 head's classes, plus the OOS label at the end when the head has no
        OOS class (CLINC150), so Tier 4's `oos` answer has an index. A Tier-1 match on a label
        outside `classes` (Kestrel rules on CLINC150) counts as no opinion."""
        idx = {c: i for i, c in enumerate(classes)}
        z = np.array([r["t3"]["logits"] for r in recs], dtype=np.float64) / t3
        p3 = np.exp(z - z.max(axis=1, keepdims=True))
        p3 /= p3.sum(axis=1, keepdims=True)
        m1 = np.array([r["t1"]["outcome"] == "match" and r["t1"]["intent"] in idx for r in recs])
        m4 = np.array(["label" in r["t4"] for r in recs])
        return cls(
            gold=np.array([-2 if r["compound"] else idx[r["gold"]] for r in recs]),
            compound=np.array([r["compound"] for r in recs]),
            oos=np.array([r["gold"] == oos_label for r in recs]),
            conj=np.array([r["t1"]["conjunction"] for r in recs]),
            amb=np.array([r["t1"]["outcome"] == "ambiguous" for r in recs]),
            lang=np.array([r["lang"] for r in recs]),
            family=np.array([r.get("family", r["id"].split("/")[0]) for r in recs]),
            m1=m1,
            y1=np.array([idx[r["t1"]["intent"]] if ok else -1 for r, ok in zip(recs, m1)]),
            c1=np.where(m1, rule_conf, 0.0),
            y3=p3.argmax(axis=1),
            c3=p3.max(axis=1),
            m4=m4,
            y4=np.array([idx[r["t4"]["label"]] if ok else -1 for r, ok in zip(recs, m4)]),
            c4=np.array([step_confidence(r["t4"]["steps"], t4) if ok else 0.0 for r, ok in zip(recs, m4)]),
            k1=np.array([r["t1"]["us"] / 1e3 for r in recs]),
            k3=np.array([r["t3"]["ms"] for r in recs]),
            k4=np.array([r["t4"].get("ms", 0.0) for r in recs]),
            oos_idx=idx.get(oos_label, -3),
        )

    def subset(self, mask) -> "Answers":
        return Answers(**{k: (v[mask] if isinstance(v, np.ndarray) else v) for k, v in self.__dict__.items()})


@dataclass
class Routed:
    pred: np.ndarray  # class index, -1 = abstained
    tier: np.ndarray  # tier where routing ended (accepted, or the last one consulted)
    cost: np.ndarray
    flag: np.ndarray  # sent straight to Tier 4 by the routing signal
    pool1: np.ndarray  # Tier 1 had an opinion and was allowed to accept
    acc1: np.ndarray
    reach3: np.ndarray
    acc3: np.ndarray
    reach4: np.ndarray
    acc4: np.ndarray
    use: tuple


def _signal(a: Answers, signal):
    return {None: np.zeros(len(a.gold), bool), "conjunction": a.conj, "ambiguous": a.amb}[signal]


def route(a: Answers, tau, tiers=(1, 3, 4), signal=None) -> Routed:
    """The cascade over logged outputs: same policy as kestrel_router::Cascade (no budget). A flagged
    request (signal) skips every tier below Tier 4 and pays nothing for them (ADR-017)."""
    use1, use3, use4 = (k in tiers for k in (1, 3, 4))
    flag = _signal(a, signal)
    if flag.any() and not use4:
        raise ValueError("a routing signal needs Tier 4")
    pool1 = use1 & a.m1 & ~flag
    acc1 = pool1 & (a.c1 >= tau[0])
    return _finish(a, acc1, pool1, flag, (use1, use3, use4), lambda reach3: reach3 & (a.c3 >= tau[1]),
                   lambda reach4: reach4 & a.m4 & (a.c4 >= tau[2]))


def _finish(a, acc1, pool1, flag, use, accept3, accept4) -> Routed:
    use1, use3, use4 = use
    reach3 = use3 & ~acc1 & ~flag
    acc3 = accept3(reach3)
    reach4 = use4 & ~acc1 & ~acc3
    acc4 = accept4(reach4)
    pred = np.full(len(a.gold), -1)
    pred[acc1], pred[acc3], pred[acc4] = a.y1[acc1], a.y3[acc3], a.y4[acc4]
    tier = np.where(reach4, 4, np.where(reach3, 3, np.where(use1, 1, 0)))
    cost = (use1 & ~flag) * a.k1 + reach3 * a.k3 + reach4 * a.k4
    return Routed(pred, tier, cost, flag, pool1, acc1, reach3, acc3, reach4, acc4, use)


def correct(a: Answers, pred, tier) -> np.ndarray:
    accepted_below_4 = (pred != -1) & (tier < 4)
    oos_ok = (pred == -1) | (pred == a.oos_idx)
    return np.where(a.compound, ~accepted_below_4, np.where(a.oos, oos_ok, pred == a.gold))


def rule_precision(recs) -> tuple[float | None, int]:
    """Precision of Tier-1 matches (a match on a compound request is wrong): its calibrated confidence."""
    hits = [r["t1"]["intent"] == r["gold"] for r in recs if r["t1"]["outcome"] == "match"]
    return (sum(hits) / len(hits) if hits else None), len(hits)


def best_point(points: list[dict], floor: float):
    """The cheapest point with accuracy >= floor (ties: higher accuracy), or None."""
    ok = [p for p in points if p["acc"] >= floor - 1e-12]
    return min(ok, key=lambda p: (p["cost"], -p["acc"])) if ok else None


def r_rand(a: Answers, det: Routed, seeds: int = 1000) -> dict:
    """D10: at each tier accept a uniformly random set of the requests it could accept, as many as
    `det` accepted there; everything else escalates as in `det`. Means over seeds."""
    n1, n3, n4 = int(det.acc1.sum()), int(det.acc3.sum()), int(det.acc4.sum())
    acc, cost, t4 = [], [], []
    for s in range(seeds):
        rng = np.random.default_rng(s)

        def pick(pool, n):
            out = np.zeros(len(pool), bool)
            idx = np.flatnonzero(pool)
            out[rng.choice(idx, size=min(n, len(idx)), replace=False)] = True
            return out

        acc1 = pick(det.pool1, n1)
        r = _finish(a, acc1, det.pool1, det.flag, det.use, lambda reach3: pick(reach3, n3),
                    lambda reach4: pick(reach4 & a.m4, n4))
        acc.append(correct(a, r.pred, r.tier).mean())
        cost.append(r.cost.mean())
        t4.append((r.tier == 4).mean())
    return {"seeds": seeds, "acc": float(np.mean(acc)), "acc_sd": float(np.std(acc)), "cost": float(np.mean(cost)),
            "tier4_rate": float(np.mean(t4))}


def summary(a: Answers, r: Routed, tau=None) -> dict:
    ok = correct(a, r.pred, r.tier)
    answered = r.pred != -1
    out = {"acc": float(ok.mean()), "cost": float(r.cost.mean()), "cost_p95": float(np.percentile(r.cost, 95)),
           "coverage": float(answered.mean()),
           "risk": float(1 - ok[answered].mean()) if answered.any() else None,
           "tier_rate": {str(k): float((r.tier == k).mean()) for k in (1, 3, 4)}}
    if tau is not None:  # a tier left out of the cascade is "off" whatever tau says
        out["tau"] = [None if t == OFF or not used else float(t) for t, used in zip(tau, r.use)]
    return out


def by_lang(a: Answers, r: Routed) -> dict:
    ok = correct(a, r.pred, r.tier)
    return {lang: {"n": int(m.sum()), "acc": float(ok[m].mean()), "tier4_rate": float((r.tier[m] == 4).mean()),
                   "cost": float(r.cost[m].mean())}
            for lang in sorted(set(a.lang)) for m in [a.lang == lang]}


def _grid(c):
    """Every threshold that gives a different outcome: accept all (0), accept none (OFF), and the
    midpoints between neighbouring observed confidences. Midpoints keep each chosen threshold away
    from every val confidence, so the Rust router's f32 arithmetic decides the same way."""
    u = np.unique(c)
    return np.r_[0.0, (u[:-1] + u[1:]) / 2, OFF]


def sweep_shared(a: Answers, signal=None, last_answers=False, tiers=(1, 3, 4)) -> list[dict]:
    """One threshold (R2, R2c) over every value where the outcome can change. By default it applies
    to every tier, Tier 4 included (abstention: the risk-coverage curve of H2). With `last_answers`
    it decides escalation only and Tier 4 always answers, as R3's grid chooses (tau4 = 0)."""
    c1 = a.c1[a.m1] if 1 in tiers else np.array([])
    if last_answers:
        taus = _grid(np.r_[c1, a.c3])
        return [summary(a, route(a, (t, t, 0.0), tiers, signal), (t, t, 0.0)) for t in taus]
    taus = _grid(np.r_[c1, a.c3, a.c4[a.m4]])
    return [summary(a, route(a, (t, t, t), tiers, signal), (t, t, t)) for t in taus]


def sweep_per_tier(a: Answers, signal=None, tiers=(1, 3, 4)) -> list[dict]:
    """Per-tier thresholds (R3). Tier-1 confidence is one constant, so tau1 is on or off."""
    t1s = (0.0, OFF) if 1 in tiers else (OFF,)
    return [summary(a, route(a, (t1, t3, t4), tiers, signal), (t1, t3, t4))
            for t1 in t1s for t3 in _grid(a.c3) for t4 in _grid(a.c4[a.m4])]


def best_per_tier(a: Answers, floors: dict, signal=None, tiers=(1, 3, 4), coverage: float = COVERAGE):
    """The choices of `best_point(sweep_per_tier(...), floor)` for every floor, and of
    `risk_at_coverage(sweep_per_tier(...))`, without simulating each grid point: CLINC150's grid
    (~1700 x 1700 thresholds over 1700 items) is too large for that.

    For fixed tau1 and tau4, raising tau3 only moves Tier-3 candidates, least confident first,
    from Tier 3 to Tier 4, so correct / answered / cost for every tau3 are prefix sums over the
    candidates sorted by confidence. Ties are broken as the full grid's order does (tau1, tau3,
    tau4), and each chosen point is summarized by an exact `route`."""
    use1 = 1 in tiers
    if not (3 in tiers and 4 in tiers):
        raise ValueError("best_per_tier needs Tier 3 and Tier 4")
    n = len(a.gold)
    flag = _signal(a, signal)
    t1s = (0.0, OFF) if use1 else (OFF,)
    g3, g4 = _grid(a.c3), _grid(a.c4[a.m4])
    ok1 = correct(a, a.y1, np.full(n, 1))
    ok3 = correct(a, a.y3, np.full(n, 3))
    k1 = ((use1 & ~flag) * a.k1).sum()
    cs = lambda x: np.r_[0.0, np.cumsum(x)]  # noqa: E731
    best = {d: None for d in floors}  # d -> (key, tau)
    cov_best = None
    for i1, t1 in enumerate(t1s):
        acc1 = use1 & a.m1 & ~flag & (a.c1 >= t1)
        cand = ~acc1 & ~flag  # reach Tier 3
        fixed4 = ~acc1 & flag  # sent straight to Tier 4
        order = np.flatnonzero(cand)[np.argsort(-a.c3[cand], kind="stable")]
        j = np.searchsorted(-a.c3[order], -g3, side="right")  # candidates with c3 >= tau3, per tau3
        cost = (k1 + a.k3[cand].sum() + a.k4[fixed4].sum() + a.k4[order].sum() - cs(a.k4[order])[j]) / n
        for i4, t4 in enumerate(g4):
            pred4 = np.where(a.m4 & (a.c4 >= t4), a.y4, -1)
            ok4 = correct(a, pred4, np.full(n, 4))
            ans4 = pred4 != -1
            ca4 = ok4 & ans4
            n_ok = (ok1[acc1].sum() + ok4[fixed4].sum() + ok4[order].sum() + cs(ok3[order] & 1)[j]
                    - cs(ok4[order] & 1)[j])
            n_ans = acc1.sum() + ans4[fixed4].sum() + ans4[order].sum() + j - cs(ans4[order] & 1)[j]
            n_ca = ((ok1 & acc1).sum() + ca4[fixed4].sum() + ca4[order].sum() + cs(ok3[order] & 1)[j]
                    - cs(ca4[order] & 1)[j])
            acc = n_ok / n
            for d, floor in floors.items():
                ok = np.flatnonzero(acc >= floor - 1e-12)
                if len(ok):
                    i3 = ok[np.lexsort((ok, -acc[ok], cost[ok]))[0]]
                    key = (cost[i3], -acc[i3], i1, i3, i4)
                    if best[d] is None or key < best[d][0]:
                        best[d] = (key, (t1, g3[i3], t4))
            cov = n_ans / n
            ok = np.flatnonzero((cov >= coverage) & (n_ans > 0))
            if len(ok):
                risk = 1 - n_ca[ok] / n_ans[ok]
                k = np.lexsort((ok, risk, cov[ok]))[0]
                key = (cov[ok[k]], risk[k], i1, ok[k], i4)
                if cov_best is None or key < cov_best[0]:
                    cov_best = (key, (t1, g3[ok[k]], t4))
    points = {d: None if b is None else summary(a, route(a, b[1], tiers, signal), b[1]) for d, b in best.items()}
    risk = None
    if cov_best is not None:
        p = summary(a, route(a, cov_best[1], tiers, signal), cov_best[1])
        risk = {"coverage": p["coverage"], "risk": p["risk"], "tau": p["tau"]}
    return points, risk


def risk_at_coverage(points: list[dict], coverage: float = COVERAGE):
    """Selective risk of the point with the smallest coverage >= `coverage` (D4)."""
    ok = [p for p in points if p["coverage"] >= coverage and p["risk"] is not None]
    if not ok:
        return None
    p = min(ok, key=lambda p: (p["coverage"], p["risk"]))
    return {"coverage": p["coverage"], "risk": p["risk"], "tau": p["tau"]}


def _tau(p):
    return tuple(OFF if t is None else t for t in p["tau"])


def crossfit(a: Answers, r0_acc_of, delta: float, signal=None, tiers=(1, 3, 4)) -> dict:
    """Thresholds chosen on one half of val (split by template family), scored on the other, both ways."""
    fams = np.unique(a.family)
    rng = np.random.default_rng(0)
    half = set(rng.permutation(fams)[: len(fams) // 2])
    fold = np.array([f in half for f in a.family])
    ok, cost, n = 0.0, 0.0, 0
    for train in (fold, ~fold):
        at, ae = a.subset(train), a.subset(~train)
        p = best_per_tier(at, {delta: r0_acc_of(at) - delta}, signal, tiers)[0][delta]
        r = route(ae, _tau(p), tiers, signal)
        ok += correct(ae, r.pred, r.tier).sum()
        cost += r.cost.sum()
        n += len(ae.gold)
    return {"acc": ok / n, "cost": cost / n}


def run(recs, classes, t3_cal: float, oos_label: str = "unsupported", tiers=(1, 3, 4)) -> dict:
    """`classes`: head classes, plus `oos_label` at the end if the head lacks it."""
    cal = [r for r in recs if r["split"] == "cal"]
    val = [r for r in recs if r["split"] == "val"]
    p_rule, n_rule = rule_precision(cal) if 1 in tiers else (None, 0)
    single_cal = [r for r in cal if not r["compound"] and "steps" in r["t4"]]
    t4_cal = fit_conf_temperature([r["t4"]["steps"] for r in single_cal],
                                  [r["t4"]["label"] == r["gold"] for r in single_cal])
    build = lambda **kw: Answers.build(val, classes, oos_label=oos_label, **kw)  # noqa: E731
    raw = build(rule_conf=1.0, t3=1.0, t4=1.0)
    cal_ = build(rule_conf=p_rule if p_rule is not None else 1.0, t3=t3_cal, t4=t4_cal)

    def r0(a):
        return route(a, (OFF, OFF, 0.0), tiers=(4,))

    def r0_acc(a):
        r = r0(a)
        return float(correct(a, r.pred, r.tier).mean())

    # Per-tier quality on items whose gold label is a head class (CLINC150's OOS items have none).
    n_head = len(val[0]["t3"]["logits"])
    single = ~raw.compound & (raw.gold >= 0) & (raw.gold < n_head)
    ece_of = lambda c, y: float(ece(list(c[single]), list((y == raw.gold)[single]), n_bins=ECE_BINS))  # noqa: E731
    out = {
        "tiers": list(tiers), "oos_label": oos_label, "n_val": len(val), "n_cal": len(cal),
        "calibration": {"rule_precision_cal": p_rule, "rule_matches_cal": n_rule,
                        "rule_precision_val": rule_precision(val)[0], "rule_matches_val": rule_precision(val)[1],
                        "t3": t3_cal, "t4": t4_cal},
        "tiers_val_single": {
            "t1_coverage": float(raw.m1[single].mean()),
            "t3": {"acc": float((raw.y3 == raw.gold)[single].mean()), "ece_raw": ece_of(raw.c3, raw.y3),
                   "ece_cal": ece_of(cal_.c3, cal_.y3)},
            "t4": {"acc": float((raw.y4 == raw.gold)[single].mean()), "ece_raw": ece_of(raw.c4, raw.y4),
                   "ece_cal": ece_of(cal_.c4, cal_.y4)}},
        "cost_ms_mean": {"t1": float(raw.k1.mean()), "t3": float(raw.k3.mean()), "t4": float(raw.k4.mean())},
        "signals": {s: {"compound_flagged": int(_signal(raw, s)[raw.compound].sum()),
                        "single_flagged": int(_signal(raw, s)[~raw.compound].sum()),
                        "n_compound": int(raw.compound.sum())} for s in SIGNALS if s},
    }
    rr0 = r0(raw)
    out["R0"] = summary(raw, rr0) | {"by_lang": by_lang(raw, rr0)}
    r1 = [summary(cal_, route(cal_, (OFF, t, OFF), tiers=(3,)), (OFF, t, OFF)) for t in _grid(cal_.c3)]
    out["R1"] = {"always_answer": r1[0], "risk_at_coverage": risk_at_coverage(r1)}
    floor = {d: out["R0"]["acc"] - d for d in DELTAS}
    for name, a in (("R2", raw), ("R2c", cal_)):
        out[name] = {"risk_at_coverage": risk_at_coverage(sweep_shared(a, tiers=tiers)), "points": {}}
        for s in SIGNALS[:2]:
            pts = sweep_shared(a, s, last_answers=True, tiers=tiers)
            out[name]["points"][str(s)] = {str(d): best_point(pts, floor[d]) for d in DELTAS}
    out["R3"] = {}
    for s in SIGNALS:
        points, risk = best_per_tier(cal_, floor, s, tiers)
        res = {"risk_at_coverage": risk, "points": {}}
        for d in DELTAS:
            p = points[d]
            if p is None:
                res["points"][str(d)] = None
                continue
            r = route(cal_, _tau(p), tiers, s)
            res["points"][str(d)] = p | {"by_lang": by_lang(cal_, r), "r_rand": r_rand(cal_, r),
                                         "crossfit": crossfit(cal_, r0_acc, d, s, tiers)}
        out["R3"][str(s)] = res
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", default="intents_v0", choices=sorted(DATASETS))
    args = ap.parse_args(argv)
    tiers, oos_label = DATASETS[args.dataset]
    recs, meta, outputs = read_tier_outputs(args.dataset)
    classes = meta["tier3"]["classes"]
    classes = classes if oos_label in classes else classes + [oos_label]
    out = {"dataset": args.dataset} | run(recs, classes, meta["tier3"]["temperature_cal"], oos_label, tiers)
    out["inputs"] = {"tier_outputs": outputs.name, "git_head_at_logging": meta["git_head"]}
    path = LOG_DIR / f"policies_{args.dataset}.json"
    path.write_text(json.dumps(out, indent=1) + "\n")
    print(json.dumps({k: v for k, v in out.items() if k not in ("R2", "R2c", "R3")}, indent=1))
    for name in ("R2", "R2c"):
        print(name, "risk at coverage", json.dumps(out[name]["risk_at_coverage"]))
        for s, pts in out[name]["points"].items():
            for d, p in pts.items():
                if p:
                    print(f"{name} signal={s} delta={d}: acc {p['acc']:.3f} cost {p['cost']:.0f} ms "
                          f"t4 {p['tier_rate']['4']:.2f} tau {p['tau'][1]}")
    for s, res in out["R3"].items():
        for d, p in res["points"].items():
            if p:
                print(f"R3 signal={s} delta={d}: acc {p['acc']:.3f} cost {p['cost']:.0f} ms tau {p['tau']} "
                      f"t4 {p['tier_rate']['4']:.2f} | r_rand acc {p['r_rand']['acc']:.3f} cost "
                      f"{p['r_rand']['cost']:.0f} | crossfit acc {p['crossfit']['acc']:.3f} cost "
                      f"{p['crossfit']['cost']:.0f}")


if __name__ == "__main__":
    main()
