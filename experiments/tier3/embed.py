"""experiments/tier3/embed.py -- Tier-3 embedding cache (blueprint Ch. 33.2 steps 1-2).

Embeds texts with a frozen GGUF sentence encoder through llama.cpp's `llama-embedding`: the same
engine and commit (26394b4e, ADR-006) that the Rust runtime links through `llama-cpp-2`, so the
head is trained on the vectors it will see at inference time. Embeddings are L2-normalized and
pooled the way the GGUF file says (bge: CLS, e5: mean).

What the encoder sees is `prefix + normalize(text)`:
- normalize = kestrel_core::normalize (prep.norm_text), because the Rust Request carries
  normalized text;
- prefix = what the encoder was trained with: multilingual-e5 expects "query: " (its model card),
  bge-small-en-v1.5 needs no instruction for classification.

Cache: one .npy per (encoder SHA-256, prefix, ordered input texts) under experiments/cache/
(not in git). Delete the folder to recompute.

    python experiments/tier3/embed.py --time 200     # Ch. 33.3: time 200 items, extrapolate
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "data"))
from prep import norm_text  # noqa: E402

LLAMA_EMBEDDING = ROOT / "third_party" / "llama.cpp" / "build" / "bin" / "llama-embedding"
MODELS_MANIFEST = ROOT / "models" / "MANIFEST.toml"
CACHE_DIR = ROOT / "experiments" / "cache" / "embeddings"

# Input prefix per encoder family, from each model card.
PREFIX_BY_FAMILY = {"xlm-roberta": "query: ", "bert": ""}
# Every prompt is a short request; 2048 tokens per batch holds dozens of them. Set explicitly:
# the CLI default context (131072) allocates far more than an encoder needs.
CTX_TOKENS = 2048


@dataclass(frozen=True)
class Encoder:
    name: str
    path: Path | None
    sha256: str
    prefix: str


def load_encoder(name: str, manifest: Path = MODELS_MANIFEST) -> Encoder:
    """The manifest entry `name`, which must have role = "encoder"."""
    for m in tomllib.loads(manifest.read_text())["model"]:
        if m["name"] == name and m["role"] == "encoder":
            return Encoder(name, ROOT / "models" / m["file"], m["sha256"], PREFIX_BY_FAMILY[m["family"]])
    raise KeyError(f"no encoder named {name!r} in {manifest}")


def model_input(text: str, prefix: str) -> str:
    return prefix + norm_text(text)


def parse_array_output(stdout: str, n_expected: int) -> np.ndarray:
    """The `--embd-output-format array` block ([[...],[...]]) from llama-embedding's stdout."""
    start, end = stdout.find("[["), stdout.rfind("]]")
    if start < 0 or end < start:
        raise ValueError("no embedding array in llama-embedding output")
    rows = json.loads(stdout[start : end + 2])
    if len(rows) != n_expected:
        raise ValueError(f"got {len(rows)} embeddings, expected {n_expected}")
    if len({len(r) for r in rows}) != 1:
        raise ValueError("embeddings have different lengths")
    return np.asarray(rows, dtype=np.float32)


def run_llama_embedding(encoder: Encoder, inputs: list[str], threads: int = 6) -> np.ndarray:
    """One llama-embedding process over `inputs` (one per line), L2-normalized, model pooling."""
    if any("\n" in s for s in inputs):
        raise ValueError("inputs must be single lines (the CLI splits prompts on newlines)")
    with tempfile.NamedTemporaryFile("w", suffix=".txt", encoding="utf-8") as f:
        f.write("\n".join(inputs))
        f.flush()
        cmd = [str(LLAMA_EMBEDDING), "-m", str(encoder.path), "-f", f.name,
               "--embd-output-format", "array", "--embd-normalize", "2",
               "-c", str(CTX_TOKENS), "-b", str(CTX_TOKENS), "-t", str(threads)]
        done = subprocess.run(cmd, capture_output=True, text=True)
    if done.returncode != 0:
        raise RuntimeError(f"llama-embedding failed ({done.returncode}): {done.stderr[-2000:]}")
    return parse_array_output(done.stdout, len(inputs))


def cache_key(encoder: Encoder, texts: list[str]) -> str:
    h = hashlib.sha256()
    for part in (encoder.sha256, encoder.prefix, *texts):
        h.update(part.encode("utf-8"))
        h.update(b"\x00")  # separator, so ("ab", "c") and ("a", "bc") differ
    return h.hexdigest()


def embed(encoder: Encoder, texts: list[str], *, chunk: int = 512, cache_dir: Path = CACHE_DIR,
          runner=run_llama_embedding) -> np.ndarray:
    """[len(texts) x dim] float32 embeddings of `texts`, in order; cached on disk."""
    key = cache_key(encoder, texts)
    path = cache_dir / f"{encoder.name}-{key[:20]}.npy"
    if path.exists():
        return np.load(path)
    inputs = [model_input(t, encoder.prefix) for t in texts]
    parts = [runner(encoder, inputs[i : i + chunk]) for i in range(0, len(inputs), chunk)]
    x = np.concatenate(parts).astype(np.float32)
    cache_dir.mkdir(parents=True, exist_ok=True)
    np.save(path, x)
    path.with_suffix(".json").write_text(json.dumps(
        {"encoder": encoder.name, "sha256": encoder.sha256, "prefix": encoder.prefix,
         "n": len(texts), "dim": int(x.shape[1]), "key": key}, indent=1))
    return x


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--time", type=int, metavar="N", help="time N CLINC150 train texts per encoder")
    ap.add_argument("--cache", action="store_true", help="embed the dev splits (train/cal/val) of --datasets")
    ap.add_argument("--encoders", nargs="+", default=["bge-small-en-v1.5-q8_0", "multilingual-e5-small-q8_0"])
    ap.add_argument("--datasets", nargs="+", default=["intents_v0", "clinc150"])
    args = ap.parse_args(argv)
    from splits import load_splits
    if args.time:
        texts = [r["text"] for r in load_splits("clinc150")["train"][: args.time]]
        for name in args.encoders:
            enc = load_encoder(name)
            t0 = time.perf_counter()
            run_llama_embedding(enc, [model_input(t, enc.prefix) for t in texts])
            dt = time.perf_counter() - t0
            print(f"{name}: {args.time} texts in {dt:.2f} s ({1000 * dt / args.time:.2f} ms/text)")
    if args.cache:
        for name in args.encoders:
            enc = load_encoder(name)
            for ds in args.datasets:
                for split, rows in load_splits(ds).items():
                    t0 = time.perf_counter()
                    x = embed(enc, [r["text"] for r in rows], chunk=2048)
                    print(f"{name} {ds}/{split}: {x.shape} in {time.perf_counter() - t0:.1f} s", flush=True)


if __name__ == "__main__":
    main()
