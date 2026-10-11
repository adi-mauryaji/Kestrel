# E0 sessions

Protocol v3 (ADR-011) for the 3 repeatability sessions: 2 warm-up + 3 measured rounds, threads pinned one per physical core (`cpu_mask` 555 here), user's Docker containers stopped.

| Session | Date (UTC) | Status | Protocol |
|---|---|---|---|
| `20261010T194613Z` | 2026-10-10 19:46 | **Pilot.** Don't pool it with later sessions. | 3 rounds, no warm-up round; no CPU-frequency or background-load logging (added afterwards) |
| `20261011T073450Z` | 2026-10-11 07:34 | **Unpinned reference.** Not pooled: the series restarted pinned (ADR-011). | 1 warm-up + 3 measured rounds; frequency + background logging; Docker containers kcc-kafka/kcc-mysql stopped |
| `20261011T081903Z` | 2026-10-11 08:19 | **Not a repeatability session**: AC power was lost during measured round 2 (rows record `ac_online` False from there). Warm-up rounds 0–1 are the pinned on-AC reference. | protocol v3 (pinned `555`, 2 warm-up + 3 measured rounds); containers stopped; started 12 min after session 1 ended |

## Pilot findings (20261010T194613Z)

Conditions: AC power, tuned `throughput-performance`, governor `performance` on all 12 threads, boost on, 6 threads, warm page cache. Energy was **not** measured.

1. **Two power regimes.** Round 0 prefill tok/s was 26–39% higher than rounds 1–2, and rounds 1 and 2 agree with each other. Decode tok/s changed by ≤3%. Round 0 peaked at 84–93 °C; rounds 1–2 ended around 55–58 °C.
   - Likely cause: a short boost window followed by a lower sustained power limit. This is **not confirmed**, because frequency wasn't logged.
   - Fix: later sessions run one recorded `warmup` round and log CPU frequency.
2. **Sustained-regime numbers** (mean of rounds 1–2; one session, so no CV yet):

| Model | File GB | Prefill tok/s | Decode tok/s | 128-tok prefill (s) | 64-tok decode (s) | Peak RSS GiB | Startup (s) |
|---|---|---|---|---|---|---|---|
| Qwen3-1.7B Q4_K_M | 1.28 | 92.1 | 18.1 | 1.39 | 3.5 | 1.91 | 1.0 |
| Qwen3-1.7B Q8_0 | 2.17 | 68.8 | 11.4 | 1.86 | 5.6 | 2.11 | 0.6 |
| Gemma-4-E2B Q4_0 | 2.84 | 58.3 | 12.3 | 2.20 | 5.2 | 3.74 | 1.9 |
| Gemma-4-E2B Q8_0 | 4.97 | 47.1 | 8.0 | 2.72 | 8.0 | 4.75 | 1.4 |
| Qwen3-4B Q4_K_M | 2.50 | 31.8 | 8.1 | 4.02 | 7.9 | 4.06 | 2.1 |
| Qwen3-4B Q8_0 | 4.28 | 24.8 | 4.9 | 5.17 | 13.0 | 4.09 | 0.9 |
| bge-small-en-v1.5 Q8_0 | 0.04 | 3556 (32-tok pass ≈ 9.0 ms) | — | — | — | 0.06 | ≈0 |
| multilingual-e5-small F16 | 0.24 | 3107 (≈ 10.3 ms) | — | — | — | 0.38 | 0.8 |
| multilingual-e5-small Q8_0 | 0.13 | 3574 (≈ 9.0 ms) | — | — | — | 0.28 | 0.7 |

3. **Q4 files did not save RAM as expected.** Peak RSS for the Q4 files is well above file size (Qwen3-1.7B Q4: 1.91 GiB RSS for a 1.28 GB file). Q8 RSS is about equal to file size. Q4 and Q8 Qwen3-4B differ by only 0.03 GiB of RSS.
   - Hypothesis to verify: llama.cpp's CPU weight repacking keeps a second, repacked copy for Q4 formats.
   - Matters for H6, which assumes Q4 lowers RAM.
4. **Scale gap.** An encoder pass takes about 10 ms. The cheapest SLM needs about 1.4 s just to prefill a 128-token prompt, before it generates anything.
   - That's an observation about this machine, not a test of H1.
5. Startup times were measured with a warm page cache and are not cold-start numbers.

## Session 1 (20261011T073450Z)

Conditions: AC, battery 100%, tuned `throughput-performance` (governor `performance`), boost on, 6 threads (unpinned), warm page cache, start temperature 42 °C, user's Docker containers stopped. Background CPU busy % during cooldowns: mean 1.0, max 5.5 (n = 96). Energy was **not** measured. Duration 33 min.

Measured rounds 1–3 (mean, CV across the 3 rounds):

| Model | Prefill tok/s (CV) | Decode tok/s (CV) | Peak RSS MiB | Startup (s) | vs pilot: prefill / decode |
|---|---|---|---|---|---|
| Qwen3-1.7B Q4_K_M | 118.3 (0.4%) | 18.7 (0.0%) | 1956 | 1.00 | +28% / +3% |
| Qwen3-1.7B Q8_0 | 97.8 (5.6%) | 11.6 (0.2%) | 2159 | 0.65 | +42% / +2% |
| Gemma-4-E2B Q4_0 | 80.6 (4.8%) | 12.8 (0.2%) | 3839 | 1.82 | +38% / +4% |
| Gemma-4-E2B Q8_0 | 68.5 (3.9%) | 8.3 (0.1%) | 4864 | 1.44 | +45% / +3% |
| Qwen3-4B Q4_K_M | 46.6 (1.0%) | 8.4 (0.2%) | 4162 | 2.06 | +46% / +3% |
| Qwen3-4B Q8_0 | 36.6 (0.5%) | 5.0 (0.2%) | 4192 | 0.86 | +48% / +2% |
| bge-small-en-v1.5 Q8_0 | 4449 (12.3%) | — | — | — | +25% |
| multilingual-e5-small F16 | 4140 (2.2%) | — | — | — | +33% |
| multilingual-e5-small Q8_0 | 3862 (24.3%) | — | — | — | +8% |

Findings:

1. **Decode is stable; prefill is not.** Within the session, decode varied ≤ 0.2% across rounds, while prefill varied up to 5.6%. Against the pilot's sustained rounds, decode is 2–4% higher but prefill is **25–48% higher**. Prefill is compute-bound and follows the CPU's power limit; decode is memory-bandwidth-bound. Today the measured rounds ran hotter (up to 89 °C) at 2.5–2.9 GHz; the pilot's sustained rounds ended at ~55–58 °C. One explanation is a lower sustained power limit during the pilot (e.g. thermal/power history after the long build session); it can't be confirmed (no frequency log then, no `platform_profile` on this laptop, power limits need root). **The 3-session CV will show whether prefill is repeatable at all; plan to report it per session.**
2. **The warm-up round doesn't absorb all boost.** Measured round 1 ran at 2710 MHz vs ~2600 for rounds 2–3 and is the fast outlier where prefill CV is high (Qwen3-1.7B Q8: 104 vs 94.6; Gemma Q4: 85 vs 78.5).
3. **Encoder latency is bimodal with unpinned threads.** e5-small Q8 alternated between ~6.5 and ~9.6 ms per 32-token pass. Logical CPUs 2k and 2k+1 are SMT siblings here. Exploratory check right after the session (8 alternating trials each, not part of the CSV): unpinned median 6.60 ms, CV 19.6%, outliers at 8–10 ms; pinned one thread per physical core (`-C 555 --cpu-strict 1`) median 6.37 ms, CV 4.4%, no outliers. Supports the "two threads on one core" explanation; small n.
4. **RSS repeats exactly** (same values as the pilot, ±1 MiB). Q4 RSS ≫ file size still holds (H6 check pending).

## Pinned session (20261011T081903Z)

Start temperature 41 °C, 12 minutes after the unpinned session ended; background busy % mean ~1.

**Correction (2026-10-11, same day).** The first version of this section blamed the slowdown on a load-history power regime. The rows show `ac_online` turned False at that exact point: the laptop was **on battery** from measured round 2 (`qwen3-4b-q8_0`, ~25 min in) to the end. The script recorded it but kept running; it now stops instead (see below).

1. **On battery, everything slows by 20–35%, decode included.** From the AC loss on, every process ran at ~1.95–2.0 GHz with temperatures of ~56–59 °C (Qwen3-1.7B Q4: 18.7 → 14.3 tok/s; encoder passes 6.3–7.6 → 10.1–12.0 ms). These rows are battery data and are excluded.
2. **Pinning is not what slowed this session.** On AC, pinned SLM numbers equal the unpinned ones (Qwen3-1.7B Q4: 118.3 / 18.71 vs 118.3 / 18.68 tok/s). Pinned encoder latency on AC was 6.27–7.61 ms with no bimodality (unpinned: 6.5 vs 9.6 ms modes), which supports ADR-011.
3. **The pilot's lower prefill is still unexplained.** The pilot was on AC throughout; its sustained rounds had 25–48% lower prefill than session 1 but decode only 2–4% lower, ending at ~55–58 °C. That pattern differs from the battery one (decode −23%), so it is not explained by this session.

`experiments/analysis/e0_summary.py <csv> [<csv> ...]` prints per-session means/CVs and, for 2+ sessions, the CV of session means.
