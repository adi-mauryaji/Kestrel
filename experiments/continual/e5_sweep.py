"""experiments/continual/e5_sweep.py -- E5 dev: equal-budget sweeps of naive / replay / EWC (Ch. 22, ADR-027).

    python experiments/continual/e5_sweep.py --dry-run     # the grid and the run count; nothing runs
    python experiments/continual/e5_sweep.py --jobs 5      # CPU minutes: asked before it runs (ADR-027)

Every configuration runs on 3 domain orders x 5 seeds (5 stages x 2 CLINC150 domains; dev: train -> val).
12 configurations per method, and the baseline is never tuned less than a mitigation: naive's lr x epochs
grid contains every lr x epochs pair the others use. A method's configuration is the one with the best
mean average accuracy on val over its runs, among configurations whose runs all completed (an EWC step
with lr x lam x max F >= 2 is refused as unstable); ties go to the earlier configuration. Controls at the
selected naive lr / epochs: joint (upper bound) and stage0 (lower bound). eps2 = 2 x SD of the selected
naive configuration's average forgetting over its runs (ADR-027). The lam grid follows the Fisher
measured on this data (max ~5e-4 per stage, summed over up to 4 stages). Out:
experiments/logs/E5/sweep_dev.json with every run's R matrix.
"""
from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from cl_head import run_sequence, stages  # noqa: E402

ORDERS = (1, 2, 3)
SEEDS = (1, 2, 3, 4, 5)
METHODS = ("naive", "replay", "ewc")
GRID = {
    "naive": [{"lr": lr, "epochs": e} for lr in (0.3, 1.0, 3.0, 10.0) for e in (2, 5, 10)],
    "replay": [{"per_class": p, "lr": lr, "epochs": e} for p in (5, 20, 50) for lr in (1.0, 3.0) for e in (5, 10)],
    "ewc": [{"lam": lam, "lr": lr, "epochs": e} for lam in (10.0, 100.0, 300.0) for lr in (1.0, 3.0) for e in (5, 10)],
}
METRICS = ("avg_accuracy", "avg_forgetting", "bwt", "seconds", "buffer_bytes")
OUT = ROOT / "experiments" / "logs" / "E5" / "sweep_dev.json"

_SETS: dict = {}


def _init(sets):
    _SETS.clear()
    _SETS.update(sets)


def _one(job) -> dict:
    method, config, cfg, order, seed, d, k = job
    train, val = _SETS[order]
    row = {"method": method, "config": config, "cfg": cfg, "order": order, "seed": seed}
    try:
        return row | run_sequence(method, train, val, d=d, k=k, cfg=cfg, seed=seed)
    except FloatingPointError as e:
        return row | {"error": str(e)}


def _stats(v):
    return {"mean": statistics.fmean(v), "sd": statistics.stdev(v) if len(v) > 1 else 0.0} if v else None


def summarize(runs, method, grid) -> list[dict]:
    out = []
    for i, cfg in enumerate(grid[method]):
        mine = [r for r in runs if r["method"] == method and r["config"] == i]
        ok = [r for r in mine if "error" not in r]
        out.append({"config": i, "cfg": cfg, "n": len(mine), "n_ok": len(ok)}
                   | {m: _stats([r[m] for r in ok]) for m in METRICS})
    return out


def select(summary) -> int:
    """The best mean average accuracy among configurations whose runs all completed; ties: the earlier."""
    eligible = [s for s in summary if s["n_ok"] == s["n"] and s["n"] > 0]
    if not eligible:
        raise ValueError("no configuration completed all its runs")
    best = max(s["avg_accuracy"]["mean"] for s in eligible)
    return min(s["config"] for s in eligible if s["avg_accuracy"]["mean"] == best)


def _run(jobs, sets, n_jobs):
    if n_jobs <= 1:
        _init(sets)
        return [_one(j) for j in jobs]
    import multiprocessing as mp
    for var in ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS", "MKL_NUM_THREADS"):
        os.environ[var] = "1"  # one BLAS thread per worker process
    with mp.get_context("forkserver").Pool(n_jobs, initializer=_init, initargs=(sets,)) as pool:
        return pool.map(_one, jobs, chunksize=4)


def run_all(sets: dict, *, d: int, k: int, grid=GRID, seeds=SEEDS, jobs: int = 1) -> dict:
    """sets: {order: (train stages, val stages)}. Sweeps, selection, controls, eps2."""
    todo = [(m, i, cfg, o, s, d, k) for m in METHODS for i, cfg in enumerate(grid[m]) for o in sets for s in seeds]
    runs = _run(todo, sets, jobs)
    summary = {m: summarize(runs, m, grid) for m in METHODS}
    selected = {}
    for m in METHODS:
        i = select(summary[m])
        selected[m] = {"config": i, "cfg": grid[m][i], "summary": summary[m][i]}
    base = {"lr": selected["naive"]["cfg"]["lr"], "epochs": selected["naive"]["cfg"]["epochs"]}
    controls = {"joint": [base], "stage0": [base]}
    runs += _run([(m, 0, base, o, s, d, k) for m in controls for o in sets for s in seeds], sets, jobs)
    for m in controls:
        selected[m] = {"config": 0, "cfg": base, "summary": summarize(runs, m, controls)[0]}
    naive = [r["avg_forgetting"] for r in runs
             if r["method"] == "naive" and r["config"] == selected["naive"]["config"] and "error" not in r]
    return {"runs": runs, "summary": summary, "selected": selected,
            "eps2_forgetting": 2 * statistics.stdev(naive), "eps2_runs": len(naive)}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--jobs", type=int, default=5)
    args = ap.parse_args(argv)
    n = (sum(len(GRID[m]) for m in METHODS) + 2) * len(ORDERS) * len(SEEDS)
    print(json.dumps({m: GRID[m] for m in METHODS}), f"\n{n} runs (orders {ORDERS}, seeds {SEEDS})")
    if args.dry_run:
        return 0
    from e5_data import by_stage, load
    data = load()
    order_stages = {o: stages(list(data["domains"]), o) for o in ORDERS}
    sets = {o: (by_stage(data, "train", sd), by_stage(data, "val", sd)) for o, sd in order_stages.items()}
    t0 = time.perf_counter()
    out = run_all(sets, d=data["x"]["train"].shape[1], k=len(data["classes"]), jobs=args.jobs)
    report = {"experiment": "E5 dev sweep (ADR-027)", "data": "clinc150 dev, e5 Q8_0 cache, train -> val",
              "orders": {str(o): sd for o, sd in order_stages.items()}, "seeds": list(SEEDS), "grid": GRID,
              "selection": "best mean avg_accuracy on val among complete configs; ties -> earlier",
              "wall_seconds": time.perf_counter() - t0, "jobs": args.jobs} | out
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(report, indent=1) + "\n")
    for m, s in out["selected"].items():
        sm = s["summary"]
        print(f"{m:7s} cfg {s['cfg']}  acc {sm['avg_accuracy']['mean']:.3f}±{sm['avg_accuracy']['sd']:.3f}  "
              f"forget {sm['avg_forgetting']['mean']:.3f}±{sm['avg_forgetting']['sd']:.3f}  "
              f"bwt {sm['bwt']['mean']:.3f}  s/run {sm['seconds']['mean']:.1f}")
    print(f"eps2 = {out['eps2_forgetting']:.4f} from {out['eps2_runs']} naive runs -> {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
