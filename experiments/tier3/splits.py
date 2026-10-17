"""experiments/tier3/splits.py -- the development splits Tier-3 work may touch.

Only train (fit the head), cal (fit the temperature) and val (choose hyperparameters, compare
encoders). The test sets stay untouched until the PREREG is frozen (Ch. 34.6); nothing here can
load them.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "data"))
from clinc150 import RAW as CLINC_RAW, kestrel_splits, load_official  # noqa: E402

DEV_SPLITS = ("train", "cal", "val")
INTENTS_V0 = ROOT / "datasets" / "kestrel_intents_v0" / "splits"


def load_splits(dataset: str) -> dict[str, list[dict]]:
    """{train, cal, val: rows}; every row has at least `text` and `label`."""
    if dataset == "intents_v0":
        return {s: [json.loads(l) for l in (INTENTS_V0 / f"{s}.jsonl").open(encoding="utf-8")]
                for s in DEV_SPLITS}
    if dataset == "clinc150":
        full = kestrel_splits(load_official())
        return {s: full[s] for s in DEV_SPLITS}
    raise KeyError(f"unknown dataset {dataset!r}")


def parity_texts(n_per_lang: int = 50) -> list[dict]:
    """Fixed texts for encoder parity checks: the first n English and n Hinglish Intents-v0
    train items by id (train only, so no evaluation item is used)."""
    rows = sorted(load_splits("intents_v0")["train"], key=lambda r: r["id"])
    return [r for lang in ("en", "hi-en") for r in [r for r in rows if r["lang"] == lang][:n_per_lang]]
