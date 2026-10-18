"""experiments/router/policies.py -- routing policies simulated on the logged tier outputs (Week 7, Ch. 13.2-13.4).

    python experiments/router/policies.py      # seconds; reads tier_outputs_intents_v0.jsonl

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
OOS_LABEL = "unsupported"
DELTAS = (0.0, 0.01, 0.02, 0.03, 0.05)
SIGNALS = (None, "conjunction", "ambiguous")
COVERAGE = 0.80  # D4
ECE_BINS = 15
OFF = np.inf  # a threshold no confidence reaches: the tier never accepts


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
    def build(cls, recs, classes, *, rule_conf: float, t3: float, t4: float) -> "Answers":
        idx = {c: i for i, c in enumerate(classes)}
        z = np.array([r["t3"]["logits"] for r in recs], dtype=np.float64) / t3
        p3 = np.exp(z - z.max(axis=1, keepdims=True))
        p3 /= p3.sum(axis=1, keepdims=True)
        m1 = np.array([r["t1"]["outcome"] == "match" for r in recs])
        m4 = np.array(["label" in r["t4"] for r in recs])
        return cls(
            gold=np.array([-2 if r["compound"] else idx[r["gold"]] for r in recs]),
            compound=np.array([r["compound"] for r in recs]),
            oos=np.array([r["gold"] == OOS_LABEL for r in recs]),
            conj=np.array([r["t1"]["conjunction"] for r in recs]),
            amb=np.array([r["t1"]["outcome"] == "ambiguous" for r in recs]),
            lang=np.array([r["lang"] for r in recs]),
            family=np.array([r["id"].split("/")[0] for r in recs]),
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
            oos_idx=idx.get(OOS_LABEL, -3),
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
    if tau is not None:
        out["tau"] = [None if t == OFF else float(t) for t in tau]
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


def sweep_shared(a: Answers, signal=None, last_answers=False) -> list[dict]:
    """One threshold (R2, R2c) over every value where the outcome can change. By default it applies
    to every tier, Tier 4 included (abstention: the risk-coverage curve of H2). With `last_answers`
    it decides escalation only and Tier 4 always answers, as R3's grid chooses (tau4 = 0)."""
    if last_answers:
        taus = _grid(np.r_[a.c1[a.m1], a.c3])
        return [summary(a, route(a, (t, t, 0.0), signal=signal), (t, t, 0.0)) for t in taus]
    taus = _grid(np.r_[a.c1[a.m1], a.c3, a.c4[a.m4]])
    return [summary(a, route(a, (t, t, t), signal=signal), (t, t, t)) for t in taus]


def sweep_per_tier(a: Answers, signal=None) -> list[dict]:
    """Per-tier thresholds (R3). Tier-1 confidence is one constant, so tau1 is on or off."""
    return [summary(a, route(a, (t1, t3, t4), signal=signal), (t1, t3, t4))
            for t1 in (0.0, OFF) for t3 in _grid(a.c3) for t4 in _grid(a.c4[a.m4])]


def risk_at_coverage(points: list[dict], coverage: float = COVERAGE):
    """Selective risk of the point with the smallest coverage >= `coverage` (D4)."""
    ok = [p for p in points if p["coverage"] >= coverage and p["risk"] is not None]
    if not ok:
        return None
    p = min(ok, key=lambda p: (p["coverage"], p["risk"]))
    return {"coverage": p["coverage"], "risk": p["risk"], "tau": p["tau"]}


def _tau(p):
    return tuple(OFF if t is None else t for t in p["tau"])


def crossfit(a: Answers, r0_acc_of, delta: float, signal=None) -> dict:
    """Thresholds chosen on one half of val (split by template family), scored on the other, both ways."""
    fams = np.unique(a.family)
    rng = np.random.default_rng(0)
    half = set(rng.permutation(fams)[: len(fams) // 2])
    fold = np.array([f in half for f in a.family])
    ok, cost, n = 0.0, 0.0, 0
    for train in (fold, ~fold):
        at, ae = a.subset(train), a.subset(~train)
        p = best_point(sweep_per_tier(at, signal), r0_acc_of(at) - delta)
        r = route(ae, _tau(p), signal=signal)
        ok += correct(ae, r.pred, r.tier).sum()
        cost += r.cost.sum()
        n += len(ae.gold)
    return {"acc": ok / n, "cost": cost / n}


def run(recs, classes, t3_cal: float) -> dict:
    cal = [r for r in recs if r["split"] == "cal"]
    val = [r for r in recs if r["split"] == "val"]
    p_rule, n_rule = rule_precision(cal)
    single_cal = [r for r in cal if not r["compound"] and "steps" in r["t4"]]
    t4_cal = fit_conf_temperature([r["t4"]["steps"] for r in single_cal],
                                  [r["t4"]["label"] == r["gold"] for r in single_cal])
    raw = Answers.build(val, classes, rule_conf=1.0, t3=1.0, t4=1.0)
    cal_ = Answers.build(val, classes, rule_conf=p_rule if p_rule is not None else 1.0, t3=t3_cal, t4=t4_cal)

    def r0(a):
        return route(a, (OFF, OFF, 0.0), tiers=(4,))

    def r0_acc(a):
        r = r0(a)
        return float(correct(a, r.pred, r.tier).mean())

    single = ~raw.compound
    ece_of = lambda c, y: float(ece(list(c[single]), list((y == raw.gold)[single]), n_bins=ECE_BINS))  # noqa: E731
    out = {
        "n_val": len(val), "n_cal": len(cal),
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
        out[name] = {"risk_at_coverage": risk_at_coverage(sweep_shared(a)), "points": {}}
        for s in SIGNALS[:2]:
            pts = sweep_shared(a, s, last_answers=True)
            out[name]["points"][str(s)] = {str(d): best_point(pts, floor[d]) for d in DELTAS}
    out["R3"] = {}
    for s in SIGNALS:
        pts = sweep_per_tier(cal_, s)
        res = {"risk_at_coverage": risk_at_coverage(pts), "points": {}}
        for d in DELTAS:
            p = best_point(pts, floor[d])
            if p is None:
                res["points"][str(d)] = None
                continue
            r = route(cal_, _tau(p), signal=s)
            res["points"][str(d)] = p | {"by_lang": by_lang(cal_, r), "r_rand": r_rand(cal_, r),
                                         "crossfit": crossfit(cal_, r0_acc, d, s)}
        out["R3"][str(s)] = res
    return out


def main():
    recs = [json.loads(l) for l in TIER_OUTPUTS.read_text().splitlines()]
    meta = json.loads(TIER_OUTPUTS.with_suffix(".meta.json").read_text())
    out = run(recs, meta["tier3"]["classes"], meta["tier3"]["temperature_cal"])
    out["inputs"] = {"tier_outputs": TIER_OUTPUTS.name, "git_head_at_logging": meta["git_head"]}
    path = LOG_DIR / "policies_intents_v0.json"
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
