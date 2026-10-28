import os
import shutil
import tempfile
from pathlib import Path

import pytest

from bench_mmap import CONDITIONS, cached_bytes, evict, order, summarize, warm

# A real disk: /tmp is tmpfs, whose pages are the file itself and never leave the cache.
DISK = Path(__file__).resolve().parents[2] / "target"


def test_every_condition_runs_once_in_every_position_over_four_rounds():
    assert order(1) == [("map", "cold"), ("copy", "warm"), ("map", "warm"), ("copy", "cold")]
    for pos in range(4):
        assert sorted(order(r)[pos] for r in range(4)) == sorted(CONDITIONS)


@pytest.mark.skipif(shutil.which("fincore") is None, reason="needs fincore (util-linux)")
def test_evict_and_warm_move_a_file_out_of_and_into_the_page_cache():
    DISK.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=DISK) as d:
        f = Path(d) / "weights.bin"
        f.write_bytes(os.urandom(4 << 20))
        os.sync()  # dirty pages cannot be dropped
        warm(f)
        assert cached_bytes(f) == 4 << 20
        evict(f)
        assert cached_bytes(f) == 0


def row(load, mode, cache, ttft, tps, anon, file, text="Red, yellow and blue."):
    return {"mode": mode, "cache": cache, "load_ms": load, "session_ms": 10.0,
            "ttft_first_ms": ttft, "decode_tok_per_s": tps, "peak_kib": 4 << 20,
            "anon_kib": anon << 10, "file_kib": file << 10, "text": text}


def test_summary_gives_medians_per_condition_and_confirms_identical_outputs():
    rows = [row(900.0, "copy", "cold", 300.0, 12.0, 3000, 5),
            row(1100.0, "copy", "cold", 320.0, 12.4, 3000, 5),
            row(1000.0, "copy", "cold", 310.0, 12.2, 3000, 5),
            row(200.0, "map", "warm", 280.0, 12.6, 1200, 1500)]
    assert summarize(rows) == (
        "| load | cache | n | load ms | first token ms | decode tok/s | anon MiB | file MiB |\n"
        "|---|---|---|---|---|---|---|---|\n"
        "| copy | cold | 3 | 1000 | 310 | 12.2 | 3000 | 5 |\n"
        "| map | warm | 1 | 200 | 280 | 12.6 | 1200 | 1500 |\n"
        "\n"
        "Outputs: 4 runs, 1 distinct text (byte-identical).\n"
    )


def test_summary_says_when_outputs_differ():
    rows = [row(1.0, "copy", "warm", 1.0, 1.0, 1, 1, text="a"),
            row(1.0, "map", "warm", 1.0, 1.0, 1, 1, text="b")]
    assert summarize(rows).endswith("Outputs: 2 runs, 2 distinct texts (NOT identical).\n")
