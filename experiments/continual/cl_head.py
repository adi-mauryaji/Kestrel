"""experiments/continual/cl_head.py -- E5's head updates (Ch. 21-22, A.14 adapted; ADR-027).

A softmax-regression head on FROZEN encoder embeddings, updated stage by stage:
- naive: minibatch SGD on the new stage's data only (the baseline every mitigation is compared with);
- replay: the new data plus `per_class` stored examples of every class of the earlier stages;
- ewc: the new data with the penalty (lam/2) * sum F (W - W*)^2, F = the diagonal empirical Fisher
  summed over the earlier stages (online EWC), W* = the weights after the last stage; the bias is
  not penalized;
- joint: a fresh head on all stages so far (the upper bound, Ch. 22.4);
- stage0: trained on the first stage and never updated (the lower bound).

Changes from A.14: minibatch SGD with a seeded shuffle (A.14's full-batch descent from zero weights
has no seed variance, so its seeds varied nothing but the replay draw); every task is evaluated after
every stage (A.14 wrote 0.0 for tasks not yet seen); cost columns (update seconds, buffer bytes); the
two controls. `toy_tasks` makes synthetic data for tests: toy numbers are pipeline checks, not findings.
"""
from __future__ import annotations

import sys
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments" / "analysis"))
sys.path.insert(0, str(ROOT / "experiments" / "data"))
from metrics import forgetting  # noqa: E402
from prep import seeded_order  # noqa: E402

STRATEGIES = ("naive", "replay", "ewc", "joint", "stage0")


def softmax(z):
    z = z - z.max(axis=1, keepdims=True)
    e = np.exp(z)
    return e / e.sum(axis=1, keepdims=True)


class Head:
    def __init__(self, d: int, k: int):
        self.W = np.zeros((d, k))
        self.b = np.zeros(k)

    def logits(self, X):
        return X @ self.W + self.b

    def grad(self, X, y, ewc=None):
        """Mean NLL gradient (+ the EWC penalty's gradient on W)."""
        P = softmax(self.logits(X))
        P[np.arange(len(y)), y] -= 1.0
        gW, gb = X.T @ P / len(y), P.mean(axis=0)
        if ewc is not None:  # d/dW of (lam/2) * sum F * (W - W*)^2
            gW = gW + ewc["lam"] * ewc["F"] * (self.W - ewc["W_star"])
        return gW, gb

    def fit(self, X, y, *, epochs: int, lr: float, batch: int = 64, seed: int = 0, ewc=None):
        """Minibatch SGD; the shuffle of every epoch comes from `seed`. Raises FloatingPointError if
        the weights stop being finite (lr too large, or lr * lam * max F >= 2 under EWC)."""
        rng = np.random.default_rng(seed)
        with np.errstate(over="ignore", invalid="ignore"):
            for _ in range(epochs):
                order = rng.permutation(len(y))
                for s in range(0, len(y), batch):
                    idx = order[s:s + batch]
                    gW, gb = self.grad(X[idx], y[idx], ewc)
                    self.W -= lr * gW
                    self.b -= lr * gb
                if not (np.isfinite(self.W).all() and np.isfinite(self.b).all()):
                    raise FloatingPointError(f"SGD diverged (lr {lr})")

    def accuracy(self, X, y) -> float:
        return float((self.logits(X).argmax(axis=1) == y).mean())

    def fisher_diag(self, X, y):
        """Empirical diagonal Fisher: the mean squared per-example gradient, x_d^2 (p_k - y_k)^2."""
        P = softmax(self.logits(X))
        P[np.arange(len(y)), y] -= 1.0
        return (X ** 2).T @ (P ** 2) / len(y)


def stages(domains, order_seed, per_stage: int = 2) -> list[list[str]]:
    """Domains in stages of `per_stage`: sorted (order_seed None) or a seeded permutation of the names."""
    if not domains or len(domains) % per_stage:
        raise ValueError(f"{len(domains)} domains do not split into stages of {per_stage}")
    names = sorted(domains)
    if order_seed is not None:
        names = seeded_order(names, f"e5-order:{order_seed}")
    return [names[i:i + per_stage] for i in range(0, len(names), per_stage)]


def per_class_sample(X, y, n: int, seed: int):
    """Up to n examples of every class in y (seeded, without replacement)."""
    rng = np.random.default_rng(seed)
    idx = np.concatenate([rng.choice(np.flatnonzero(y == c), min(n, int((y == c).sum())), replace=False)
                          for c in np.unique(y)])
    return X[idx], y[idx]


def run_sequence(strategy: str, train, evals, *, d: int, k: int, cfg: dict, seed: int) -> dict:
    """train / evals: one (X, y) per stage. cfg: lr, epochs (+ batch), per_class (replay), lam (ewc),
    confusion (bool). Returns the matrix R[i][j] (accuracy on stage j's eval items after stage i), its
    forgetting summary, update seconds and the replay buffer's size; with confusion also C[i][j]: the
    shares of stage j's items that are correct, predicted as a class of a newer stage (or a class no
    stage has trained), or as another class of stage j or older (Ch. 22.4's interference check)."""
    if strategy not in STRATEGIES:
        raise KeyError(f"unknown strategy {strategy!r}")

    def fit(h, X, y, t, ewc=None):
        h.fit(X, y, epochs=cfg["epochs"], lr=cfg["lr"], batch=cfg.get("batch", 64), seed=seed * 1000 + t,
              ewc=ewc)

    h, R, C, seconds = Head(d, k), [], [], 0.0
    stage_of = np.full(k, len(train))  # a class no stage trains counts as newer than every stage
    for t, (_, y) in reversed(list(enumerate(train))):
        stage_of[np.unique(y)] = t
    buf_X, buf_y, ewc = [], [], None
    for t, (X, y) in enumerate(train):
        t0 = time.perf_counter()
        if strategy == "joint":
            h = Head(d, k)
            fit(h, np.vstack([x for x, _ in train[:t + 1]]), np.concatenate([v for _, v in train[:t + 1]]), t)
        elif strategy == "stage0":
            if t == 0:
                fit(h, X, y, t)
        elif strategy == "replay":
            fit(h, np.vstack([X, *buf_X]), np.concatenate([y, *buf_y]), t)
            if t < len(train) - 1:
                bx, by = per_class_sample(X, y, cfg["per_class"], seed * 1000 + t)
                buf_X.append(bx)
                buf_y.append(by)
        elif strategy == "ewc":
            if ewc is not None and cfg["lr"] * cfg["lam"] * ewc["F"].max() >= 2:
                raise FloatingPointError(f"unstable EWC step: lr * lam * max F = "
                                         f"{cfg['lr'] * cfg['lam'] * ewc['F'].max():.2f} >= 2 (stage {t})")
            fit(h, X, y, t, ewc)
            F = h.fisher_diag(X, y)
            ewc = {"lam": cfg["lam"], "F": F if ewc is None else ewc["F"] + F, "W_star": h.W.copy()}
        else:
            fit(h, X, y, t)
        seconds += time.perf_counter() - t0
        R.append([h.accuracy(Xe, ye) for Xe, ye in evals])
        if cfg.get("confusion"):
            C.append([_where(h.logits(Xe).argmax(axis=1), ye, stage_of, j) for j, (Xe, ye) in enumerate(evals)])
    items = int(sum(len(v) for v in buf_y))
    out = {"R": R, "seconds": seconds, "buffer_items": items, "buffer_bytes": items * (d * 4 + 8), **forgetting(R)}
    return out | ({"C": C} if cfg.get("confusion") else {})


def _where(pred, y, stage_of, j: int) -> dict:
    """Shares of stage j's items: correct, wrong into a newer stage's classes, wrong otherwise."""
    right = pred == y
    newer = ~right & (stage_of[pred] > j)
    return {"correct": float(right.mean()), "newer": float(newer.mean()),
            "older": float((~right & ~newer).mean())}


def toy_tasks(n_tasks: int = 3, classes_per_task: int = 3, d: int = 20, n: int = 200, seed: int = 0):
    """Synthetic class-incremental tasks (A.14's make_tasks): ([(X, y)] train, [(X, y)] eval)."""
    rng = np.random.default_rng(seed)
    centers = rng.normal(0, 2.0, size=(n_tasks * classes_per_task, d))
    train, evals = [], []
    for t in range(n_tasks):
        cls = np.arange(t * classes_per_task, (t + 1) * classes_per_task)
        for out in (train, evals):
            y = rng.choice(cls, size=n)
            out.append((centers[y] + rng.normal(0, 1.0, size=(n, d)), y))
    return train, evals
