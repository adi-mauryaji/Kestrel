"""experiments/data/clinc150.py -- CLINC150 loader and Kestrel's four sets (Ch. 31, 32).

CLINC150 "full" (Larson et al. 2019, clinc/oos-eval, CC BY 3.0): 150 intents x 100/20/30
train/val/test queries, plus out-of-scope (OOS) 100/100/1000. Raw file: datasets/raw/ (not in
git; scripts/fetch_datasets.py downloads and verifies it).

Kestrel's sets (Ch. 32: keep the official test intact; carve calibration and validation from the
official training/validation portions; OOS evaluates abstention and never trains a head):
- train: official train, in-scope only                         (fit the head)
- cal:   half of official val in-scope, stratified by intent   (fit temperature)
- val:   other half of official val in-scope + official train and val OOS
                                                               (tune thresholds / abstention)
- test:  official test, in-scope + OOS, untouched              (touched once per frozen config)

    python experiments/data/clinc150.py --write-manifest    # sizes + fingerprints -> datasets/clinc150/
"""
from __future__ import annotations

import argparse
import hashlib
import json
from collections import defaultdict
from pathlib import Path

from prep import NAMES, dataset_fingerprint, norm_text, stratified_split4

ROOT = Path(__file__).resolve().parents[2]
RAW = ROOT / "datasets" / "raw" / "clinc150" / "data_full.json"
MANIFEST = ROOT / "datasets" / "clinc150" / "manifest.json"
OFFICIAL = ("train", "val", "test", "oos_train", "oos_val", "oos_test")
OOS_LABEL = "oos"


def load_official(path=RAW):
    """{official split: rows}; each row keeps its official split and an id stable across runs."""
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    return {split: [{"id": f"clinc150/{split}/{i}", "text": text, "label": label,
                     "official_split": split, "oos": label == OOS_LABEL}
                    for i, (text, label) in enumerate(data[split])]
            for split in OFFICIAL}


def kestrel_splits(official, seed=0):
    """{train, cal, val, test: rows} as described in the module docstring."""
    val_in = official["val"]
    halves = stratified_split4(val_in, fracs=(0.0, 0.5, 0.5, 0.0), seed=seed)
    return {
        "train": list(official["train"]),
        "cal": [val_in[i] for i in halves["cal"]],
        "val": [val_in[i] for i in halves["val"]] + official["oos_train"] + official["oos_val"],
        "test": official["test"] + official["oos_test"],
    }


def cross_split_duplicates(splits):
    """[(normalized text, sorted set names)] for texts that occur in more than one set."""
    where = defaultdict(set)
    for name, rows in splits.items():
        for r in rows:
            where[norm_text(r["text"])].add(name)
    return sorted((t, sorted(s)) for t, s in where.items() if len(s) > 1)


def manifest(splits, seed=0):
    dups = cross_split_duplicates(splits)
    return {
        "dataset": "clinc150-full",
        "raw_sha256": hashlib.sha256(RAW.read_bytes()).hexdigest(),
        "seed": seed,
        "oos_label": OOS_LABEL,
        "splits": {name: {
            "n": len(splits[name]),
            "n_oos": sum(r["oos"] for r in splits[name]),
            "n_intents": len({r["label"] for r in splits[name] if not r["oos"]}),
            "fingerprint": dataset_fingerprint(splits[name]),
        } for name in NAMES},
        "cross_split_duplicates": len(dups),
        "cross_split_duplicate_examples": [[t, names] for t, names in dups[:10]],
    }


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--write-manifest", action="store_true")
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args(argv)
    m = manifest(kestrel_splits(load_official(), a.seed), a.seed)
    if a.write_manifest:
        MANIFEST.parent.mkdir(parents=True, exist_ok=True)
        MANIFEST.write_text(json.dumps(m, indent=1, ensure_ascii=False) + "\n")
    print(json.dumps({k: v for k, v in m.items() if k != "cross_split_duplicate_examples"}, indent=1))


if __name__ == "__main__":
    main()
