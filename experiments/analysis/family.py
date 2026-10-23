"""experiments/analysis/family.py -- Holm over the secondary family and every hypothesis's outcome (ADR-029, ADR-032).

    python experiments/analysis/family.py     # reads the E1, E2, E3, E5 test analyses

The family (PREREG §6, clarification v1.0.1): H1 on Intents-v0; H1b, H1c and H2 on each dataset; H3, H4, H5,
H6, H7a, H7b: 13 members, Holm at 0.05. Each member's p, effect and CI come from its experiment's analysis
log; H1 on CLINC150 is the primary and stays outside (its outcome is the CI rule). A member without its p yet
(H4 and H6 until their amendment) enters with p = 1: Holm's adjusted p cannot rise when another p falls, so an
interim "supported" stays supported, while an interim "inconclusive" may still change. Outcomes (ADR-029):
supported = Holm-adjusted p < 0.05 with the effect on the claimed side; not supported = the 95% CI wholly on
the other side; inconclusive otherwise; a claim with parts combines them. Out: experiments/logs/family_v1.json.
"""
from __future__ import annotations

import json
from pathlib import Path

from kestrel_stats import combine, holm, outcome

ROOT = Path(__file__).resolve().parents[2]
LOGS = ROOT / "experiments" / "logs"
NAME = {"intents_v0": "Intents-v0", "clinc150": "CLINC150"}
MEMBERS = ("H1 Intents-v0", "H1b CLINC150", "H1b Intents-v0", "H1c CLINC150", "H1c Intents-v0", "H2 CLINC150",
           "H2 Intents-v0", "H3", "H4", "H5", "H6", "H7a", "H7b")


def _part(c: dict, *, above: bool, bound: float, key: str = "diff") -> dict:
    return {"diff": c[key], "ci95": c["ci95"], "above": above, "bound": bound}


def members(e1: dict, e2: dict, e3: dict, e5: dict) -> list[dict]:
    """Every member with its p and its parts (effect, CI, claimed side), or p None when not run yet."""
    out = {m: {"id": m, "p": None, "parts": []} for m in MEMBERS}

    def put(mid, p, *parts, source):
        out[mid].update(p=p, parts=list(parts), source=source)

    if "intents_v0" in e1:
        r = e1["intents_v0"]
        d = r["delta"]
        put("H1 Intents-v0", r["h1"]["p_iut"], _part(r["h1"]["accuracy"], above=True, bound=-d),
            _part(r["h1"]["latency"], above=False, bound=0.0, key="diff_ms"), source="E1 Intents-v0")
        put("H7a", r["h7a"]["accuracy"]["p_noninferior"], _part(r["h7a"]["accuracy"], above=True, bound=-d),
            source="E1 Intents-v0 hi-en")
    for ds, r in e1.items():
        put(f"H1b {NAME[ds]}", r["h1b"]["p"], _part(r["h1b"]["accuracy"], above=True, bound=0.0),
            source=f"E1 {NAME[ds]}")
        put(f"H1c {NAME[ds]}", r["h1c"]["p"], _part(r["h1c"]["accuracy"], above=True, bound=-r["delta"]),
            source=f"E1 {NAME[ds]}")
    for ds, r in e2.items():
        put(f"H2 {NAME[ds]}", r["h2"]["p"], _part(r["h2"]["risk_at_080"], above=False, bound=0.0),
            source=f"E2 {NAME[ds]}")
        if "h7b" in r:
            put("H7b", r["h7b"]["tier3"]["p"], _part(r["h7b"]["tier3"], above=True, bound=0.0, key="gap"),
                source=f"E2 {NAME[ds]}")
    if "intents_v0" in e3:
        r = e3["intents_v0"]
        put("H3", r["h3"]["p"], _part(r["h3"]["accuracy"], above=True, bound=-r["delta"]),
            _part(r["h3"]["cost"], above=False, bound=0.0, key="diff_ms"), source="E3 Intents-v0")
    if e5:
        put("H5", e5["h5"]["p"], _part(e5["h5"], above=False, bound=0.0), source="E5 CLINC150")
    return [out[m] for m in MEMBERS]


def decide(ms: list[dict], alpha: float = 0.05) -> list[dict]:
    """Holm over the family (pending members at p = 1) and each member's outcome."""
    adj, reject = holm([1.0 if m["p"] is None else m["p"] for m in ms], alpha)
    pending = any(m["p"] is None for m in ms)
    out = []
    for m, a, rej in zip(ms, adj, reject):
        if m["p"] is None:
            out.append(m | {"p_holm": None, "outcome": "pending"})
            continue
        verdict = combine(*(outcome(bool(rej), p["diff"], p["ci95"], above=p["above"], bound=p["bound"])
                            for p in m["parts"]))
        if verdict == "inconclusive" and pending:
            verdict = "inconclusive (interim)"
        out.append(m | {"p_holm": float(a), "outcome": verdict})
    return out


def _load(path: Path):
    return json.loads(path.read_text()) if path.exists() else None


def main() -> int:
    e1 = {ds: r for ds in NAME if (r := _load(LOGS / "E1" / f"analysis_{ds}_test.json"))}
    e2 = {ds: r for ds in NAME if (r := _load(LOGS / "E2" / f"analysis_{ds}_test.json"))}
    e3 = {ds: r for ds in NAME if (r := _load(LOGS / "E3" / f"analysis_{ds}_test.json"))}
    e5 = _load(LOGS / "E5" / "analysis_test.json") or {}
    ms = decide(members(e1, e2, e3, e5))
    primary = e1.get("clinc150", {}).get("h1")
    report = {"family": "PREREG §6 + clarification v1.0.1 (ADR-032)", "alpha": 0.05, "members": ms,
              "primary_h1_clinc150": {"outcome": primary["outcome"], "p_iut": primary["p_iut"]} if primary else None}
    (LOGS / "family_v1.json").write_text(json.dumps(report, indent=1) + "\n")
    for m in ms:
        p = "—" if m["p"] is None else f"{m['p']:.2g}"
        ph = "—" if m["p_holm"] is None else f"{m['p_holm']:.2g}"
        print(f"{m['id']:15s} p {p:>8s}  Holm {ph:>8s}  {m['outcome']}")
    print("H1 CLINC150 (primary):", report["primary_h1_clinc150"] or "not run yet")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
