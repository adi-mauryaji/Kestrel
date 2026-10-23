"""experiments/router/make_config.py -- the router config from the dev tuning (Week 7 "D"; Ch. 13.4).

    python experiments/router/make_config.py --signal conjunction --delta 0.0

Takes one R3 point from policies_<dataset>.json: the cheapest point on val with accuracy >= the better
of tuned R0 and tuned R1, minus `--delta`, a selection tolerance (ADR-019). The pre-registered
tolerance is 0, the default (ADR-021: δ is only the test margin).
It writes experiments/configs/router_<dataset>.json, which the Rust router reads with
`RouterConfig::from_json`. The thresholds come from val only. `config_sha256` is the SHA-256 of the
canonical JSON of every other field, so a run can record exactly which config it used.

JSON rather than the blueprint's router.toml: reading TOML in Rust needs the `toml` crate, a new
dependency that the capability registry has not approved; serde_json is already pinned.

The config is self-contained for the runtime (`kestrel route-items`): model file names, Tier 4's
label list (its answers, in order; ADR-019), context size and few-shot examples (k per label and language from train, ADR-015), and per-tier cost
estimates (measured dev means, rounded up; the budget is not binding on dev).

`tiers` lists the tiers the cascade is built from. A tier whose threshold came out "off" (it never
accepts) is left out, because a threshold in [0, 1] cannot express "never". `accept` is indexed by
`Tier::index()`; Tier 2 is not built, so its slot holds 1.0.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import math
import sys
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


def outputs_sha256(path: Path) -> str:
    """SHA-256 of the logged tier outputs' content, so the plain file and its committed .gz agree."""
    return hashlib.sha256(gzip.open(path).read() if path.suffix == ".gz" else path.read_bytes()).hexdigest()


def inputs(dataset: str):
    """(policies, tier-output meta, tier-output path) of a dataset; the plain log if present, else
    the committed .gz (CLINC150)."""
    plain = LOG_DIR / f"tier_outputs_{dataset}.jsonl"
    outputs = plain if plain.exists() else plain.with_name(plain.name + ".gz")
    policies = json.loads((LOG_DIR / f"policies_{dataset}.json").read_text())
    meta = json.loads((LOG_DIR / f"tier_outputs_{dataset}.meta.json").read_text())
    return policies, meta, outputs


def runtime_inputs(meta: dict) -> dict:
    """What the runtime needs beyond thresholds, recomputed the way tier_outputs.py ran Tier 3/4."""
    for sub in ("data", "analysis", "tier3", "tier4"):
        sys.path.insert(0, str(ROOT / "experiments" / sub))
    from b3_dev import TASKS, select_shots
    from embed import load_encoder
    from splits import load_splits

    _, labels_file, task_ctx = TASKS[meta["dataset"]]
    n_ctx = meta["tier4"].get("n_ctx", task_ctx)  # older logs ran at the task's context
    labels = [l for l in labels_file.read_text().split("\n") if l and not l.startswith("#") and l != "oos"]
    train = load_splits(meta["dataset"])["train"]
    shots = select_shots(train, labels, meta["tier4"]["shots_per_label_and_language"])
    return {"encoder_file": load_encoder(meta["tier3"]["encoder"]).path.name, "n_ctx": n_ctx,
            "labels": labels_file.name, "shots": [{"text": s["text"], "label": s["label"]} for s in shots]}


def build(policies: dict, meta: dict, tier_outputs_sha256: str, runtime: dict, *, signal, delta: float,
          budget_ms: int = 10_000, policy: str = "R3") -> dict:
    """The R3 point for (signal, delta), or with `policy` R0 / R1 the tuned single-tier baseline
    (ADR-019) as its own config for E1, sharing every model and calibration field with R3."""
    if signal not in SIGNALS:
        raise ValueError(f"the router has no {signal!r} signal")
    if policy == "R3":
        point = policies["R3"][str(signal)]["points"].get(str(delta))
        if point is None:
            raise ValueError(f"no R3 point within delta = {delta} for signal {signal}")
        status = (f"CANDIDATE: chosen on val at tolerance {delta} (ADR-021; delta is only the test margin, "
                  "PREREG §5); FROZEN at the PREREG freeze (Week 13)")
        chosen = {"split": "val", "n": policies["n_val"], "delta": delta, "reference": policies["reference"],
                  "acc": point["acc"], "cost_ms": point["cost"], "crossfit": point["crossfit"]}
    elif policy in ("R0", "R1"):
        point = policies["R0_tuned"] if policy == "R0" else policies["R1"]["tuned"]
        status = "CANDIDATE: tuned on val (ADR-019); FROZEN at the PREREG freeze (Week 13)"
        chosen = {"split": "val", "n": policies["n_val"],
                  "rule": "tuned_single: one abstention threshold maximising val accuracy (ADR-019)",
                  "acc": point["acc"], "cost_ms": point["cost"]}
    else:
        raise ValueError(f"make_config builds no {policy!r} policy")
    t1, t3, t4 = point["tau"]
    cal = policies["calibration"]
    cfg = {
        "format": 1,
        "status": status,
        "dataset": meta["dataset"],
        "policy": policy,
        "tiers": [k for k, t in ((1, t1), (3, t3), (4, t4)) if t is not None],
        "accept": [UNUSED if t1 is None else t1, UNUSED, UNUSED if t3 is None else t3, UNUSED if t4 is None else t4],
        "latency_budget_ms": budget_ms,
        "est_cost_ms": [math.ceil(policies["cost_ms_mean"]["t1"]), 0, math.ceil(policies["cost_ms_mean"]["t3"]),
                        math.ceil(policies["cost_ms_mean"]["t4"])],
        # One tier has nothing to skip, so the signal is only for the cascade.
        "multi_intent_to_generative": SIGNALS[signal] if policy == "R3" else False,
        "calibration": {"tier1_confidence": cal["rule_precision_cal"], "tier3_temperature": cal["t3"],
                        "tier4_temperature": cal["t4"]},
        "models": {"tier3": {k: meta["tier3"][k] for k in ("encoder", "encoder_sha256", "head", "head_bin_sha256")}
                   | {"encoder_file": runtime["encoder_file"]},
                   "tier4": {k: meta["tier4"][k] for k in ("model", "model_file", "prompt", "prompt_sha256",
                                                           "shots_per_label_and_language")}
                   | {"labels": runtime["labels"], "n_ctx": runtime["n_ctx"], "shots": runtime["shots"]}},
        "chosen_on": chosen,
        "provenance": {"tier_outputs_sha256": tier_outputs_sha256, "git_head_at_logging": meta["git_head"]},
    }
    cfg["config_sha256"] = config_hash(cfg)
    return cfg


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", default="intents_v0")
    ap.add_argument("--signal", default="conjunction", choices=["none", "conjunction"])
    ap.add_argument("--delta", type=float, default=0.0, help="selection tolerance; pre-registered 0 (ADR-021)")
    ap.add_argument("--budget-ms", type=int, default=10_000, help="not binding on dev (the simulation has none)")
    ap.add_argument("--policy", default="R3", choices=["R3", "R0", "R1"],
                    help="R0 / R1: the tuned baseline for E1 -> router_<dataset>_r0.json / _r1.json")
    args = ap.parse_args(argv)
    policies, meta, outputs = inputs(args.dataset)
    cfg = build(policies, meta, outputs_sha256(outputs), runtime_inputs(meta),
                signal=None if args.signal == "none" else args.signal, delta=args.delta, budget_ms=args.budget_ms,
                policy=args.policy)
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    suffix = "" if args.policy == "R3" else f"_{args.policy.lower()}"
    path = OUT_DIR / f"router_{args.dataset}{suffix}.json"
    path.write_text(json.dumps(cfg, indent=1, ensure_ascii=False) + "\n")
    print(path.relative_to(ROOT), cfg["config_sha256"])


if __name__ == "__main__":
    main()
