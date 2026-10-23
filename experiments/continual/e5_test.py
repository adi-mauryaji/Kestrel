"""experiments/continual/e5_test.py -- E5 on CLINC150 test (Week 14, PREREG §4, ADR-027): the frozen selection.

    python experiments/continual/e5_test.py --jobs 5     # ~5 min of CPU on 5 processes

Every method's selected configuration (configs/e5.json, FROZEN: naive, replay, EWC and the joint / stage0
controls) on the same 3 domain orders x 5 seeds as the dev sweep: train on each stage's train items, R[i][j] =
accuracy on stage j's TEST in-scope items after stage i. Refused unless the PREREG is frozen
(run_e1.guard_test). The test items are embedded through the runtime encoder (multilingual-e5-small Q8_0,
the cache's route; a miss runs the model). Out: experiments/logs/E5/test.json (every run's R matrix and the
per-method summary). The analysis (H5) is experiments/analysis/analyze_e5.py.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
from e5_sweep import METRICS, SEEDS, _run, _stats  # noqa: E402

OUT = ROOT / "experiments" / "logs" / "E5" / "test.json"
E5_CONFIG = ROOT / "experiments" / "configs" / "e5.json"


def jobs_for(selected: dict, *, orders, seeds, d: int, k: int) -> list[tuple]:
    """One job per method x order x seed, each with its selected configuration (index 0)."""
    return [(m, 0, cfg, o, s, d, k) for m, cfg in selected.items() for o in orders for s in seeds]


def summarize_test(runs, selected: dict) -> dict:
    out = {}
    for m in selected:
        ok = [r for r in runs if r["method"] == m and "error" not in r]
        out[m] = ({"runs": len(ok), "errors": sum(r["method"] == m and "error" in r for r in runs)}
                  | {x: _stats([r[x] for r in ok]) for x in METRICS}) if ok else None
    return out


def add_test(data: dict) -> dict:
    """data (e5_data.load) plus the in-scope CLINC150 test items, embedded on the runtime route."""
    sys.path.insert(0, str(ROOT / "experiments" / "data"))
    from clinc150 import kestrel_splits, load_official
    from e5_data import ENCODER
    from embed import embed, load_encoder
    from train import label_index

    rows = [r for r in kestrel_splits(load_official())["test"] if not r["oos"]]
    x = embed(load_encoder(ENCODER), [r["text"] for r in rows], chunk=2048)
    keep, y, _ = label_index(rows, data["classes"])
    data["x"]["test"], data["y"]["test"] = x[keep], y
    return data


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--jobs", type=int, default=5)
    args = ap.parse_args(argv)
    sys.path.insert(0, str(ROOT / "experiments" / "e1"))
    from run_e1 import guard_test

    cfg = json.loads(E5_CONFIG.read_text())
    guard_test([cfg])
    from e5_data import by_stage, load

    data = add_test(load())
    orders = cfg["orders"]
    sets = {o: (by_stage(data, "train", sd), by_stage(data, "test", sd)) for o, sd in orders.items()}
    t0 = time.perf_counter()
    runs = _run(jobs_for(cfg["selected"], orders=list(orders), seeds=cfg.get("seeds", SEEDS),
                         d=data["x"]["train"].shape[1], k=len(data["classes"])), sets, args.jobs)
    report = {"experiment": "E5 test (ADR-027, ADR-031)",
              "e5_json_sha256": hashlib.sha256(E5_CONFIG.read_bytes()).hexdigest(),
              "data": "clinc150: train -> test in-scope items, e5 Q8_0 runtime route",
              "n_test": int(len(data["y"]["test"])), "orders": orders, "selected": cfg["selected"],
              "wall_seconds": time.perf_counter() - t0, "jobs": args.jobs,
              "summary": summarize_test(runs, cfg["selected"]), "runs": runs}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(report, indent=1) + "\n")
    print(f"{len(runs)} runs, {report['wall_seconds']:.0f} s -> {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
