"""experiments/tier3/train.py -- Week-5 Tier-3 development run (blueprint Ch. 52 Week 5, Ch. 33.2).

For each dataset x encoder, on the development splits only (the test sets stay closed):
  1. C (L2 strength) chosen by macro-F1 on val, ties -> smaller C;
  2. head trained on train with that C (lbfgs: convex and deterministic, so seeds do not change it);
  3. temperature fitted on cal (single-intent, in-scope items);
  4. val: accuracy, macro-F1, ECE (15 right-closed bins, PREREG D5) and NLL before/after T;
     Intents-v0 also per language (en / hi-en) and the mean top probability of compound items
     (the router should escalate those); CLINC150 also OOS AUROC of the top probability.
  5. head exported to experiments/artifacts/tier3/<dataset>-<encoder>/ (not in git) with 100 val
     inputs + Python probabilities (cases.json) for the Rust parity check.

    python experiments/tier3/train.py                  # writes experiments/logs/tier3/week5_dev.json
    python experiments/tier3/train.py --encoders multilingual-e5-small-f16 --c 100 --out …/e5_quant.json
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
import sklearn
from sklearn.metrics import roc_auc_score

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "analysis"))
sys.path.insert(0, str(ROOT / "experiments" / "data"))
from embed import embed, load_encoder  # noqa: E402
from head import INTENTS_CONTRACT, export_head, fit_temperature, logits, nll, softmax, train_head  # noqa: E402
from metrics import accuracy, ece, macro_f1  # noqa: E402
from splits import load_splits  # noqa: E402

C_GRID = (0.1, 1.0, 10.0, 100.0, 1000.0)
ECE_BINS = 15
ARTIFACTS = ROOT / "experiments" / "artifacts" / "tier3"
LOG = ROOT / "experiments" / "logs" / "tier3" / "week5_dev.json"


def label_index(rows, classes, strict=False):
    """(positions of rows whose label is a class, their class indices, positions of the others)."""
    pos = {c: i for i, c in enumerate(classes)}
    keep, y, other = [], [], []
    for i, r in enumerate(rows):
        if r["label"] in pos:
            keep.append(i)
            y.append(pos[r["label"]])
        elif strict:
            raise ValueError(f"label {r['label']!r} is not a head class")
        else:
            other.append(i)
    return keep, np.array(y, dtype=np.int64), other


def choose_c(scores: dict) -> float:
    """C with the best validation score; among ties the smallest C (strongest regularization)."""
    best = max(scores.values())
    return min(c for c, s in scores.items() if s == best)


def classes_for(dataset, splits):
    if dataset == "intents_v0":
        return [l for l in INTENTS_CONTRACT.read_text().split("\n") if l and not l.startswith("#")]
    return sorted({r["label"] for r in splits["train"]})


def _scores(p, y, classes):
    pred, conf = p.argmax(axis=1), p.max(axis=1)
    gold = [classes[i] for i in y]
    names = [classes[i] for i in pred]
    return {"n": int(len(y)), "accuracy": accuracy(gold, names), "macro_f1": macro_f1(gold, names),
            "ece": ece(conf.tolist(), (pred == y).tolist(), n_bins=ECE_BINS)}


def run(dataset, encoder_name, c_grid=C_GRID):
    enc = load_encoder(encoder_name)
    splits = load_splits(dataset)
    classes = classes_for(dataset, splits)
    x = {s: embed(enc, [r["text"] for r in rows], chunk=2048) for s, rows in splits.items()}
    tr_keep, y_tr, _ = label_index(splits["train"], classes, strict=True)
    cal_keep, y_cal, _ = label_index(splits["cal"], classes, strict=True)
    val_keep, y_val, val_other = label_index(splits["val"], classes)
    x_tr, x_cal, x_val = x["train"][tr_keep], x["cal"][cal_keep], x["val"][val_keep]

    c_scores = {}
    for c in c_grid:
        w, b = train_head(x_tr, y_tr, len(classes), c)
        pred = logits(x_val, w, b).argmax(axis=1)
        c_scores[c] = macro_f1([classes[i] for i in y_val], [classes[i] for i in pred])
    c = choose_c(c_scores)
    w, b = train_head(x_tr, y_tr, len(classes), c)

    z_cal, z_val = logits(x_cal, w, b), logits(x_val, w, b)
    cal_errors = int((z_cal.argmax(axis=1) != y_cal).sum())
    try:
        t, t_note = fit_temperature(z_cal, y_cal), "fitted on cal"
    except ValueError as e:
        t, t_note = 1.0, f"NOT FITTED ({e}); T = 1 kept"
    out = {"dataset": dataset, "encoder": encoder_name, "classes": len(classes), "c_grid_val_macro_f1": c_scores,
           "c": c, "temperature": t, "temperature_note": t_note, "cal_n": int(len(y_cal)), "cal_errors": cal_errors,
           "val_raw": _scores(softmax(z_val, 1.0), y_val, classes) | {"nll": nll(z_val, y_val, 1.0)},
           "val_calibrated": _scores(softmax(z_val, t), y_val, classes) | {"nll": nll(z_val, y_val, t)}}

    val_rows = [splits["val"][i] for i in val_keep]
    if dataset in ("intents_v0", "kinds_v0"):
        p = softmax(z_val, t)
        for lang in ("en", "hi-en"):
            m = np.array([r["lang"] == lang for r in val_rows])
            out[f"val_calibrated_{lang}"] = _scores(p[m], y_val[m], classes)
    if dataset == "intents_v0":
        compound = x["val"][val_other]
        out["val_compound"] = {"n": len(val_other),
                               "mean_top_prob": float(softmax(logits(compound, w, b), t).max(axis=1).mean()),
                               "mean_top_prob_single": float(p.max(axis=1).mean())}
    elif dataset == "clinc150":
        oos = x["val"][val_other]
        top_in = softmax(z_val, t).max(axis=1)
        top_oos = softmax(logits(oos, w, b), t).max(axis=1)
        is_in = np.r_[np.ones(len(top_in)), np.zeros(len(top_oos))]
        out["val_oos"] = {"n": len(val_other), "auroc_top_prob": float(roc_auc_score(is_in, np.r_[top_in, top_oos]))}

    head_dir = ARTIFACTS / f"{dataset}-{encoder_name}"
    export_head(head_dir, w, b, t, classes, {
        "encoder": encoder_name, "encoder_sha256": enc.sha256, "input_prefix": enc.prefix,
        "embedding": "llama-embedding, model pooling, L2-normalized", "dataset": dataset,
        "train_n": int(len(y_tr)), "c": c, "solver": f"sklearn {sklearn.__version__} LogisticRegression lbfgs",
        "seed": None, "temperature_note": t_note})
    sample = x_val[:100].astype(np.float32)
    (head_dir / "cases.json").write_text(json.dumps({
        "tolerance": 1e-5, "inputs": sample.astype(np.float64).tolist(),
        "probs": softmax(logits(sample.astype(np.float64), w.astype(np.float64), b.astype(np.float64)),
                         float(np.float32(t))).tolist()}) + "\n")
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--datasets", nargs="+", default=["intents_v0", "clinc150"])
    ap.add_argument("--encoders", nargs="+", default=["bge-small-en-v1.5-q8_0", "multilingual-e5-small-q8_0"])
    ap.add_argument("--c", type=float, help="fixed C instead of the grid (one fit per run; Ch. 33.3 cost)")
    ap.add_argument("--out", type=Path, default=LOG, help="results JSON")
    args = ap.parse_args(argv)
    results = []
    for ds in args.datasets:
        for name in args.encoders:
            r = run(ds, name, (args.c,) if args.c else C_GRID)
            results.append(r)
            raw, cal = r["val_raw"], r["val_calibrated"]
            print(f"{ds:10s} {name:28s} C={r['c']:<6g} T={r['temperature']:.3f} cal_err={r['cal_errors']:<3d} "
                  f"val acc={cal['accuracy']:.3f} F1={cal['macro_f1']:.3f} "
                  f"ECE {raw['ece']:.3f}->{cal['ece']:.3f}", flush=True)
    out = args.out.resolve()
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(results, indent=1) + "\n")
    print(f"wrote {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
