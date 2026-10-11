"""experiments/analysis/e0_summary.py -- per-session and across-session E0 summaries (Week-2 "T").

    python experiments/analysis/e0_summary.py experiments/logs/E0/<session>.csv [more sessions ...]

Per session: mean and CV over the measured rounds (warm-up rounds are excluded) of tok/s for
pp/tg/embd tests and of wall seconds for startup, plus the largest peak RSS.
With 2+ sessions: the mean and CV of the per-session means -- the repeatability check.
Pass only sessions run under the same protocol (see experiments/logs/E0/README.md).
"""
from __future__ import annotations

import csv
import statistics as st
import sys
from collections import defaultdict


def _cv_pct(values):
    return st.stdev(values) / st.mean(values) * 100 if len(values) > 1 else None


def per_session(rows):
    """{(model, test): {n, mean, cv_pct, unit, rss_max_mib}} over one session's measured rounds."""
    if len({r["session"] for r in rows}) > 1:
        raise ValueError("per_session takes rows from one session")
    groups = defaultdict(list)
    for r in rows:
        if r["phase"] == "measure":
            groups[(r["model"], r["test"])].append(r)
    out = {}
    for key, rs in groups.items():
        startup = key[1] == "startup"
        values = [float(r["wall_s"] if startup else r["avg_tok_s"]) for r in rs]
        out[key] = {"n": len(values), "mean": st.mean(values), "cv_pct": _cv_pct(values),
                    "unit": "s" if startup else "tok/s",
                    "rss_max_mib": max(float(r["peak_rss_mib"]) for r in rs)}
    return out


def across_sessions(sessions):
    """{(model, test): {n_sessions, mean, cv_pct}} from a list of per_session() results."""
    means = defaultdict(list)
    for s in sessions:
        for key, v in s.items():
            means[key].append(v["mean"])
    return {key: {"n_sessions": len(v), "mean": st.mean(v), "cv_pct": _cv_pct(v)}
            for key, v in means.items()}


def _fmt(x, nd=1):
    return "-" if x is None else f"{x:.{nd}f}"


def main(paths):
    sessions = []
    for path in paths:
        with open(path, newline="") as f:
            rows = list(csv.DictReader(f))
        s = per_session(rows)
        sessions.append(s)
        print(f"\n{path}  (measured rounds per test: {sorted({v['n'] for v in s.values()})})")
        print(f"  {'model':28} {'test':8} {'mean':>9} {'unit':5} {'CV% rounds':>10} {'RSS MiB':>8}")
        for (model, test), v in sorted(s.items()):
            print(f"  {model:28} {test:8} {_fmt(v['mean'], 2):>9} {v['unit']:5} "
                  f"{_fmt(v['cv_pct']):>10} {v['rss_max_mib']:8.0f}")
    if len(sessions) > 1:
        print(f"\nAcross {len(sessions)} sessions (CV of session means):")
        for (model, test), v in sorted(across_sessions(sessions).items()):
            print(f"  {model:28} {test:8} {_fmt(v['mean'], 2):>9} CV {_fmt(v['cv_pct']):>5}%  "
                  f"(n={v['n_sessions']})")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1:])
