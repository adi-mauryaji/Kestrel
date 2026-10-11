#!/usr/bin/env python3
"""Download the datasets listed in datasets/MANIFEST.toml and verify them (blueprint Ch. 31, 44.3).

Raw files go to datasets/raw/ (gitignored). Same atomic download and SHA-256/size check as
fetch_models.py. Standard library only.

Usage: python scripts/fetch_datasets.py [--only NAME ...]
"""

import argparse
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fetch_models import ROOT, download  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", nargs="*")
    args = ap.parse_args()
    manifest = tomllib.loads((ROOT / "datasets" / "MANIFEST.toml").read_text())
    for d in manifest["dataset"]:
        if args.only and d["name"] not in args.only:
            continue
        print(f"{d['name']}  ({d['license']})", flush=True)
        download(d["url"], ROOT / "datasets" / "raw" / d["file"], d["sha256"], d["bytes"])


if __name__ == "__main__":
    main()
