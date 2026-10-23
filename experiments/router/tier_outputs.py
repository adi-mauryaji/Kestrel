"""experiments/router/tier_outputs.py -- every tier's output on Kestrel-Intents-v0 dev, logged once (Week 7).

Each routing policy (R0, R1, R2, R2c, R3, R-rand) is a function of these per-item outputs, so they
are computed once here and the policies are simulated offline (policies.py). Dev splits only
(cal + val); the test split cannot be loaded here.

    python experiments/router/tier_outputs.py      # ~3-4 min of full CPU: Tier 4 on 228 items
    python experiments/router/tier_outputs.py --dataset clinc150 --shots K [--prompt FILE] [--n-ctx N]
                                                   # ~1 h: Tier 4 on 3200 items (ADR-018)

- Tier 1: `rules_items` (Rust `RuleSet::default_set`, text normalized as `Request::new` does):
  outcome, conjunction flag, time of `classify` in us.
- Tier 3: multilingual-e5-small Q8_0 (ADR-014) + the Week-5 head: raw logits, so any temperature
  can be applied later, and per-item embedding time. The vectors come from the runtime embedder
  and must equal the training cache within 1e-6 (train/serve parity), or the run stops.
- Tier 4: Gemma-4-E2B-it Q4_0 (ADR-016), k = 2 shots per (label, language) from train (ADR-015),
  via `classify_items`: label, constrained steps (confidence at any T), ms. An item the prompt
  builder rejects carries `error` instead.

Out: experiments/logs/router/tier_outputs_<dataset>.jsonl (one record per item, cal then val)
and .meta.json (models, hashes, settings). Timings are dev timings as found, not E1.

    python experiments/router/tier_outputs.py --dataset clinc150 --split test    # E2 (Week 13, ADR-031)

`--split test` runs every tier on the test split with exactly the frozen router config's models, head,
prompt, context and few-shot examples (hashes checked), and only on a frozen PREREG (run_e1.guard_test:
the tag, FROZEN configs, a clean tree) -> tier_outputs_<dataset>_test.jsonl(.gz). E2 uses its
confidences; its timings are not measurements (E3 takes costs from E1's rows).
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
for sub in ("data", "analysis", "tier3", "tier4"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from b3_dev import CONTRACTS, TASKS, classify, model_path, select_shots  # noqa: E402
from embed import EMBED_TEXTS, embed, load_encoder, model_input  # noqa: E402
from head import load_head, logits  # noqa: E402
from splits import load_splits  # noqa: E402

RULES_ITEMS = ROOT / "target" / "release" / "examples" / "rules_items"
ENCODER = "multilingual-e5-small-q8_0"
HEADS = ROOT / "experiments" / "artifacts" / "tier3"
SLM = "gemma-4-e2b-it-q4_0"
SHOTS_PER = 2  # Intents-v0 (ADR-015)
PARITY_TOL = 1e-6
LOG_DIR = ROOT / "experiments" / "logs" / "router"


def run_rules(rows: list[dict]) -> list[dict]:
    """Tier-1 outcome per row, in row order (the Rust rules on Request-normalized text)."""
    if not RULES_ITEMS.exists():
        raise FileNotFoundError(f"{RULES_ITEMS}: cargo build --release -p kestrel-nlu --example rules_items")
    with tempfile.TemporaryDirectory() as d:
        items, out = Path(d) / "items.jsonl", Path(d) / "out.jsonl"
        items.write_text("".join(json.dumps({"id": r["id"], "text": r["text"]}) + "\n" for r in rows))
        subprocess.run([str(RULES_ITEMS), str(items), str(out)], check=True)
        return [json.loads(l) for l in out.read_text().splitlines()]


def timed_embeddings(enc, texts: list[str]) -> tuple[np.ndarray, list[float]]:
    """The runtime embedder over texts, one at a time: vectors and ms per text."""
    with tempfile.TemporaryDirectory() as d:
        src, dst, ms = Path(d) / "inputs.txt", Path(d) / "out.f32", Path(d) / "ms.txt"
        src.write_text("\n".join(model_input(t, enc.prefix) for t in texts) + "\n", encoding="utf-8")
        done = subprocess.run([str(EMBED_TEXTS), str(enc.path), str(src), str(dst), str(ms)],
                              check=True, capture_output=True, text=True)
        n, dim = map(int, done.stdout.split())
        assert n == len(texts)
        return np.fromfile(dst, dtype="<f4").reshape(n, dim), [float(v) for v in ms.read_text().split()]


def run_tier3(enc, rows: list[dict], head_dir: Path):
    """Raw head logits [n x K], ms per item, head metadata."""
    w, b, t, meta = load_head(head_dir)
    if meta["encoder"] != enc.name or meta["encoder_sha256"] != enc.sha256:
        raise ValueError(f"head {head_dir.name} was trained on another encoder file")
    texts = [r["text"] for r in rows]
    x, ms = timed_embeddings(enc, texts)
    gap = float(np.abs(x - embed(enc, texts)).max())
    if gap > PARITY_TOL:
        raise ValueError(f"runtime vectors differ from the training cache by {gap:.3g} > {PARITY_TOL}")
    return logits(x, w, b), ms, meta | {"temperature": t, "cache_gap": gap}


def assemble(split: str, rows: list[dict], t1: list[dict], z3, ms3: list[float], t4: list[dict]) -> list[dict]:
    """One record per row, in row order. Every tier's output must be for the same items, in order."""
    ids = [r["id"] for r in rows]
    if [o["id"] for o in t1] != ids or [o["id"] for o in t4] != ids or len(z3) != len(ids) or len(ms3) != len(ids):
        raise ValueError(f"{split}: tier outputs are not aligned with the rows")
    drop_id = lambda o: {k: v for k, v in o.items() if k != "id"}  # noqa: E731
    return [{"split": split, "id": r["id"], "gold": r["label"], "lang": r.get("lang", "en"),
             "family": r.get("family", r["id"]), "compound": "|" in r["label"],
             "t1": drop_id(a), "t3": {"logits": [float(v) for v in z], "ms": m}, "t4": drop_id(c)}
            for r, a, z, m, c in zip(rows, t1, z3, ms3, t4)]


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def frozen_tier4(cfg: dict) -> tuple[Path, int, list[dict]]:
    """Tier 4's prompt, context and few-shot examples in a router config: a test run uses exactly these."""
    t4 = cfg["models"]["tier4"]
    return CONTRACTS / t4["prompt"], t4["n_ctx"], t4["shots"]


def run_test(dataset: str) -> None:
    """Every tier on the test split, with the frozen R3 config's models and settings (ADR-031)."""
    sys.path.insert(0, str(ROOT / "experiments" / "e1"))
    from run_e1 import guard_test, hash_problems, load_items

    cfg_path = ROOT / "experiments" / "configs" / f"router_{dataset}.json"
    cfg = json.loads(cfg_path.read_text())
    guard_test([cfg])
    if problems := hash_problems(cfg):
        raise SystemExit("not starting:\n- " + "\n- ".join(problems))
    if cfg["models"]["tier3"]["encoder"] != ENCODER:
        raise SystemExit(f"the frozen config's encoder is {cfg['models']['tier3']['encoder']}, not {ENCODER}")
    prompt, n_ctx, shots = frozen_tier4(cfg)
    head_dir = HEADS / cfg["models"]["tier3"]["head"]
    rows = load_items(dataset, "test")
    enc = load_encoder(ENCODER)
    t1 = run_rules(rows)
    z3, ms3, head_meta = run_tier3(enc, rows, head_dir)
    t4 = classify(model_path(cfg["models"]["tier4"]["model"]), rows, shots, dataset, prompt, n_ctx)
    records = assemble("test", rows, t1, z3, ms3, t4)
    out_path = LOG_DIR / f"tier_outputs_{dataset}_test.jsonl"
    out_path.write_text("".join(json.dumps(r) + "\n" for r in records))
    meta = {
        "dataset": dataset, "split": "test", "n": {"test": len(records)},
        "date": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
        "git_head": subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True,
                                   cwd=ROOT).stdout.strip(),
        "config": str(cfg_path.relative_to(ROOT)), "config_sha256": cfg["config_sha256"],
        "tier1": {"rules": "kestrel_nlu::rules::RuleSet::default_set"},
        "tier3": {"encoder": ENCODER, "encoder_sha256": enc.sha256, "head": head_dir.name,
                  "head_bin_sha256": _sha256(head_dir / "head.bin"), "classes": head_meta["classes"]
                  if "classes" in head_meta else json.loads((head_dir / "head.json").read_text())["classes"],
                  "temperature_cal": head_meta["temperature"], "cache_gap_max_abs": head_meta["cache_gap"]},
        "tier4": {"model": cfg["models"]["tier4"]["model"], "prompt": prompt.name, "prompt_sha256": _sha256(prompt),
                  "shots": len(shots), "n_ctx": n_ctx, "errors": sum("error" in r["t4"] for r in records)},
        "timing": {"note": "not a measurement: E2 uses the confidences; E3 takes costs from E1's rows"},
    }
    out_path.with_suffix(".meta.json").write_text(json.dumps(meta, indent=1) + "\n")
    print(json.dumps({k: v for k, v in meta.items() if k != "tier3"}, indent=1))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dataset", default="intents_v0", choices=sorted(TASKS))
    ap.add_argument("--shots", type=int, default=SHOTS_PER, help="few-shot examples per label and language")
    ap.add_argument("--prompt", help="prompt contract under datasets/contracts/ instead of the task's")
    ap.add_argument("--n-ctx", type=int, help="Tier-4 context tokens instead of the task's")
    ap.add_argument("--split", default="dev", choices=["dev", "test"],
                    help="test: the frozen config's settings, only after the freeze (ADR-031)")
    args = ap.parse_args(argv)
    if args.split == "test":
        if args.shots != SHOTS_PER or args.prompt or args.n_ctx:
            ap.error("--split test takes every setting from the frozen config")
        return run_test(args.dataset)
    dataset = args.dataset
    task_prompt, labels_file, task_ctx = TASKS[dataset]
    prompt = CONTRACTS / args.prompt if args.prompt else task_prompt
    n_ctx = args.n_ctx or task_ctx
    head_dir = HEADS / f"{dataset}-{ENCODER}"
    out_path = LOG_DIR / f"tier_outputs_{dataset}.jsonl"
    splits = load_splits(dataset)
    labels = [l for l in labels_file.read_text().split("\n") if l and not l.startswith("#")]
    shots = select_shots(splits["train"], [l for l in labels if l != "oos"], args.shots)  # train has no OOS
    enc = load_encoder(ENCODER)
    slm = model_path(SLM)

    records, head_meta = [], None
    for split in ("cal", "val"):
        rows = splits[split]
        t1 = run_rules(rows)
        z3, ms3, head_meta = run_tier3(enc, rows, head_dir)
        t4 = classify(slm, rows, shots, dataset, prompt, n_ctx)
        records += assemble(split, rows, t1, z3, ms3, t4)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text("".join(json.dumps(r) + "\n" for r in records))
    boost = Path("/sys/devices/system/cpu/cpufreq/boost")
    head_json = json.loads((head_dir / "head.json").read_text())
    meta = {
        "dataset": dataset, "n": {s: sum(r["split"] == s for r in records) for s in ("cal", "val")},
        "date": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
        "git_head": subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True,
                                   cwd=ROOT).stdout.strip(),
        "tier1": {"rules": "kestrel_nlu::rules::RuleSet::default_set"},
        "tier3": {"encoder": ENCODER, "encoder_sha256": enc.sha256, "head": head_dir.name,
                  "head_bin_sha256": _sha256(head_dir / "head.bin"), "classes": head_json["classes"],
                  "temperature_cal": head_meta["temperature"], "cache_gap_max_abs": head_meta["cache_gap"]},
        "tier4": {"model": SLM, "model_file": slm.name, "prompt": prompt.name, "prompt_sha256": _sha256(prompt),
                  "shots_per_label_and_language": args.shots, "shots": len(shots), "n_ctx": n_ctx,
                  "errors": sum("error" in r["t4"] for r in records)},
        "timing": {"boost": boost.read_text().strip() if boost.exists() else None,
                   "note": "dev timing on the laptop as found; not an E1 measurement"},
    }
    out_path.with_suffix(".meta.json").write_text(json.dumps(meta, indent=1) + "\n")
    print(json.dumps(meta, indent=1))


if __name__ == "__main__":
    main()
