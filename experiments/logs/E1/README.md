# E1 logs

Written by `experiments/e1/run_e1.py`, one directory per chunk (ADR-020, PREREG §4). Each holds `rows.jsonl`
(one row per item × system × repetition, with each consulted tier's answer and time), `rows.meta.json` (systems,
load time, peak RSS from `kestrel bench-items`), `samples.jsonl` (10-s machine samples), `stderr.txt` and
`manifest.json` (environment, hashes, checks, problems, `valid`).

No test-split run exists yet: the harness refuses the test split until the PREREG is frozen (tag `exp-E1-frozen`).

## Dev dry runs (2026-10-22, before the freeze)

Purpose: check that the harness and `bench-items` decide exactly what the simulation of each config decides, for all
three systems, before any test run. Run on **battery, boost 1, governor powersave**, with `--allow battery boost
governor` and a 30-s cooldown, so `valid` is false by design and the timings are dev timings, not E1 results. The
laptop had just rebooted and its wall clock was not synchronized (NTP inactive), so the UTC stamps in the directory
names may be off; durations use a monotonic clock.

| Run | Items (latency subset) | Rows | R0 / R1 / R3 vs simulation | Repetitions disagreeing | Problems |
|---|---|---|---|---|---|
| `clinc150_val_n30` | 30 (6 × 5 reps) | 177 | 30/30 · 30/30 · 30/30 (confidence gap ≤ 1.1e-6) | 0 | none |
| `intents_v0_val_n20` | 20 (20 × 5 reps) | 315 | 20/20 · 20/20 · 20/20 (≤ 4.4e-7) | 0 | rule 2: background busy 13.8% over 1 min outside the mask (peak 54%) |

Rows = 3 systems × (5 warm-ups + the items' repetitions). Rule 2 firing on the second run shows the background check
works: something else ran on the SMT siblings (probably post-boot services or the IDE). Real chunks need an idle
laptop (IDE and browser closed).

**Cascade overhead** (Week 8 risk "cascade overhead eats the gain"): row wall time minus the sum of the consulted
tiers' times, i.e. normalization plus routing. Median ≈ 5–7 µs, p95 ≤ 12 µs, worst 106 µs (Intents-v0 R3), which is
≈ 2–3 × 10⁻⁵ of R3's mean time. The overhead is negligible.

| Dev timings (battery, boost 1; not E1) | R0 mean / median | R1 mean | R3 mean / median |
|---|---|---|---|
| CLINC150 (54 timed requests per system) | 941 / 994 ms | 5.4 ms | 243 / 5.5 ms |
| Intents-v0 (100 per system) | 763 / 763 ms | 5.4 ms | 420 / 577 ms |

Model load + Tier-4 prefix priming: 131 s for CLINC150 (150 shots + descriptions, cold page cache after the reboot),
27 s for Intents-v0. Peak RSS with every model loaded: 3780 / 3641 MiB (VmHWM, no mmap).

## Analysis pipeline, checked on the dry runs (2026-10-23, before the freeze)

`experiments/analysis/analyze_e1.py` (statistics from `kestrel_stats.py`) writes `analysis_<run>.json` next to the
runs: Table R1 (R0 tuned and untuned, R1, R3), H1 (intersection–union of the two 95% paired-bootstrap CIs), H1c,
per-language H7a and D11, ρ, the signal's false-positive rate and, on CLINC150 test, the D1 sensitivity analysis. It
applies rule 3 across chunks and refuses a chunk with zero or two usable runs. `analysis_intents_v0_val_n20.json` and
`analysis_clinc150_val_n30.json` come from the two dry runs above with `--dev`: they check the plumbing (the scoring
agrees with `policies.correct` on all 150 decisions) and are **not E1 results** (dry runs, 20 and 30 val items).
