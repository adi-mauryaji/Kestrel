"""experiments/repro.py -- the clean-checkout reproduction check (Week 15, Ch. 40.3).

    python experiments/repro.py              # clone HEAD, regenerate every analysis from the committed logs, compare
    python experiments/repro.py --rerun-e5   # also rerun E5 on test in the clone (~2 min) and compare its numbers

A fresh clone of the committed HEAD (a dirty tree is refused: what is reproduced is what is committed) gets the
pinned raw datasets (copied from datasets/raw/ after the SHA-256 check of datasets/MANIFEST.toml, which is what
scripts/fetch_datasets.py does from the network), then runs every analysis script in the clone on the
committed raw logs; each output must be byte-identical to the committed one (fixed seeds, no timestamps).
With --rerun-e5 the clone also gets the embedding cache (derived data, experiments/tier3/train.py rebuilds it)
and reruns E5 on test: accuracies, forgetting, BWT, buffer sizes and H5 must equal the committed analysis
(update seconds are timings and are left out). It uses this interpreter (the pinned requirements.lock
environment). On a second machine follow experiments/README_REPRO.md instead. Out:
experiments/logs/repro/<UTC>.json.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "experiments" / "logs" / "repro"
A = "experiments/analysis"
L = "experiments/logs"
STEPS = [([f"{A}/analyze_e{e}.py", "--dataset", d, "--split", "test"], [f"{L}/E{e}/analysis_{d}_test.json"])
         for e in (1, 2, 3) for d in ("intents_v0", "clinc150")] + [
    ([f"{A}/analyze_e5.py"], [f"{L}/E5/analysis_test.json"]),
    ([f"{A}/family.py"], [f"{L}/family_v1.json"]),
    ([f"{A}/results.py"], ["experiments/RESULTS.md"]),
    ([f"{A}/errors.py"], ["experiments/ERROR_ANALYSIS.md", f"{L}/errors_test.json"]),
]


def compare(a: Path, b: Path, rels) -> dict[str, str]:
    """Per file: identical, differs, or missing (in either tree)."""
    out = {}
    for rel in rels:
        pa, pb = a / rel, b / rel
        out[rel] = "missing" if not (pa.exists() and pb.exists()) else \
            "identical" if pa.read_bytes() == pb.read_bytes() else "differs"
    return out


def copy_verified(entries, src: Path, dst: Path) -> None:
    """Copy each manifest file from src to dst, refusing one whose SHA-256 is not the manifest's."""
    for e in entries:
        f = src / e["file"]
        if hashlib.sha256(f.read_bytes()).hexdigest() != e["sha256"]:
            raise ValueError(f"{e['file']}: SHA-256 differs from datasets/MANIFEST.toml")
        (dst / e["file"]).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(f, dst / e["file"])


def e5_numbers(report: dict) -> dict:
    """E5's analysis without the timings (update seconds), which no rerun reproduces."""
    table = {m: {k: v for k, v in s.items() if k != "seconds"} for m, s in report["table_r6"].items() if s}
    return table | {"h5": {k: report["h5"][k] for k in ("diff", "ci95", "p")}}


def _run(cmd, cwd: Path) -> dict:
    p = subprocess.run([sys.executable, *cmd], cwd=cwd, capture_output=True, text=True, timeout=3600)
    return {"cmd": " ".join(cmd), "exit": p.returncode, "stderr_tail": p.stderr[-300:]}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--rerun-e5", action="store_true")
    args = ap.parse_args(argv)
    if subprocess.run(["git", "status", "--porcelain"], capture_output=True, text=True, cwd=ROOT).stdout.strip():
        sys.exit("refused: the working tree is not clean (only committed state is reproduced)")
    head = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, cwd=ROOT).stdout.strip()
    report = {"date": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"), "head": head,
              "python": sys.executable}
    with tempfile.TemporaryDirectory(prefix="kestrel-repro-") as d:
        clone = Path(d) / "kestrel"
        subprocess.run(["git", "clone", "--quiet", "--no-hardlinks", str(ROOT), str(clone)], check=True)
        manifest = tomllib.loads((clone / "datasets" / "MANIFEST.toml").read_text())["dataset"]
        copy_verified(manifest, ROOT / "datasets" / "raw", clone / "datasets" / "raw")
        report["steps"] = [_run(cmd, clone) for cmd, _ in STEPS]
        report["outputs"] = compare(ROOT, clone, [o for _, outs in STEPS for o in outs])
        if args.rerun_e5:
            shutil.copytree(ROOT / "experiments" / "cache", clone / "experiments" / "cache")
            report["e5_rerun"] = [_run(["experiments/continual/e5_test.py", "--jobs", "5"], clone),
                                  _run([f"{A}/analyze_e5.py"], clone)]
            mine = e5_numbers(json.loads((ROOT / "experiments/logs/E5/analysis_test.json").read_text()))
            theirs = e5_numbers(json.loads((clone / "experiments/logs/E5/analysis_test.json").read_text()))
            report["e5_numbers_equal"] = mine == theirs
    ok = all(s["exit"] == 0 for s in report["steps"]) and all(v == "identical" for v in report["outputs"].values())
    ok = ok and report.get("e5_numbers_equal", True)
    report["reproduced"] = ok
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    path = OUT_DIR / (report["date"].replace(":", "").replace("-", "").replace("+0000", "Z") + ".json")
    path.write_text(json.dumps(report, indent=1) + "\n")
    for rel, v in report["outputs"].items():
        print(f"{v:9s}  {rel}")
    if args.rerun_e5:
        print(f"E5 rerun numbers equal: {report['e5_numbers_equal']}")
    print(f"{'REPRODUCED' if ok else 'NOT REPRODUCED'} at {head[:7]} -> {path.relative_to(ROOT)}")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
