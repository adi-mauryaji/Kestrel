"""experiments/tier4/b3_dev.py -- B3 (always-SLM, system R0) on the development splits (Week 6 R).

Every cal + val request goes to the SLM through the Tier-4 runtime path (`classify_items`:
Qwen3 ChatML with thinking off, the prompt contract datasets/contracts/tier4_intent_prompt.txt,
constrained label decoding with the prefix KV-cached). The test sets are not touched.

Confidence = P(chosen label) under the label-constrained distribution (`step_confidence`).
Constrained decoding yields only the chosen label's probability, not a full distribution, so the
temperature is fitted on cal by minimizing the binary cross-entropy of confidence vs correctness
(what selective prediction needs: confidence ~ P(correct)), not the multi-class NLL.

Latency here is development timing (laptop as found, boost on), not an E1 measurement.

    cargo build --release -p kestrel-inference --features llama --example classify_items
    python experiments/tier4/b3_dev.py --model qwen3-1.7b-q4_k_m
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

import numpy as np
from scipy.optimize import minimize_scalar

ROOT = Path(__file__).resolve().parents[2]
for sub in ("data", "analysis", "tier3"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from metrics import accuracy, ece, macro_f1  # noqa: E402
from prep import norm_text  # noqa: E402
from splits import load_splits  # noqa: E402

CLASSIFY = ROOT / "target" / "release" / "examples" / "classify_items"
PROMPT = ROOT / "datasets" / "contracts" / "tier4_intent_prompt.txt"
LABELS = ROOT / "datasets" / "contracts" / "intents.txt"
LOG_DIR = ROOT / "experiments" / "logs" / "tier4"
T_BOUNDS = (0.05, 20.0)
ECE_BINS = 15


def step_confidence(steps, t: float = 1.0) -> float:
    """Product over the constrained steps of softmax(allowed logits / t)[chosen]."""
    p = 1.0
    for s in steps:
        z = np.asarray(s["logits"], dtype=np.float64) / t
        z -= z.max()
        p *= float(np.exp(z[s["chosen"]]) / np.exp(z).sum())
    return p


def fit_conf_temperature(steps_list, correct) -> float:
    """T minimizing the mean binary cross-entropy of step_confidence(., T) against correctness."""
    y = np.asarray(correct, dtype=np.float64)
    if y.min() == y.max():
        raise ValueError("calibration needs both correct and wrong items")

    def bce(log_t):
        c = np.clip([step_confidence(s, float(np.exp(log_t))) for s in steps_list], 1e-12, 1 - 1e-12)
        return float(-np.mean(y * np.log(c) + (1 - y) * np.log(1 - c)))

    lo, hi = np.log(T_BOUNDS[0]), np.log(T_BOUNDS[1])
    res = minimize_scalar(bce, bounds=(lo, hi), method="bounded", options={"xatol": 1e-5})
    if min(res.x - lo, hi - res.x) < 1e-3:
        raise ValueError(f"temperature optimum at the search bound {T_BOUNDS}: {np.exp(res.x):.4g}")
    return float(np.exp(res.x))


def select_shots(train_rows, labels, k: int) -> list[dict]:
    """k few-shot examples per (label, language) from the training split, each from a different
    template family (first families by name, first item by id). Round-robin order over labels,
    so the examples nearest the request are not all one label."""
    picks = {}
    for label in labels:
        for lang in ("en", "hi-en"):
            cands = sorted((r for r in train_rows if r["label"] == label and r["lang"] == lang),
                           key=lambda r: (r["family"], r["id"]))
            chosen, families = [], set()
            for r in cands:
                if r["family"] not in families:
                    families.add(r["family"])
                    chosen.append(r)
                if len(chosen) == k:
                    break
            picks[(label, lang)] = chosen
    return [{"text": picks[(label, lang)][i]["text"], "label": label}
            for i in range(k) for lang in ("en", "hi-en") for label in labels
            if i < len(picks[(label, lang)])]


def model_path(name: str) -> Path:
    for m in tomllib.loads((ROOT / "models" / "MANIFEST.toml").read_text())["model"]:
        if m["name"] == name and m["role"] == "slm":
            return ROOT / "models" / m["file"]
    raise KeyError(name)


def classify(model: Path, rows: list[dict], shots: list[dict] | None = None) -> list[dict]:
    """Runs classify_items over rows (text normalized like Request.text); results in row order."""
    if not CLASSIFY.exists():
        raise FileNotFoundError(f"{CLASSIFY}: cargo build --release -p kestrel-inference --features llama "
                                "--example classify_items")
    with tempfile.TemporaryDirectory() as d:
        items, out = Path(d) / "items.jsonl", Path(d) / "out.jsonl"
        items.write_text("".join(json.dumps({"id": r["id"], "text": norm_text(r["text"])}) + "\n" for r in rows))
        cmd = [str(CLASSIFY), str(model), str(PROMPT), str(LABELS), str(items), str(out)]
        if shots:
            ex = Path(d) / "examples.jsonl"
            ex.write_text("".join(json.dumps({"text": norm_text(s["text"]), "label": s["label"]}) + "\n"
                                  for s in shots))
            cmd.append(str(ex))
        subprocess.run(cmd, check=True)
        res = [json.loads(l) for l in out.read_text().splitlines()]
    assert [r["id"] for r in res] == [r["id"] for r in rows]
    return res


def _metrics(gold, pred, conf, correct):
    return {"n": len(gold), "accuracy": accuracy(gold, pred), "macro_f1": macro_f1(gold, pred),
            "ece": ece(list(conf), list(correct), n_bins=ECE_BINS)}


def run(model_name: str, dataset: str = "intents_v0", shots_per: int = 0):
    splits = load_splits(dataset)
    model = model_path(model_name)
    labels = [l for l in LABELS.read_text().split("\n") if l and not l.startswith("#")]
    shots = select_shots(splits["train"], labels, shots_per) if shots_per else []
    out = {"dataset": dataset, "model": model_name, "prompt": PROMPT.name,
           "shots_per_label_and_language": shots_per, "shots": len(shots),
           "note": "dev timing on the laptop as found (boost on); not an E1 measurement"}
    res = {s: classify(model, splits[s], shots) for s in ("cal", "val")}
    single = lambda r: "|" not in r["label"]  # noqa: E731

    cal_rows = [(r, o) for r, o in zip(splits["cal"], res["cal"]) if single(r) and "steps" in o]
    cal_correct = [o["label"] == r["label"] for r, o in cal_rows]
    try:
        t, note = fit_conf_temperature([o["steps"] for _, o in cal_rows], cal_correct), "fitted on cal (BCE)"
    except ValueError as e:
        t, note = 1.0, f"NOT FITTED ({e}); T = 1 kept"
    out |= {"temperature": t, "temperature_note": note, "cal_n": len(cal_rows),
            "cal_accuracy": float(np.mean(cal_correct))}

    val = [(r, o) for r, o in zip(splits["val"], res["val"]) if "steps" in o]
    errors = [o for o in res["val"] if "steps" not in o]
    sv = [(r, o) for r, o in val if single(r)]
    gold, pred = [r["label"] for r, _ in sv], [o["label"] for _, o in sv]
    correct = [g == p for g, p in zip(gold, pred)]
    for name, tt in (("val_raw", 1.0), ("val_calibrated", t)):
        conf = [step_confidence(o["steps"], tt) for _, o in sv]
        out[name] = _metrics(gold, pred, conf, correct)
    for lang in ("en", "hi-en"):
        idx = [i for i, (r, _) in enumerate(sv) if r["lang"] == lang]
        conf = [step_confidence(sv[i][1]["steps"], t) for i in idx]
        out[f"val_calibrated_{lang}"] = _metrics([gold[i] for i in idx], [pred[i] for i in idx], conf,
                                                 [correct[i] for i in idx])
    comp = [(r, o) for r, o in val if not single(r)]
    out["val_compound"] = {"n": len(comp),
                           "mean_confidence": float(np.mean([step_confidence(o["steps"], t) for _, o in comp]))
                           if comp else None,
                           "label_in_gold": float(np.mean([o["label"] in r["intents"] for r, o in comp]))
                           if comp else None}
    ms = np.array([o["ms"] for o in res["cal"] + res["val"] if "ms" in o])
    out["dev_ms"] = {"mean": float(ms.mean()), "p50": float(np.median(ms)),
                     "p95": float(np.sort(ms)[int(np.ceil(0.95 * len(ms))) - 1])}
    out["control_token_rejections"] = len(errors)
    confusion = {}
    for g, p in zip(gold, pred):
        if g != p:
            confusion[f"{g} -> {p}"] = confusion.get(f"{g} -> {p}", 0) + 1
    out["val_errors"] = dict(sorted(confusion.items(), key=lambda kv: -kv[1]))
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--model", default="qwen3-1.7b-q4_k_m")
    ap.add_argument("--dataset", default="intents_v0", choices=["intents_v0"])
    ap.add_argument("--shots", type=int, default=0, help="few-shot examples per label and language (train)")
    args = ap.parse_args(argv)
    r = run(args.model, args.dataset, args.shots)
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    path = LOG_DIR / f"b3_dev_{args.dataset}_{args.model}_k{args.shots}.json"
    path.write_text(json.dumps(r, indent=1) + "\n")
    print(json.dumps({k: v for k, v in r.items() if k != "val_errors"}, indent=1))
    print("val errors:", r["val_errors"])
    print(f"wrote {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
