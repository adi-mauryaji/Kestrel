#!/usr/bin/env python3
"""v3 step (a) (ADR-036): an SLM loaded with and without mmap, page cache cold and warm.

Each run is one process of the `generate` example (Kestrel's own llama.cpp adapter), so memory is
that process's alone: VmHWM and the RssAnon / RssFile split at the end (PREREG D12's fields).
Cold = the model file evicted from the page cache first (posix_fadvise DONTNEED, no root needed);
warm = the whole file read first; `fincore` records what was cached when each run started. The
four conditions rotate every round (Ch. 34.5), after warm-up rounds that are written but not
summarized (ADR-011). Decoding is greedy, so every run must print the same text: the summary
checks it byte for byte. Preconditions are bench_e0's (AC power, boost off) and every row records
them.

    cargo build --release -p kestrel-inference --features llama --example generate
    .venv/bin/python experiments/measure/bench_mmap.py [--model M] [--rounds 3] [--warmup 1]
    .venv/bin/python experiments/measure/bench_mmap.py --summarize ROWS.jsonl   (same bytes again)

Writes experiments/logs/v3/mmap/<UTC>.jsonl (one row per run) and <UTC>.md (the summary).
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

from bench_e0 import conditions, protocol_problems

ROOT = Path(__file__).resolve().parents[2]
EXAMPLE = ROOT / "target/release/examples/generate"
OUT = ROOT / "experiments/logs/v3/mmap"
PROMPT = "The three primary colours are"
TOKENS = 64

CONDITIONS = [("copy", "cold"), ("map", "cold"), ("copy", "warm"), ("map", "warm")]


def order(r: int) -> list[tuple[str, str]]:
    """Round r's run order: CONDITIONS rotated by r, so each one takes every position in turn."""
    k = r % len(CONDITIONS)
    return CONDITIONS[k:] + CONDITIONS[:k]


def warm(path: Path) -> None:
    with open(path, "rb") as f:
        while f.read(16 << 20):
            pass


def evict(path: Path) -> None:
    fd = os.open(path, os.O_RDONLY)
    try:
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
    finally:
        os.close(fd)


def cached_bytes(path: Path) -> int:
    out = subprocess.run(["fincore", "--bytes", "--noheadings", "--output", "RES", str(path)],
                         capture_output=True, text=True, check=True).stdout
    return int(out)


def summarize(rows: list[dict]) -> str:
    """Medians per condition (in CONDITIONS order) and whether every run printed the same bytes."""
    lines = ["| load | cache | n | load ms | first token ms | decode tok/s | anon MiB | file MiB |",
             "|---|---|---|---|---|---|---|---|"]
    for mode, cache in CONDITIONS:
        rs = [r for r in rows if (r["mode"], r["cache"]) == (mode, cache)]
        if not rs:
            continue
        med = lambda key: statistics.median(r[key] for r in rs)  # noqa: E731
        lines.append(f"| {mode} | {cache} | {len(rs)} | {med('load_ms'):.0f} | "
                     f"{med('ttft_first_ms'):.0f} | {med('decode_tok_per_s'):.1f} | "
                     f"{med('anon_kib') / 1024:.0f} | {med('file_kib') / 1024:.0f} |")
    texts = {r["text"] for r in rows}
    same = len(texts) == 1
    return ("\n".join(lines) + f"\n\nOutputs: {len(rows)} runs, {len(texts)} distinct "
            f"{'text' if same else 'texts'} ({'byte-identical' if same else 'NOT identical'}).\n")


def run(model: Path, mode: str, cache: str) -> dict:
    if cache == "cold":
        evict(model)
    else:
        warm(model)
    cached = cached_bytes(model)
    args = [str(EXAMPLE), str(model), str(TOKENS), PROMPT, "--json"] + (["--mmap"] if mode == "map" else [])
    out = subprocess.run(args, capture_output=True, text=True, check=True).stdout
    return {"mode": mode, "cache": cache, "cached_at_start_bytes": cached, **json.loads(out)}


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--model", type=Path, default=ROOT / "models/gemma-4-E2B-it-Q4_0.gguf")
    p.add_argument("--rounds", type=int, default=3)
    p.add_argument("--warmup", type=int, default=1)
    p.add_argument("--allow-battery", action="store_true")
    p.add_argument("--allow-boost", action="store_true")
    p.add_argument("--summarize", type=Path, metavar="ROWS.jsonl")
    a = p.parse_args()
    if a.summarize:
        rows = [json.loads(l) for l in a.summarize.read_text().splitlines()]
        sys.stdout.write(summarize([r for r in rows if r["phase"] == "measured"]))
        return
    problems = protocol_problems(conditions(), a.allow_battery, a.allow_boost)
    if problems:
        sys.exit("\n".join(problems))
    if not EXAMPLE.is_file():
        sys.exit("build it first: cargo build --release -p kestrel-inference --features llama --example generate")
    OUT.mkdir(parents=True, exist_ok=True)
    stem = OUT / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    measured = []
    with open(stem.with_suffix(".jsonl"), "w") as f:
        for r in range(-a.warmup, a.rounds):
            phase = "warmup" if r < 0 else "measured"
            for mode, cache in order(r):
                row = {"round": r, "phase": phase, "model": a.model.name, **conditions(),
                       **run(a.model, mode, cache)}
                f.write(json.dumps(row, sort_keys=True) + "\n")
                f.flush()
                print(f"round {r} {mode}/{cache}: load {row['load_ms']:.0f} ms, "
                      f"anon {row['anon_kib'] >> 10} MiB, file {row['file_kib'] >> 10} MiB", file=sys.stderr)
                if phase == "measured":
                    measured.append(row)
    stem.with_suffix(".md").write_text(summarize(measured))
    sys.stdout.write(summarize(measured))


if __name__ == "__main__":
    main()
