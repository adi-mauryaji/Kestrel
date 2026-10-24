"""experiments/analysis/errors.py -- failure analysis on the E1 test rows (Week 15, Ch. 43): EXPLORATORY.

    python experiments/analysis/errors.py     # -> experiments/ERROR_ANALYSIS.md, logs/errors_test.json

Written after the test results were read, so nothing here is a pre-registered test (PREREG §6: anything
decided after seeing test data is exploratory). From E1's first-repetition decisions: every R3 error by
scope (in scope / out of scope / compound), the tier where routing ended and whether it answered; the
items where R3 and R1 (CLINC150) or R3 and R0 (Intents-v0) disagree on correctness; the confidence band
that separates R3's Tier-3 acceptance from R1's abstention; the top confusions with example requests; and
every R3 error on Kestrel-Intents-v0 with its text (synthetic data).
"""
from __future__ import annotations

import json
import re
from collections import Counter
from pathlib import Path

from analyze_e1 import OOS, item_correct, load_runs, rep1

ROOT = Path(__file__).resolve().parents[2]
CONFIGS = ROOT / "experiments" / "configs"
OUT_MD = ROOT / "experiments" / "ERROR_ANALYSIS.md"
OUT_JSON = ROOT / "experiments" / "logs" / "errors_test.json"
FEATURES = ROOT / "crates" / "kestrel-core" / "src" / "features.rs"


def _scope(g: dict, oos: str) -> str:
    return "compound" if "|" in g["label"] else "out of scope" if g["label"] == oos else "in scope"


def error_rows(dec, gold_rows, oos: str) -> list[dict]:
    """Every wrong first-repetition decision of one system."""
    return [{"id": g["id"], "lang": g.get("lang"), "text": g.get("text", ""), "gold": g["label"], "pred": r["intent"],
             "tier": r["tier"], "confidence": r["confidence"], "scope": _scope(g, oos)}
            for r, g in zip(dec, gold_rows) if not item_correct(r, g, oos)]


def breakdown(errs) -> list[dict]:
    c = Counter((e["scope"], e["tier"], e["pred"] is not None) for e in errs)
    return [{"scope": s, "tier": t, "answered": a, "n": n} for (s, t, a), n in c.most_common()]


def discordant(dec_a, dec_b, gold_rows, oos: str) -> dict:
    ok_a = [item_correct(r, g, oos) for r, g in zip(dec_a, gold_rows)]
    ok_b = [item_correct(r, g, oos) for r, g in zip(dec_b, gold_rows)]
    return {"a_right_b_wrong": sum(a and not b for a, b in zip(ok_a, ok_b)),
            "a_wrong_b_right": sum(b and not a for a, b in zip(ok_a, ok_b)),
            "both_wrong": sum(not a and not b for a, b in zip(ok_a, ok_b))}


def confusions(errs, k: int = 5) -> list[dict]:
    c = Counter((e["gold"], e["pred"]) for e in errs if e["pred"] is not None)
    out = []
    for (g, p), n in c.most_common(k):
        ex = [e["text"] for e in errs if (e["gold"], e["pred"]) == (g, p)][:2]
        out.append({"gold": g, "pred": p, "n": n, "examples": ex})
    return out


def clinc150() -> dict:
    run = load_runs("clinc150", "test")
    _, dec, gold_rows, _ = rep1(run["rows"], run["gold"])
    oos = OOS["clinc150"]
    errs = error_rows(dec["R3"], gold_rows, oos)
    t3_r3 = json.loads((CONFIGS / "router_clinc150.json").read_text())["accept"][2]
    t3_r1 = json.loads((CONFIGS / "router_clinc150_r1.json").read_text())["accept"][2]
    band = [r for r, g in zip(dec["R3"], gold_rows)
            if g["label"] == oos and r["tier"] == 3 and r["intent"] is not None and r["confidence"] < t3_r1]
    n_oos = sum(g["label"] == oos for g in gold_rows)
    return {"n": len(gold_rows), "n_oos": n_oos, "r3_errors": len(errs), "breakdown": breakdown(errs),
            "r1_vs_r3": discordant(dec["R1"], dec["R3"], gold_rows, oos), "tau3": {"R3": t3_r3, "R1": t3_r1},
            "oos_accepted_in_band": len(band), "oos_errors": sum(e["scope"] == "out of scope" for e in errs),
            "confusions_in_scope": confusions([e for e in errs if e["scope"] == "in scope"]),
            "confusions_oos": confusions([e for e in errs if e["scope"] == "out of scope"])}


def conjunctions() -> list[str]:
    """The routing signal's words (kestrel_core::features::CONJUNCTIONS, ADR-017)."""
    line = next(x for x in FEATURES.read_text().splitlines() if x.startswith("pub const CONJUNCTIONS"))
    return line.split("= &[", 1)[1].rsplit("]", 1)[0].replace('"', "").replace(" ", "").split(",")


def intents_v0() -> dict:
    run = load_runs("intents_v0", "test")
    _, dec, gold_rows, _ = rep1(run["rows"], run["gold"])
    oos = OOS["intents_v0"]
    errs = error_rows(dec["R3"], gold_rows, oos)
    rules = [e for e in errs if e["tier"] == 1]
    missed = [e for e in errs if e["scope"] == "compound" and e["tier"] < 4]
    from splits import load_splits  # on the path through analyze_e1 -> run_e1
    words = set(conjunctions())
    val_comp = [r for r in load_splits("intents_v0")["val"] if "|" in r["label"]]
    unflagged = [r for r in val_comp if not set(re.findall(r"[a-z]+", r["text"].lower())) & words]
    return {"missed_families": sorted({e["id"].split("/")[0] for e in missed}),
            "val_compounds": len(val_comp), "val_compounds_unflagged": len(unflagged),
            "n": len(gold_rows), "r3_errors": errs, "breakdown": breakdown(errs),
            "compound_accepted_below_tier4": len(missed), "conjunctions": conjunctions(),
            "r0_vs_r3": discordant(dec["R0"], dec["R3"], gold_rows, oos), "tier1_errors": len(rules),
            "by_lang": dict(Counter(e["lang"] for e in errs))}


def render(c: dict, i: dict, val_oos_share: float | None) -> str:
    md = ["# Kestrel failure analysis (Week 15, Ch. 43): EXPLORATORY", "",
          "Generated by `experiments/analysis/errors.py` from E1's test rows after the results were read, so nothing "
          "here is a pre-registered test. First-repetition decisions, as scored.", "",
          "## CLINC150: where the cascade loses", "",
          f"R3 makes {c['r3_errors']} errors on {c['n']} items; **{c['oos_errors']} "
          f"({c['oos_errors'] / c['r3_errors']:.0%}) are out-of-scope requests it answered with an in-scope intent**.",
          "",
          "| Scope | Routing ended at | Answered | Errors |", "|---|---|---|---|"]
    md += [f"| {b['scope']} | Tier {b['tier']} | {'yes' if b['answered'] else 'no (abstained)'} | {b['n']} |"
           for b in c["breakdown"]]
    d = c["r1_vs_r3"]
    md += ["", f"R1 (the encoder alone) is right where R3 is wrong on {d['a_right_b_wrong']} items, and the reverse on "
           f"{d['a_wrong_b_right']}. The mechanism is the Tier-3 threshold: R3 accepts Tier 3 at confidence ≥ "
           f"{c['tau3']['R3']:.3f} (the cheapest point on val that meets the accuracy floor, ADR-019/021), while R1 "
           f"abstains below {c['tau3']['R1']:.3f}. {c['oos_accepted_in_band']} out-of-scope test requests fall in that "
           f"band and R3 answers them; R1 does not."]
    if val_oos_share is not None:
        md += [f"Out-of-scope requests are {val_oos_share:.1%} of val but {c['n_oos'] / c['n']:.1%} of test (the "
               "official test keeps its 1000 OOS items), so a threshold chosen for cost on val under-weights them: a "
               "shift in the out-of-scope rate between val and test, not a calibration failure (E2: Tier-3 ECE 0.008 "
               "after T)."]
    md += ["", "Top confusions, out of scope answered as:", "", "| Answered as | n | Examples |", "|---|---|---|"]
    md += [f"| {x['pred']} | {x['n']} | {' · '.join(x['examples'])} |" for x in c["confusions_oos"]]
    md += ["", "Top confusions, in scope:", "", "| Gold | Answered as | n | Examples |", "|---|---|---|---|"]
    md += [f"| {x['gold']} | {x['pred']} | {x['n']} | {' · '.join(x['examples'])} |"
           for x in c["confusions_in_scope"]]
    md += ["", "Remedies to test later (Ch. 43, not run): an out-of-scope-aware floor or a reject band at Tier 3 (the "
           "open \"Tier-3 reject band\" item), choosing R3's point on a val set with the test's OOS share, or an OOS "
           "detector ahead of Tier 3.", "",
           "## Kestrel-Intents-v0 (synthetic): every R3 error", "",
           f"R3 makes {len(i['r3_errors'])} errors on {i['n']} items "
           f"({', '.join(f'{k} {v}' for k, v in i['by_lang'].items())}); {i['tier1_errors']} of them are Tier-1 rule "
           "matches (the rules were accepted at confidence 1.0, their precision on cal).", "",
           f"**{i['compound_accepted_below_tier4']} are compound requests accepted as one intent below Tier 4.** "
           f"The multi-intent signal only knows {', '.join(i['conjunctions'])} (ADR-017), so requests joined by "
           "\"also\" or \", now\" were not flagged, and a single-intent rule took them. This is why the rules cost "
           f"accuracy on test (E3: {{4}} 1.000 vs {{1,4}} 0.976). They come from the families "
           f"{', '.join(i['missed_families'])}; all {i['val_compounds']} val compounds contain a signal word "
           f"({i['val_compounds_unflagged']} unflagged), so tuning could not see the gap. A wider signal is a "
           "post-hoc change: a new, exploratory version.", "",
           "| Item | Lang | Request | Gold | Answered | Tier |", "|---|---|---|---|---|---|"]
    md += [f"| {e['id']} | {e['lang']} | {e['text']} | {e['gold']} | {e['pred'] or '— (abstained)'} | "
           f"{e['tier']} |" for e in i["r3_errors"]]
    d = i["r0_vs_r3"]
    md += ["", f"R0 (Gemma alone) is right where R3 is wrong on {d['a_right_b_wrong']} items and wrong where R3 is "
           f"right on {d['a_wrong_b_right']}.", ""]
    return "\n".join(md) + "\n"


def val_oos_share() -> float:
    from splits import load_splits  # on the path through analyze_e1 -> run_e1
    val = load_splits("clinc150")["val"]
    return sum(r["label"] == OOS["clinc150"] for r in val) / len(val)


def main() -> int:
    c, i = clinc150(), intents_v0()
    share = val_oos_share()
    OUT_JSON.write_text(json.dumps({"exploratory": True, "clinc150": c, "intents_v0": i, "val_oos_share": share},
                                   indent=1, ensure_ascii=False) + "\n")
    OUT_MD.write_text(render(c, i, share))
    print(f"-> {OUT_MD.relative_to(ROOT)}, {OUT_JSON.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
