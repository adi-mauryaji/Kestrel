"""experiments/continual/e5_data.py -- the CLINC150 10-domain sequence for E5 (Ch. 22.2, ADR-027), dev only.

Embeddings come from the Week-5 cache only (multilingual-e5-small Q8_0 on the runtime route, the
encoder of ADR-014): a cache miss is an error, never a model run. Domains: datasets/raw/clinc150/
domains.json (10 domains x 15 intents, same pinned commit as data_full.json), checked against the 150
in-scope intents. OOS items are not part of E5: the head has no OOS class (OOS never trains, Ch. 32).
Train = the Kestrel train split; the stage matrix and the sweep's selection use val (ADR-027); cal is
kept for the temperature behind the ECE gate (eps3).
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "tier3"))
from embed import embed, load_encoder  # noqa: E402
from splits import load_splits  # noqa: E402
from train import label_index  # noqa: E402

ENCODER = "multilingual-e5-small-q8_0"
DOMAINS = ROOT / "datasets" / "raw" / "clinc150" / "domains.json"


def no_runs(*_args, **_kwargs):
    raise RuntimeError("embedding cache miss: E5 reads the Week-5 cache only (experiments/tier3/train.py fills it)")


def check_domains(domains: dict, intents) -> dict:
    """Every intent in exactly one domain, and no domain intent outside the data."""
    flat = [i for v in domains.values() for i in v]
    if len(flat) != len(set(flat)):
        raise ValueError("an intent is in more than one domain")
    if set(flat) != set(intents):
        missing, extra = sorted(set(intents) - set(flat)), sorted(set(flat) - set(intents))
        raise ValueError(f"domains do not match the intents: missing {missing[:5]}, extra {extra[:5]}")
    return domains


def load() -> dict:
    """{classes: the 150 intents sorted, domains: {domain: intents}, x / y: {train, cal, val: in-scope items}}."""
    enc = load_encoder(ENCODER)
    splits = load_splits("clinc150")
    classes = sorted({r["label"] for r in splits["train"]})
    domains = check_domains(json.loads(DOMAINS.read_text(encoding="utf-8")), classes)
    x, y = {}, {}
    for s, rows in splits.items():
        X = embed(enc, [r["text"] for r in rows], chunk=2048, runner=no_runs)
        keep, y[s], _ = label_index(rows, classes)
        x[s] = X[keep]
    return {"classes": classes, "domains": domains, "x": x, "y": y}


def by_stage(data: dict, split: str, stage_domains) -> list[tuple[np.ndarray, np.ndarray]]:
    """(X, y) of `split` for each stage: the items whose intent is in one of the stage's domains."""
    domain_of = {i: d for d, intents in data["domains"].items() for i in intents}
    item_domain = np.array([domain_of[data["classes"][c]] for c in data["y"][split]])
    out = []
    for doms in stage_domains:
        mask = np.isin(item_domain, doms)
        out.append((data["x"][split][mask], data["y"][split][mask]))
    return out
