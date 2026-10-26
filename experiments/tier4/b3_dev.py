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
import os
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
from prep import norm_text, seeded_order  # noqa: E402
from splits import load_splits  # noqa: E402

CLASSIFY = ROOT / "target" / "release" / "examples" / "classify_items"
CONTRACTS = ROOT / "datasets" / "contracts"
# Per dataset: (prompt contract, label list, runner context tokens).
TASKS = {
    "intents_v0": (CONTRACTS / "tier4_intent_prompt.txt", CONTRACTS / "intents.txt", 2048),
    "clinc150": (CONTRACTS / "tier4_clinc150_prompt.txt", CONTRACTS / "clinc150_labels.txt", 6144),
    "kinds_v0": (CONTRACTS / "tier4_kind_prompt.txt", CONTRACTS / "kinds.txt", 2048),  # v3 (ADR-037)
}
PROMPT, LABELS, _ = TASKS["intents_v0"]
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


def select_shots(train_rows, labels, k: int, seed: int | None = None) -> list[dict]:
    """k few-shot examples per (label, language) from the training split, each from a different
    template family (first families by name, first item by id). Round-robin order over labels,
    so the examples nearest the request are not all one label. With `seed`, families and items
    are taken in seeded hash order instead: the δ pilot's few-shot draw (ADR-020)."""
    # CLINC150 rows have no language or family: one language, each item its own family.
    langs = sorted({r.get("lang") for r in train_rows}, key=str)
    family = lambda r: r.get("family", r["id"])  # noqa: E731
    picks = {}
    for label in labels:
        for lang in langs:
            cands = [r for r in train_rows if r["label"] == label and r.get("lang") == lang]
            if seed is None:
                order = lambda r: (family(r), r["id"])  # noqa: E731
            else:
                key = f"{seed}:{label}:{lang}"
                fam_rank = {f: i for i, f in enumerate(seeded_order(sorted({family(r) for r in cands}), key))}
                id_rank = {x: i for i, x in enumerate(seeded_order([r["id"] for r in cands], key))}
                order = lambda r: (fam_rank[family(r)], id_rank[r["id"]])  # noqa: E731
            cands.sort(key=order)
            chosen, families = [], set()
            for r in cands:
                if family(r) not in families:
                    families.add(family(r))
                    chosen.append(r)
                if len(chosen) == k:
                    break
            picks[(label, lang)] = chosen
    return [{"text": picks[(label, lang)][i]["text"], "label": label}
            for i in range(k) for lang in langs for label in labels
            if i < len(picks[(label, lang)])]


def subset(rows: list[dict], n: int, seed: int = 0) -> list[dict]:
    """A fixed sample of n rows (seeded hash order of the ids), kept in the original order."""
    if n >= len(rows):
        return rows
    keep = set(seeded_order([r["id"] for r in rows], seed)[:n])
    return [r for r in rows if r["id"] in keep]


def model_path(name: str) -> Path:
    for m in tomllib.loads((ROOT / "models" / "MANIFEST.toml").read_text())["model"]:
        if m["name"] == name and m["role"] == "slm":
            return ROOT / "models" / m["file"]
    raise KeyError(name)


def classify(model: Path, rows: list[dict], shots: list[dict] | None = None,
             task: str = "intents_v0", prompt: Path | None = None, n_ctx: int | None = None) -> list[dict]:
    """Runs classify_items over rows (text normalized like Request.text); results in row order.
    `prompt` and `n_ctx` override the task's prompt contract and runner context."""
    if not CLASSIFY.exists():
        raise FileNotFoundError(f"{CLASSIFY}: cargo build --release -p kestrel-inference --features llama "
                                "--example classify_items")
    with tempfile.TemporaryDirectory() as d:
        items, out = Path(d) / "items.jsonl", Path(d) / "out.jsonl"
        items.write_text("".join(json.dumps({"id": r["id"], "text": norm_text(r["text"])}) + "\n" for r in rows))
        task_prompt, labels, task_ctx = TASKS[task]
        prompt, n_ctx = prompt or task_prompt, n_ctx or task_ctx
        cmd = [str(CLASSIFY), str(model), str(prompt), str(labels), str(items), str(out)]
        if shots:
            ex = Path(d) / "examples.jsonl"
            ex.write_text("".join(json.dumps({"text": norm_text(s["text"]), "label": s["label"]}) + "\n"
                                  for s in shots))
            cmd.append(str(ex))
        subprocess.run(cmd, check=True, env={**os.environ, "KESTREL_N_CTX": str(n_ctx)})
        res = [json.loads(l) for l in out.read_text().splitlines()]
    assert [r["id"] for r in res] == [r["id"] for r in rows]
    return res


def _metrics(gold, pred, conf, correct):
    return {"n": len(gold), "accuracy": accuracy(gold, pred), "macro_f1": macro_f1(gold, pred),
            "ece": ece(list(conf), list(correct), n_bins=ECE_BINS)}


def log_name(dataset: str, model: str, shots: int, n_subset: int | None, prompt: Path) -> str:
    """Log file name; a prompt other than the task's default is named in it."""
    tag = f"_n{n_subset}" if n_subset else ""
    if prompt != TASKS[dataset][0]:
        tag += f"_{prompt.stem}"
    return f"b3_dev_{dataset}_{model}_k{shots}{tag}.json"


def run(model_name: str, dataset: str = "intents_v0", shots_per: int = 0, n_subset: int | None = None,
        prompt: Path | None = None, n_ctx: int | None = None):
    splits = load_splits(dataset)
    if n_subset:
        splits = {s: (rows if s == "train" else subset(rows, n_subset)) for s, rows in splits.items()}
    model = model_path(model_name)
    task_prompt, labels_file, task_ctx = TASKS[dataset]
    prompt = prompt or task_prompt
    labels = [l for l in labels_file.read_text().split("\n") if l and not l.startswith("#")]
    shot_labels = [l for l in labels if l != "oos"]  # train has no OOS items (D2)
    shots = select_shots(splits["train"], shot_labels, shots_per) if shots_per else []
    out = {"dataset": dataset, "model": model_name, "prompt": prompt.name,
           "shots_per_label_and_language": shots_per, "shots": len(shots),
           "subset_per_split": n_subset, "n_ctx": n_ctx or task_ctx,
           "note": "dev timing on the laptop as found (boost on); not an E1 measurement"}
    res = {s: classify(model, splits[s], shots, dataset, prompt, n_ctx) for s in ("cal", "val")}
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
    for lang in sorted({r["lang"] for r, _ in sv if r.get("lang")}):
        idx = [i for i, (r, _) in enumerate(sv) if r.get("lang") == lang]
        conf = [step_confidence(sv[i][1]["steps"], t) for i in idx]
        out[f"val_calibrated_{lang}"] = _metrics([gold[i] for i in idx], [pred[i] for i in idx], conf,
                                                 [correct[i] for i in idx])
    oos = [i for i, g in enumerate(gold) if g == "oos"]
    if oos:
        ins = [i for i, g in enumerate(gold) if g != "oos"]
        out["val_oos"] = {"n_oos": len(oos), "oos_recall": float(np.mean([pred[i] == "oos" for i in oos])),
                          "in_scope_accuracy": float(np.mean([correct[i] for i in ins])),
                          "in_scope_predicted_oos": float(np.mean([pred[i] == "oos" for i in ins]))}
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
    ap.add_argument("--dataset", default="intents_v0", choices=sorted(TASKS))
    ap.add_argument("--shots", type=int, default=0, help="few-shot examples per label and language (train)")
    ap.add_argument("--subset", type=int, help="fixed seeded sample of N items from cal and from val")
    ap.add_argument("--prompt", help="prompt contract under datasets/contracts/ instead of the task's")
    ap.add_argument("--n-ctx", type=int, help="runner context tokens instead of the task's")
    args = ap.parse_args(argv)
    prompt = CONTRACTS / args.prompt if args.prompt else TASKS[args.dataset][0]
    r = run(args.model, args.dataset, args.shots, args.subset, prompt, args.n_ctx)
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    path = LOG_DIR / log_name(args.dataset, args.model, args.shots, args.subset, prompt)
    path.write_text(json.dumps(r, indent=1) + "\n")
    print(json.dumps({k: v for k, v in r.items() if k != "val_errors"}, indent=1))
    print("val errors:", r["val_errors"])
    print(f"wrote {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
