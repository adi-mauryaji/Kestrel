#!/usr/bin/env python3
"""experiments/v3/kind_head.py -- the final kind head on Kestrel-Kinds-v1 (ADR-042), dev splits only.

Fixed before it was run (2026-10-27), with Kinds-v1 frozen and its test split still closed:
- C: as experiments/tier3/train.py chooses it: heads on train over C_GRID, macro-F1 on val, ties to the
  smaller C.
- T: cross-fitted. train + cal in K = 5 folds by family (whole families, round-robin within each stratum
  over a seeded order); for each fold, a head with that C trained on the other four gives the fold's
  logits; T is fitted (NLL, head.fit_temperature) on all ~1000 out-of-fold logits. A T fitted on cal's
  115 items alone moved val's ECE the wrong way three times on Kinds-v0.
- Weights: one head with that C on train + cal (cal is no longer needed for T).
- Val stays out of both: accuracy, macro-F1, ECE, NLL, per class and per language, before and after T.
  τ and τ_g are judged on val with Gemma's outputs (kind_fallback.py --dataset kinds_v1).

Exported to experiments/artifacts/tier3/kinds_v1-<encoder>/ (the head the service loads) with cases.json
for the Rust parity check; log experiments/logs/v3/kinds/final_kinds_v1.json.

    python experiments/v3/kind_head.py      # ~30 s: e5 over Kinds-v1's dev splits, 5 + 6 small fits
"""
from __future__ import annotations

import json
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
import sklearn

ROOT = Path(__file__).resolve().parents[2]
for sub in ("analysis", "data", "tier3"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from embed import embed, load_encoder  # noqa: E402
from head import export_head, fit_temperature, logits, nll, softmax, train_head  # noqa: E402
from prep import seeded_order  # noqa: E402
from splits import load_splits  # noqa: E402
from train import ARTIFACTS, C_GRID, _scores, choose_c, classes_for, label_index, per_class  # noqa: E402
from metrics import macro_f1  # noqa: E402

DATASET = "kinds_v1"
ENCODER = "multilingual-e5-small-q8_0"
K = 5
SEED = 0
LOG = ROOT / "experiments" / "logs" / "v3" / "kinds" / "final_kinds_v1.json"


def folds(rows: list[dict], k: int = K, seed: int = SEED) -> list[int]:
    """A fold per row: whole families (`group`), dealt round-robin within each `stratum` in seeded order."""
    groups = defaultdict(set)
    for r in rows:
        groups[r["stratum"]].add(r["group"])
    fold_of = {}
    for s in sorted(groups):
        for i, g in enumerate(seeded_order(sorted(groups[s]), f"{seed}:{s}")):
            fold_of[g] = i % k
    return [fold_of[r["group"]] for r in rows]


def cross_fit_logits(x, y, fold: list[int], n_classes: int, c: float) -> np.ndarray:
    """Each row's logits from a head trained on the other folds."""
    fold = np.asarray(fold)
    z = np.zeros((len(y), n_classes))
    for f in sorted(set(fold.tolist())):
        m = fold == f
        w, b = train_head(x[~m], y[~m], n_classes, c)
        z[m] = logits(x[m], w, b)
    return z


def main():
    enc = load_encoder(ENCODER)
    splits = load_splits(DATASET)
    classes = classes_for(DATASET, splits)
    x = {s: embed(enc, [r["text"] for r in rows], chunk=2048) for s, rows in splits.items() if s != "test"}
    ys = {s: label_index(splits[s], classes, strict=True)[1] for s in ("train", "cal", "val")}

    c_scores = {}
    for c in C_GRID:
        w, b = train_head(x["train"], ys["train"], len(classes), c)
        pred = logits(x["val"], w, b).argmax(axis=1)
        c_scores[c] = macro_f1([classes[i] for i in ys["val"]], [classes[i] for i in pred])
    c = choose_c(c_scores)

    fit_rows = splits["train"] + splits["cal"]
    x_fit, y_fit = np.vstack([x["train"], x["cal"]]), np.r_[ys["train"], ys["cal"]]
    fold = folds(fit_rows)
    z_oof = cross_fit_logits(x_fit, y_fit, fold, len(classes), c)
    t = fit_temperature(z_oof, y_fit)
    w, b = train_head(x_fit, y_fit, len(classes), c)

    z_val, y_val = logits(x["val"], w, b), ys["val"]
    p = softmax(z_val, t)
    out = {"dataset": DATASET, "encoder": ENCODER, "classes": classes, "c_grid_val_macro_f1": c_scores, "c": c,
           "folds": K, "fit_n": int(len(y_fit)), "oof_errors": int((z_oof.argmax(axis=1) != y_fit).sum()),
           "temperature": t, "temperature_note": f"cross-fitted on train + cal ({K} folds by family)",
           "oof": _scores(softmax(z_oof, t), y_fit, classes) | {"nll": nll(z_oof, y_fit, t)},
           "val_raw": _scores(softmax(z_val, 1.0), y_val, classes) | {"nll": nll(z_val, y_val, 1.0)},
           "val_calibrated": _scores(p, y_val, classes) | {"nll": nll(z_val, y_val, t)},
           "val_per_class": per_class(y_val, z_val.argmax(axis=1), classes)}
    for lang in ("en", "hi-en"):
        m = np.array([r["lang"] == lang for r in splits["val"]])
        out[f"val_calibrated_{lang}"] = _scores(p[m], y_val[m], classes)
    out["val_errors"] = [{"id": r["id"], "text": r["text"], "gold": r["label"], "head": classes[int(q.argmax())],
                          "p": round(float(q.max()), 3)}
                         for r, q in zip(splits["val"], p) if classes[int(q.argmax())] != r["label"]]

    head_dir = ARTIFACTS / f"{DATASET}-{ENCODER}"
    export_head(head_dir, w, b, t, classes, {
        "encoder": ENCODER, "encoder_sha256": enc.sha256, "input_prefix": enc.prefix,
        "embedding": "llama-embedding, model pooling, L2-normalized", "dataset": DATASET,
        "train_n": int(len(y_fit)), "train_splits": "train + cal", "c": c,
        "solver": f"sklearn {sklearn.__version__} LogisticRegression lbfgs", "seed": None,
        "temperature_note": out["temperature_note"]})
    sample = x["val"][:100].astype(np.float32)
    (head_dir / "cases.json").write_text(json.dumps({
        "tolerance": 1e-5, "inputs": sample.astype(np.float64).tolist(),
        "probs": softmax(logits(sample.astype(np.float64), w.astype(np.float64), b.astype(np.float64)),
                         float(np.float32(t))).tolist()}) + "\n")
    LOG.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
    print(json.dumps({k: v for k, v in out.items() if k != "val_errors"}, indent=1))
    for e in out["val_errors"]:
        print(f"  {e['gold']:9s} -> {e['head']:9s} {e['p']:.2f}  {e['text']}")
    print(f"wrote {LOG.relative_to(ROOT)} and {head_dir.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
