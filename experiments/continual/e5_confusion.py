"""experiments/continual/e5_confusion.py -- E5: forgetting or interference? (Ch. 22.4 (c); Week 12)

    python experiments/continual/e5_confusion.py [--jobs 5]    # ~1-2 min on 5 processes

Ch. 22.4 separates real forgetting from (a) a test shift and (b) label-space interference. The stage
sets are fixed across stages, so (a) cannot happen here; joint and stage0 are the bounds. This reruns
the sweep's selected configurations (logs/E5/sweep_dev.json; same data, orders and seeds) with
`confusion`, which says where each stage's items go after every stage: correct, into a newer stage's
classes, or into another class of their own or an older stage. Reported per method, averaged over the
old stages and the 15 runs: after the last stage, and right after the next stage (the first update
that could hurt them). Out: experiments/logs/E5/confusion_dev.json.
"""
from __future__ import annotations

import argparse
import json
import statistics
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from cl_head import stages  # noqa: E402
from e5_sweep import OUT as SWEEP, ORDERS, SEEDS, _run  # noqa: E402

METHODS = ("naive", "replay", "ewc", "joint")
OUT = ROOT / "experiments" / "logs" / "E5" / "confusion_dev.json"
SHARES = ("correct", "newer", "older")


def shares(runs) -> dict:
    """Per method: mean shares of the old stages' items after the last stage and after the next."""
    out = {}
    for m in sorted({r["method"] for r in runs}):
        last, nxt = [], []
        for r in (r for r in runs if r["method"] == m):
            C = r["C"]
            t = len(C)
            last += [C[t - 1][j] for j in range(t - 1)]
            nxt += [C[j + 1][j] for j in range(t - 1)]
        mean = lambda cells: {k: statistics.fmean(c[k] for c in cells) for k in SHARES}  # noqa: E731
        out[m] = {"runs": sum(r["method"] == m for r in runs), "after_last": mean(last), "after_next": mean(nxt)}
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--jobs", type=int, default=5)
    args = ap.parse_args(argv)
    sweep = json.loads(SWEEP.read_text())
    cfgs = {m: sweep["selected"][m]["cfg"] | {"confusion": True} for m in METHODS}
    from e5_data import by_stage, load
    data = load()
    order_stages = {o: stages(list(data["domains"]), o) for o in ORDERS}
    sets = {o: (by_stage(data, "train", sd), by_stage(data, "val", sd)) for o, sd in order_stages.items()}
    d, k = data["x"]["train"].shape[1], len(data["classes"])
    t0 = time.perf_counter()
    runs = _run([(m, 0, cfgs[m], o, s, d, k) for m in METHODS for o in ORDERS for s in SEEDS], sets, args.jobs)
    failed = [r for r in runs if "error" in r]
    if failed:
        raise SystemExit(f"{len(failed)} runs failed: {failed[0]['error']}")
    out = {"analysis": "E5 dev, Ch. 22.4 (c): where the old stages' items go", "configs": cfgs,
           "orders": {str(o): sd for o, sd in order_stages.items()}, "seeds": list(SEEDS),
           "wall_seconds": time.perf_counter() - t0, "shares": shares(runs),
           "runs": [{k2: r[k2] for k2 in ("method", "order", "seed", "C", "R")} for r in runs]}
    OUT.write_text(json.dumps(out, indent=1) + "\n")
    for m, v in out["shares"].items():
        a, n = v["after_last"], v["after_next"]
        print(f"{m:7s} after last: correct {a['correct']:.3f} newer {a['newer']:.3f} older {a['older']:.3f} | "
              f"after next: correct {n['correct']:.3f} newer {n['newer']:.3f} older {n['older']:.3f}")
    print(f"{len(runs)} runs in {out['wall_seconds']:.0f} s -> {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
