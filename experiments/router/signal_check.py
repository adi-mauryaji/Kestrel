"""experiments/router/signal_check.py -- how often the multi-intent signal fires, per dataset (ADR-017).

    python experiments/router/signal_check.py      # seconds; Rust rules_items over the dev splits

ADR-017 sends every request with a conjunction token straight to Tier 4, so its rate on
single-intent requests (false positives: extra Tier-4 cost, and Tier 4's accuracy instead of Tier
3's) and on compound requests (recall) is reported per dataset, split and language. Dev splits only.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "tier3"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from splits import load_splits  # noqa: E402
from tier_outputs import run_rules  # noqa: E402

OUT = ROOT / "experiments" / "logs" / "router" / "signal_check.json"


def rates(rows: list[dict], flags: list[bool], oos_label: str) -> dict:
    """Flag counts by kind of item (single in scope, out of scope, compound) and language."""
    out: dict = {}
    for r, f in zip(rows, flags):
        kind = "compound" if "|" in r["label"] else "oos" if r["label"] == oos_label else "single"
        for key in (kind, f"{kind}/{r.get('lang', 'en')}"):
            c = out.setdefault(key, {"n": 0, "flagged": 0})
            c["n"] += 1
            c["flagged"] += bool(f)
    for c in out.values():
        c["rate"] = c["flagged"] / c["n"]
    return dict(sorted(out.items()))


def main():
    res = {}
    for dataset, oos_label in (("intents_v0", "unsupported"), ("clinc150", "oos")):
        splits = load_splits(dataset)
        for split in ("cal", "val"):
            rows = splits[split]
            res[f"{dataset}/{split}"] = rates(rows, [o["conjunction"] for o in run_rules(rows)], oos_label)
    OUT.write_text(json.dumps(res, indent=1) + "\n")
    for k, v in res.items():
        print(k, {kind: f"{c['flagged']}/{c['n']}" for kind, c in v.items() if "/" not in kind})


if __name__ == "__main__":
    main()
