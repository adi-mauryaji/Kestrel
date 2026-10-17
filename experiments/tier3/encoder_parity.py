"""experiments/tier3/encoder_parity.py -- GGUF encoder vs its PyTorch reference (blueprint Ch. 26.2,
Week 5 "encoder parity test"; risk register: "GGUF encoder embeddings do not match reference").

multilingual-e5-small was converted to GGUF here (scripts/convert_encoder.sh, ADR-006), so its
embeddings must match the official PyTorch model: mean pooling over tokens, then L2 norm (the
model's sentence-transformers config), same "query: " + normalized text as the cache.

Tolerances, stated before the run: min cosine >= 0.9999 for F16 and >= 0.999 for Q8_0.
Texts: the first 50 English and 50 Hinglish items of Kestrel-Intents-v0 train (by id).

Runs in the converter venv, which has torch + transformers (the research venv does not):
    third_party/convert-venv/bin/python experiments/tier3/encoder_parity.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
import torch
from transformers import AutoModel, AutoTokenizer

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from embed import load_encoder, model_input, run_llama_embedding  # noqa: E402
from splits import parity_texts  # noqa: E402

SOURCE = ROOT / "models" / "src" / "multilingual-e5-small"
TOLERANCE = {"multilingual-e5-small-f16": 0.9999, "multilingual-e5-small-q8_0": 0.999}
LOG = ROOT / "experiments" / "logs" / "tier3" / "encoder_parity.json"


@torch.no_grad()
def reference(inputs: list[str]) -> np.ndarray:
    tok = AutoTokenizer.from_pretrained(SOURCE)
    model = AutoModel.from_pretrained(SOURCE).eval()
    batch = tok(inputs, padding=True, truncation=True, max_length=512, return_tensors="pt")
    hidden = model(**batch).last_hidden_state
    mask = batch["attention_mask"].unsqueeze(-1).to(hidden.dtype)
    mean = (hidden * mask).sum(dim=1) / mask.sum(dim=1)
    return torch.nn.functional.normalize(mean, dim=-1).numpy().astype(np.float32)


def main():
    rows = parity_texts()
    enc0 = load_encoder("multilingual-e5-small-f16")
    inputs = [model_input(r["text"], enc0.prefix) for r in rows]
    ref = reference(inputs)
    report = {"texts": len(rows), "reference": "intfloat/multilingual-e5-small (PyTorch, mean pool, L2)",
              "torch": torch.__version__, "results": {}}
    ok = True
    for name, tol in TOLERANCE.items():
        enc = load_encoder(name)
        assert enc.prefix == enc0.prefix
        got = run_llama_embedding(enc, inputs)
        cos = (got * ref).sum(axis=1) / (np.linalg.norm(got, axis=1) * np.linalg.norm(ref, axis=1))
        by_lang = {lang: float(cos[[r["lang"] == lang for r in rows]].min()) for lang in ("en", "hi-en")}
        worst = int(cos.argmin())
        passed = bool(cos.min() >= tol)
        ok &= passed
        report["results"][name] = {"tolerance_min_cosine": tol, "min_cosine": float(cos.min()),
                                   "mean_cosine": float(cos.mean()), "min_cosine_by_lang": by_lang,
                                   "max_abs_diff": float(np.abs(got - ref).max()),
                                   "worst_text": rows[worst]["text"], "pass": passed}
        print(f"{name}: min cos {cos.min():.6f} (en {by_lang['en']:.6f}, hi-en {by_lang['hi-en']:.6f}), "
              f"mean {cos.mean():.6f}, tolerance {tol} -> {'PASS' if passed else 'FAIL'}")
    LOG.parent.mkdir(parents=True, exist_ok=True)
    LOG.write_text(json.dumps(report, indent=1) + "\n")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
