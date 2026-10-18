"""experiments/router/tier_outputs.py -- every tier's output on Kestrel-Intents-v0 dev, logged once (Week 7).

Each routing policy (R0, R1, R2, R2c, R3, R-rand) is a function of these per-item outputs, so they
are computed once here and the policies are simulated offline (policies.py). Dev splits only
(cal + val); the test split cannot be loaded here.

    python experiments/router/tier_outputs.py      # ~3-4 min of full CPU: Tier 4 on 228 items

- Tier 1: `rules_items` (Rust `RuleSet::default_set`, text normalized as `Request::new` does):
  outcome, conjunction flag, time of `classify` in us.
- Tier 3: multilingual-e5-small Q8_0 (ADR-014) + the Week-5 head: raw logits, so any temperature
  can be applied later, and per-item embedding time. The vectors come from the runtime embedder
  and must equal the training cache within 1e-6 (train/serve parity), or the run stops.
- Tier 4: Gemma-4-E2B-it Q4_0 (ADR-016), k = 2 shots per (label, language) from train (ADR-015),
  via `classify_items`: label, constrained steps (confidence at any T), ms. An item the prompt
  builder rejects carries `error` instead.

Out: experiments/logs/router/tier_outputs_intents_v0.jsonl (one record per item, cal then val)
and .meta.json (models, hashes, settings). Timings are dev timings as found, not E1.
"""
from __future__ import annotations

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
from b3_dev import TASKS, classify, model_path, select_shots  # noqa: E402
from embed import EMBED_TEXTS, embed, load_encoder, model_input  # noqa: E402
from head import load_head, logits  # noqa: E402
from splits import load_splits  # noqa: E402

RULES_ITEMS = ROOT / "target" / "release" / "examples" / "rules_items"
DATASET = "intents_v0"
ENCODER = "multilingual-e5-small-q8_0"
HEAD_DIR = ROOT / "experiments" / "artifacts" / "tier3" / f"{DATASET}-{ENCODER}"
SLM = "gemma-4-e2b-it-q4_0"
SHOTS_PER = 2
PARITY_TOL = 1e-6
OUT = ROOT / "experiments" / "logs" / "router" / f"tier_outputs_{DATASET}.jsonl"


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


def run_tier3(enc, rows: list[dict]):
    """Raw head logits [n x K], ms per item, head metadata."""
    w, b, t, meta = load_head(HEAD_DIR)
    if meta["encoder"] != enc.name or meta["encoder_sha256"] != enc.sha256:
        raise ValueError(f"head {HEAD_DIR.name} was trained on another encoder file")
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
    return [{"split": split, "id": r["id"], "gold": r["label"], "lang": r["lang"], "compound": "|" in r["label"],
             "t1": drop_id(a), "t3": {"logits": [float(v) for v in z], "ms": m}, "t4": drop_id(c)}
            for r, a, z, m, c in zip(rows, t1, z3, ms3, t4)]


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    splits = load_splits(DATASET)
    prompt, labels_file, _ = TASKS[DATASET]
    labels = [l for l in labels_file.read_text().split("\n") if l and not l.startswith("#")]
    shots = select_shots(splits["train"], labels, SHOTS_PER)
    enc = load_encoder(ENCODER)
    slm = model_path(SLM)

    records, head_meta = [], None
    for split in ("cal", "val"):
        rows = splits[split]
        t1 = run_rules(rows)
        z3, ms3, head_meta = run_tier3(enc, rows)
        t4 = classify(slm, rows, shots, DATASET)
        records += assemble(split, rows, t1, z3, ms3, t4)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("".join(json.dumps(r) + "\n" for r in records))
    boost = Path("/sys/devices/system/cpu/cpufreq/boost")
    head_json = json.loads((HEAD_DIR / "head.json").read_text())
    meta = {
        "dataset": DATASET, "n": {s: sum(r["split"] == s for r in records) for s in ("cal", "val")},
        "date": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
        "git_head": subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True,
                                   cwd=ROOT).stdout.strip(),
        "tier1": {"rules": "kestrel_nlu::rules::RuleSet::default_set"},
        "tier3": {"encoder": ENCODER, "encoder_sha256": enc.sha256, "head": HEAD_DIR.name,
                  "head_bin_sha256": _sha256(HEAD_DIR / "head.bin"), "classes": head_json["classes"],
                  "temperature_cal": head_meta["temperature"], "cache_gap_max_abs": head_meta["cache_gap"]},
        "tier4": {"model": SLM, "model_file": slm.name, "prompt": prompt.name, "prompt_sha256": _sha256(prompt),
                  "shots_per_label_and_language": SHOTS_PER, "shots": len(shots),
                  "errors": sum("error" in r["t4"] for r in records)},
        "timing": {"boost": boost.read_text().strip() if boost.exists() else None,
                   "note": "dev timing on the laptop as found; not an E1 measurement"},
    }
    OUT.with_suffix(".meta.json").write_text(json.dumps(meta, indent=1) + "\n")
    print(json.dumps(meta, indent=1))


if __name__ == "__main__":
    main()
