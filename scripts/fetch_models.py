#!/usr/bin/env python3
"""Download the models listed in models/MANIFEST.toml and verify them (blueprint Ch. 44.3, 46).

The only networked step in the project. Standard library only. For each file:
skip if present with the right size and hash; otherwise download to `<file>.part`,
hash while streaming, and rename only if the SHA-256 matches. Writes models/SHA256SUMS,
so `cd models && sha256sum -c SHA256SUMS` re-checks everything offline.

Usage: python scripts/fetch_models.py [--only NAME ...] [--roles slm encoder ...]
"""

import argparse
import hashlib
import sys
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODELS = ROOT / "models"
CHUNK = 1 << 20


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while block := f.read(CHUNK):
            h.update(block)
    return h.hexdigest()


def download(url: str, dest: Path, expected_sha: str | None, expected_bytes: int | None) -> str:
    """Download url to dest atomically; return its sha256. Exits on mismatch."""
    if dest.exists() and (expected_bytes is None or dest.stat().st_size == expected_bytes):
        digest = sha256_of(dest)
        if expected_sha is None or digest == expected_sha:
            print(f"  ok (cached)  {dest.relative_to(MODELS)}", flush=True)
            return digest
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_name(dest.name + ".part")
    h = hashlib.sha256()
    done = 0
    req = urllib.request.Request(url, headers={"User-Agent": "kestrel-fetch"})
    with urllib.request.urlopen(req) as resp, part.open("wb") as out:
        total = expected_bytes or int(resp.headers.get("Content-Length") or 0)
        while block := resp.read(CHUNK):
            out.write(block)
            h.update(block)
            done += len(block)
            if total:
                print(f"\r  {dest.name}: {done / total:6.1%} of {total / 1e9:.2f} GB", end="", flush=True)
    print()
    digest = h.hexdigest()
    if expected_sha is not None and digest != expected_sha:
        part.unlink()
        sys.exit(f"SHA-256 MISMATCH for {dest.name}: expected {expected_sha}, got {digest}")
    if expected_bytes is not None and done != expected_bytes:
        part.unlink()
        sys.exit(f"SIZE MISMATCH for {dest.name}: expected {expected_bytes}, got {done}")
    part.rename(dest)
    print(f"  ok (fetched) {dest.relative_to(MODELS)}", flush=True)
    return digest


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", nargs="*", help="model names to fetch (default: all)")
    ap.add_argument("--roles", nargs="*", help="roles to fetch, e.g. slm encoder encoder-source")
    args = ap.parse_args()

    manifest = tomllib.loads((MODELS / "MANIFEST.toml").read_text())
    sums: dict[str, str] = {}
    sums_path = MODELS / "SHA256SUMS"
    if sums_path.exists():
        for line in sums_path.read_text().splitlines():
            digest, name = line.split(maxsplit=1)
            sums[name.lstrip("*")] = digest

    for m in manifest["model"]:
        if args.only and m["name"] not in args.only:
            continue
        if args.roles and m["role"] not in args.roles:
            continue
        print(f"{m['name']}  ({m['repo']}@{m['revision'][:8]}, {m['license']})", flush=True)
        if "url" in m:
            dest = MODELS / m["file"]
            sums[m["file"]] = download(m["url"], dest, m.get("sha256"), m.get("bytes"))
        elif "converted" in m:
            dest = MODELS / m["file"]
            if not dest.exists():
                print(f"  missing      {m['file']}: run {m['converted']} after fetching its source")
                continue
            digest = sha256_of(dest)
            if digest != m["sha256"]:
                sys.exit(f"SHA-256 MISMATCH for {m['file']}: expected {m['sha256']}, got {digest}")
            print(f"  ok (local)   {m['file']}")
            sums[m["file"]] = digest
        for f in m.get("files", []):
            rel = f"{m['dir']}/{f['file']}"
            url = f"https://huggingface.co/{m['repo']}/resolve/{m['revision']}/{f['file']}"
            sums[rel] = download(url, MODELS / rel, f.get("sha256"), f.get("bytes"))

    sums_path.write_text("".join(f"{d}  {n}\n" for n, d in sorted(sums.items())))
    print(f"wrote {sums_path.relative_to(ROOT)} ({len(sums)} files)")


if __name__ == "__main__":
    main()
