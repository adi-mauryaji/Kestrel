# v3 step (a): Gemma-4-E2B Q4_0 loaded with and without mmap (ADR-036)

Session `20261026T140055Z` (`.jsonl` = one row per run, `.md` = the summary). Engineering measurement for the
v3 service, not a PREREG experiment: research loads keep D12 (no mmap).

**Method.** `experiments/measure/bench_mmap.py` runs Kestrel's own adapter (`generate` example: load, one
session, two greedy generations of "The three primary colours are", memory read at the end) as one process per
run. Conditions: copy (no mmap) or map, page cache cold (file evicted with `posix_fadvise DONTNEED`) or warm (file
read first); `fincore` records what was cached at the start of every run. One warm-up round, then 3 measured
rounds, the four conditions rotated each round. Medians over n = 3.

**Conditions** (in every row): AC power, boost **on** (ADR-012 wants it off: setting it needs sudo), governor
`powersave` under tuned `balanced`, CPU 56–90 °C, load1 1.2–2.6, Kestrel's service idle. Memory does not depend
on these (E0's H6 check); timings are compared only within this session.

| load | cache | n | load ms | first token ms | decode tok/s | anon MiB | file MiB |
|---|---|---|---|---|---|---|---|
| copy | cold | 3 | 3656 | 111 | 12.0 | 3247 | 8 |
| map | cold | 3 | 2477 | 111 | 12.0 | 1144 | 2702 |
| copy | warm | 3 | 2596 | 103 | 12.0 | 3247 | 8 |
| map | warm | 3 | 1383 | 117 | 12.0 | 1144 | 2702 |

Outputs: 12 measured runs (and the 4 warm-up runs), 1 distinct text: byte-identical.

**Findings**
1. **mmap cuts the memory that cannot be reclaimed from 3247 to 1144 MiB (−2103 MiB, −65%).** What stays anonymous
   matches E0's H6 check: Q4 tensors repacked for AVX2 (CPU_REPACK) plus the KV cache and compute buffers.
2. **The file-backed part is the whole file** (2702 of 2710 MiB). llama.cpp maps with `MAP_POPULATE`
   (`src/llama-mmap.cpp:480`) and reads every tensor at load. These pages are clean, so the kernel can drop them
   under pressure. `VmHWM` counts them anyway: peak 3.5–3.8 GiB mapped vs 3.2 GiB copied, as E0's H6 check found.
3. **Loading is faster with mmap:** cold 3.66 → 2.48 s, warm 2.60 → 1.38 s.
4. **No cost found:** first token 103–117 ms in every condition, and the bytes generated never changed.
5. **The per-layer embeddings (1.35 GB, 48% of the file) are not left on disk.** gemma4 marks
   `per_layer_token_embd` for lazy reading (`src/models/gemma4.cpp:58`), but the default mode (AUTO) reads lazily
   only tensors larger than 4 GiB (`src/llama-model-loader.cpp:1094`). Mode ON has no setter in `llama-cpp-2
   0.1.157` (its params field is `pub(crate)`), so using it would need a patched or newer crate.

**Not measured here**
- Decode speed: Gemma ended this raw completion after 7 tokens, so its tok/s rests on 6 tokens. E0's 12.8 tok/s
  stands.
- The service's first Gemma answer (~20 s in daily use, README). Load is only 2.5–3.7 s of it. The likely remainder:
  Tier 4 primes its few-shot prompt prefix into the KV cache when it starts (`kestrel-inference/src/tier.rs:59`).
  This is the next measurement.
- One warm-up run (map, warm) started with 1900 of 2710 MiB cached; warm-up rows are not summarized.

**Reproduce**
```
cargo build --release -p kestrel-inference --features llama --example generate
.venv/bin/python experiments/measure/bench_mmap.py --allow-boost          # new session, ~75 s
.venv/bin/python experiments/measure/bench_mmap.py --summarize experiments/logs/v3/mmap/20261026T140055Z.jsonl \
  | cmp - experiments/logs/v3/mmap/20261026T140055Z.md                     # the summary, byte for byte
```
