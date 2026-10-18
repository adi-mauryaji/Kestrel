"""experiments/router/make_config.py -- the router config from the dev tuning (Week 7 "D"; Ch. 13.4).

    python experiments/router/make_config.py --signal conjunction --delta 0.0

Takes one R3 point from policies_<dataset>.json: the cheapest point on val with accuracy >= R0 - delta.
It writes experiments/configs/router_<dataset>.json, which the Rust router reads with
`RouterConfig::from_json`. The thresholds come from val only. `config_sha256` is the SHA-256 of the
canonical JSON of every other field, so a run can record exactly which config it used.

JSON rather than the blueprint's router.toml: reading TOML in Rust needs the `toml` crate, a new
dependency that the capability registry has not approved; serde_json is already pinned.

`tiers` lists the tiers the cascade is built from. A tier whose threshold came out "off" (it never
accepts) is left out, because a threshold in [0, 1] cannot express "never". `accept` is indexed by
`Tier::index()`; Tier 2 is not built, so its slot holds 1.0.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOG_DIR = ROOT / "experiments" / "logs" / "router"
OUT_DIR = ROOT / "experiments" / "configs"
SIGNALS = {None: False, "conjunction": True}  # signal -> RouterConfig.multi_intent_to_generative
UNUSED = 1.0


def config_hash(cfg: dict) -> str:
    body = {k: v for k, v in cfg.items() if k != "config_sha256"}
    canon = json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(canon.encode()).hexdigest()


def verify(cfg: dict) -> bool:
    return cfg.get("config_sha256") == config_hash(cfg)


def build(policies: dict, meta: dict, tier_outputs_sha256: str, *, signal, delta: float,
          budget_ms: int = 10_000) -> dict:
    if signal not in SIGNALS:
        raise ValueError(f"the router has no {signal!r} signal")
    point = policies["R3"][str(signal)]["points"].get(str(delta))
    if point is None:
        raise ValueError(f"no R3 point within delta = {delta} for signal {signal}")
    t1, t3, t4 = point["tau"]
    cal = policies["calibration"]
    cfg = {
        "format": 1,
        "status": f"DEV: chosen on val with delta = {delta}; delta is not set yet (PREREG §5), so this is not frozen",
        "dataset": meta["dataset"],
        "policy": "R3",
        "tiers": [k for k, t in ((1, t1), (3, t3), (4, t4)) if t is not None],
        "accept": [UNUSED if t1 is None else t1, UNUSED, UNUSED if t3 is None else t3, UNUSED if t4 is None else t4],
        "latency_budget_ms": budget_ms,
        "multi_intent_to_generative": SIGNALS[signal],
        "calibration": {"tier1_confidence": cal["rule_precision_cal"], "tier3_temperature": cal["t3"],
                        "tier4_temperature": cal["t4"]},
        "models": {"tier3": {k: meta["tier3"][k] for k in ("encoder", "encoder_sha256", "head", "head_bin_sha256")},
                   "tier4": {k: meta["tier4"][k] for k in ("model", "model_file", "prompt", "prompt_sha256",
                                                           "shots_per_label_and_language")}},
        "chosen_on": {"split": "val", "n": policies["n_val"], "delta": delta, "r0_acc": policies["R0"]["acc"],
                      "acc": point["acc"], "cost_ms": point["cost"], "crossfit": point["crossfit"]},
        "provenance": {"tier_outputs_sha256": tier_outputs_sha256, "git_head_at_logging": meta["git_head"]},
    }
    cfg["config_sha256"] = config_hash(cfg)
    return cfg


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", default="intents_v0")
    ap.add_argument("--signal", default="conjunction", choices=["none", "conjunction"])
    ap.add_argument("--delta", type=float, default=0.0)
    ap.add_argument("--budget-ms", type=int, default=10_000, help="not binding on dev (the simulation has none)")
    args = ap.parse_args(argv)
    outputs = LOG_DIR / f"tier_outputs_{args.dataset}.jsonl"
    policies = json.loads((LOG_DIR / f"policies_{args.dataset}.json").read_text())
    meta = json.loads(outputs.with_suffix(".meta.json").read_text())
    cfg = build(policies, meta, hashlib.sha256(outputs.read_bytes()).hexdigest(),
                signal=None if args.signal == "none" else args.signal, delta=args.delta, budget_ms=args.budget_ms)
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    path = OUT_DIR / f"router_{args.dataset}.json"
    path.write_text(json.dumps(cfg, indent=1, ensure_ascii=False) + "\n")
    print(path.relative_to(ROOT), cfg["config_sha256"])


if __name__ == "__main__":
    main()
