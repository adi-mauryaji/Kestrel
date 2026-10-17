"""experiments/tier3/head.py -- Tier-3 head: train, calibrate, export (blueprint Ch. 33.2 steps 3-6).

- train_head: softmax (multinomial logistic) regression on frozen embeddings; one row per class,
  rows in label-index order.
- fit_temperature: one scalar T minimizing the mean NLL on the calibration split (Guo et al.
  2017). Bounded search on log T instead of A.3's fixed grid [0.25, 8]: no grid error, and an
  optimum at the bound is an error instead of a silent clip.
- export_head: the Python -> Rust contract. head.bin = little-endian f32, row-major [K x D];
  head_bias.bin = [K] f32; head.json = classes, dim, temperature + provenance. T is rounded to
  f32 first, so both sides use the identical value.
- write_parity_fixture: a small seeded head + 100 inputs + Python's probabilities, which the Rust
  LinearHead test must reproduce (datasets/contracts/head_parity/).
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

import numpy as np
from scipy.optimize import minimize_scalar
from sklearn.linear_model import LogisticRegression

ROOT = Path(__file__).resolve().parents[2]
INTENTS_CONTRACT = ROOT / "datasets" / "contracts" / "intents.txt"
PARITY_DIR = ROOT / "datasets" / "contracts" / "head_parity"
FORMAT = 1
T_BOUNDS = (0.05, 20.0)
_RESERVED = {"format", "classes", "dim", "temperature", "weights", "bias", "weights_sha256", "bias_sha256"}


def logits(x, w, b):
    return np.asarray(x) @ np.asarray(w).T + np.asarray(b)


def softmax(z, t=1.0):
    z = np.asarray(z, dtype=np.float64) / t
    z = z - z.max(axis=1, keepdims=True)  # stable: the largest exponent is exp(0)
    e = np.exp(z)
    return e / e.sum(axis=1, keepdims=True)


def nll(z, y, t):
    """Mean negative log-likelihood of labels y under softmax(z / t)."""
    s = np.asarray(z, dtype=np.float64) / t
    m = s.max(axis=1, keepdims=True)
    log_norm = m[:, 0] + np.log(np.exp(s - m).sum(axis=1))
    return float(np.mean(log_norm - s[np.arange(len(y)), y]))


def fit_temperature(z, y) -> float:
    # With no misclassified item the NLL falls monotonically as T -> 0 and flattens out, so the
    # optimizer would stop anywhere on the plateau: T is not identifiable from such data.
    if (np.asarray(z).argmax(axis=1) == np.asarray(y)).all():
        raise ValueError("calibration set has no errors: temperature is not identifiable")
    lo, hi = np.log(T_BOUNDS[0]), np.log(T_BOUNDS[1])
    res = minimize_scalar(lambda lt: nll(z, y, np.exp(lt)), bounds=(lo, hi), method="bounded",
                          options={"xatol": 1e-6})
    if min(res.x - lo, hi - res.x) < 1e-3:
        raise ValueError(f"temperature optimum at the search bound {T_BOUNDS}: {np.exp(res.x):.4g}")
    return float(np.exp(res.x))


def train_head(x, y, n_classes: int, c: float):
    """(W [K x D] f32, b [K] f32) for labels y in 0..n_classes-1; L2 penalty 1/c."""
    missing = sorted(set(range(n_classes)) - set(np.unique(y).tolist()))
    if missing:
        raise ValueError(f"class {missing[0]} has no training example")
    clf = LogisticRegression(C=c, max_iter=10000, tol=1e-6).fit(x, y)
    w, b = clf.coef_, clf.intercept_
    if n_classes == 2:  # sklearn keeps one row for binary; softmax([-a/2, a/2]) = sigmoid(a)
        w, b = np.vstack([-w / 2, w / 2]), np.array([-b[0] / 2, b[0] / 2])
    return w.astype(np.float32), b.astype(np.float32)


def export_head(out_dir, w, b, t, classes, meta: dict) -> Path:
    w, b, t = np.asarray(w, dtype=np.float32), np.asarray(b, dtype=np.float32), float(np.float32(t))
    if w.ndim != 2 or b.shape != (w.shape[0],) or len(classes) != w.shape[0]:
        raise ValueError(f"shapes disagree: W {w.shape}, b {b.shape}, {len(classes)} classes")
    if not (np.isfinite(w).all() and np.isfinite(b).all() and np.isfinite(t) and t > 0):
        raise ValueError("weights, bias and temperature must be finite, temperature > 0")
    if _RESERVED & set(meta):
        raise ValueError(f"meta may not set {sorted(_RESERVED & set(meta))}")
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    wb, bb = w.astype("<f4").tobytes(order="C"), b.astype("<f4").tobytes()
    (out / "head.bin").write_bytes(wb)
    (out / "head_bias.bin").write_bytes(bb)
    head = {"format": FORMAT, "classes": list(classes), "dim": int(w.shape[1]), "temperature": t,
            "weights": "head.bin", "bias": "head_bias.bin",
            "weights_sha256": hashlib.sha256(wb).hexdigest(), "bias_sha256": hashlib.sha256(bb).hexdigest(),
            **meta}
    (out / "head.json").write_text(json.dumps(head, indent=1) + "\n")
    return out


def load_head(head_dir):
    d = Path(head_dir)
    meta = json.loads((d / "head.json").read_text())
    k, dim = len(meta["classes"]), meta["dim"]
    w = np.frombuffer((d / meta["weights"]).read_bytes(), dtype="<f4").reshape(k, dim)
    b = np.frombuffer((d / meta["bias"]).read_bytes(), dtype="<f4")
    if b.shape != (k,):
        raise ValueError(f"bias has {b.shape[0]} values, expected {k}")
    return w.astype(np.float32), b.astype(np.float32), meta["temperature"], meta


def write_parity_fixture(out_dir=PARITY_DIR, seed: int = 0, n: int = 100, dim: int = 16) -> Path:
    """Seeded head over the 9 Kestrel intents + n unit-length inputs + float64 probabilities."""
    classes = [l for l in INTENTS_CONTRACT.read_text().split("\n") if l and not l.startswith("#")]
    rng = np.random.default_rng(seed)
    w = rng.normal(0.0, 2.0, size=(len(classes), dim)).astype(np.float32)
    b = rng.normal(0.0, 1.0, size=len(classes)).astype(np.float32)
    t = float(np.float32(1.7))
    x = rng.normal(size=(n, dim))
    x = (x / np.linalg.norm(x, axis=1, keepdims=True)).astype(np.float32)
    p = softmax(logits(x.astype(np.float64), w.astype(np.float64), b.astype(np.float64)), t)
    out = export_head(out_dir, w, b, t, classes,
                      {"purpose": "Python<->Rust LinearHead parity fixture (experiments/tier3/head.py)", "seed": seed})
    cases = {"tolerance": 1e-5, "inputs": x.astype(np.float64).tolist(), "probs": p.tolist()}
    (out / "cases.json").write_text(json.dumps(cases) + "\n")
    return out


if __name__ == "__main__":
    print(write_parity_fixture())
