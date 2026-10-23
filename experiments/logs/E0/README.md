# E0 sessions

Final protocol for the 3 repeatability sessions (ADR-011 + ADR-012): 2 warm-up + 3 measured rounds, threads pinned one per physical core (`cpu_mask` 555 here), **turbo boost off**, AC with a full battery, user's Docker containers stopped. `bench_e0.py` refuses to start, and stops mid-session, on battery or with boost on.

| Session | Date (UTC) | Status | Protocol |
|---|---|---|---|
| `20261010T194613Z` | 2026-10-10 19:46 | **Pilot.** Don't pool it with later sessions. | 3 rounds, no warm-up round; no CPU-frequency or background-load logging (added afterwards) |
| `20261011T073450Z` | 2026-10-11 07:34 | **Unpinned reference.** Not pooled: the series restarted pinned (ADR-011). | 1 warm-up + 3 measured rounds; frequency + background logging; Docker containers kcc-kafka/kcc-mysql stopped |
| `20261011T081903Z` | 2026-10-11 08:19 | **Not a repeatability session**: AC power was lost during measured round 2 (rows record `ac_online` False from there). Warm-up rounds 0–1 are the pinned on-AC reference. | protocol v3 (pinned `555`, 2 warm-up + 3 measured rounds); containers stopped; started 12 min after session 1 ended |
| `20261011T101743Z` | 2026-10-11 10:17 | **Session 1 of 3** (final protocol). | v3 + ADR-012: pinned `555`, boost off, 2 warm-up + 3 measured rounds, AC with battery 99–100%, containers stopped, start 39 °C |

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
   - **Confirmed 2026-10-17**, see "H6 check" at the end of this file. It is the mmap'd original plus the repacked copy.
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

## Session 1 of 3 (20261011T101743Z): boost off, pinned

44 min; every row on AC, boost 0, governor `performance`, mask `555`; background busy % mean 1.1, max 6.0.

| Model | Prefill tok/s (CV) | Decode tok/s (CV) | Peak RSS MiB | Startup (s) | vs boost-on session 1: prefill / decode |
|---|---|---|---|---|---|
| Qwen3-1.7B Q4_K_M | 90.6 (0.2%) | 18.23 (0.1%) | 1955 | 1.88 | −23% / −2% |
| Qwen3-1.7B Q8_0 | 73.3 (0.0%) | 11.46 (0.1%) | 2159 | 1.03 | −25% / −1% |
| Gemma-4-E2B Q4_0 | 58.5 (1.1%) | 12.18 (1.6%) | 3839 | 3.12 | −27% / −5% |
| Gemma-4-E2B Q8_0 | 52.4 (0.2%) | 8.04 (0.1%) | 4868 | 2.29 | −24% / −3% |
| Qwen3-4B Q4_K_M | 36.3 (0.3%) | 8.24 (0.3%) | 4162 | 3.63 | −22% / −2% |
| Qwen3-4B Q8_0 | 28.7 (1.2%) | 4.96 (0.3%) | 4192 | 1.37 | −22% / −1% |

Encoders (median per 32-token pass, all 5 rounds): bge-small Q8 10.5 ms (one 13.5 ms outlier in measured round 2), e5-small F16 12.4–12.5 ms, e5-small Q8 10.5–10.6 ms.

1. **No regime drift.** Frequency, temperatures (50–60 °C) and throughput are flat from warm-up round 0 to the end; the warm-up rounds are indistinguishable from the measured ones. Within-session CV ≤ 1.6% for every SLM test.
2. **Boost off costs ~22–27% prefill and ~1–5% decode** against the boost-on session; encoder passes ~10.5 vs ~6.3 ms (pinned, boost on). Reported latencies are therefore conservative (ADR-012).
3. **The pilot's sustained prefill matches these no-boost numbers** for some models (Qwen3-1.7B Q4 92.1 vs 90.6; Gemma Q4 58.3 vs 58.5), though it is 12–14% lower for others. The pilot probably ran near no-boost clocks for a reason we can't see.
4. **Caveat on `freq_*_mhz`:** it averages all 12 logical CPUs. With pinning, 6 of them are idle siblings, so ~1.75 GHz understates the clock of the active cores. It still detects changes (it showed the battery drop). From session 2 on, `freq_pinned_mean_mhz` / `freq_pinned_min_mhz` also average only the pinned CPUs (smoke check: 1870 vs 1775 MHz all-CPU).

## H6 check: Q4 RSS and CPU weight repacking (2026-10-17, not an E0 session)

One-off check of pilot finding 3. Qwen3-1.7B, `llama-bench -p 16 -n 4 -r 1 --no-warmup -t 6 -v`, peak RSS from
`/usr/bin/time -f %M`, boost on, unpinned, laptop otherwise as usual. RSS is deterministic here (E0 repeats it to ±1 MiB),
so these conditions don't matter for it; no latency was taken from these runs. Logs were not kept (scratch).

| File | Load mode | Weight buffers (from `-v`) | Repacked tensors | Peak RSS MiB |
|---|---|---|---|---|
| Q4_K_M (1223 MiB) | auto = mmap (E0's mode) | CPU_Mapped 1210.6 + CPU_REPACK 645.8 | 168 (q4_K → q4_K_8x8) | **1938** |
| Q4_K_M | `--load-mode none` | CPU 571.6 + CPU_REPACK 645.8 | 168 | **1311** |
| Q8_0 (2065 MiB) | auto = mmap | CPU_Mapped 2059.1 | 0 | 2147 |
| Q8_0 | `--load-mode none` | CPU 2059.1 | 0 | 2148 |

1. **Hypothesis confirmed.** On this CPU (AVX2), llama.cpp repacks the q4_K tensors into an extra anonymous buffer
   (646 MiB). With mmap, it reads the original pages to do so, and they stay mapped, so `VmHWM` counts those weights twice.
   Without mmap, the repacked tensors are not loaded a second time and Q4 RSS ≈ file size + ~90 MiB of
   context/compute buffers. Q8_0 is not repacked here, so its load mode makes no difference.
2. **Q4's RAM saving depends on the load mode:** vs Q8, −209 MiB (−10%) with mmap, −837 MiB (−39%) without.
3. The doubled pages are clean and file-backed, so the kernel can drop them under memory pressure. `VmHWM` still counts
   them, so with mmap it overstates what Q4 really needs.
4. `-ot '.*=CPU'` did not turn repacking off. It moved only the embedding and output tensors; the layers were still repacked.
5. Consequence for the PREREG (proposed D12): fix the Tier-4 load mode before E1/E6, or report `RssAnon` and `RssFile`
   separately. The E0 RSS table stays valid for mmap loading and must be labelled that way.

## Session 2 of 3 (20261017T132649Z): evening, boost off, pinned

2026-10-17, 18:56–19:41 IST (session 1 ran at 15:47 IST). Protocol v3 + ADR-012: AC, battery Full (100%), tuned
`throughput-performance`, governor `performance`, boost 0, mask `555`, Docker containers stopped, CPU 45 °C at start.
Nothing else ran during the session.

Invalidation rules (PREREG §4): AC-off/boost-on rows 0 of 120. Background busy mean 1.18%, max 7.6% (limit 10%). Pinned
CPU frequency during SLM tests 1985–2052 MHz, within ±2% of the session median (limit 10%). Max temperature 63.1 °C.
**Valid.**

| Model | Prefill tok/s (CV) | Decode tok/s (CV) | Peak RSS MiB | vs session 1: prefill / decode |
|---|---|---|---|---|
| Qwen3-1.7B Q4_K_M | 90.98 (0.2%) | 18.29 (0.8%) | 1956 | +0.4% / +0.3% |
| Qwen3-1.7B Q8_0 | 73.20 (0.3%) | 11.49 (0.8%) | 2159 | −0.1% / +0.3% |
| Gemma-4-E2B Q4_0 | 58.69 (0.7%) | 12.01 (2.0%) | 3838 | +0.4% / −1.4% |
| Gemma-4-E2B Q8_0 | 52.05 (0.5%) | 8.00 (0.3%) | 4853 | −0.7% / −0.5% |
| Qwen3-4B Q4_K_M | 36.20 (0.3%) | 8.22 (0.1%) | 4140 | −0.2% / −0.2% |
| Qwen3-4B Q8_0 | 28.62 (0.6%) | 4.94 (0.5%) | 4192 | −0.3% / −0.4% |

Across sessions 1–2 (CV of the session means, `experiments/analysis/e0_summary.py`):
1. **SLM throughput is repeatable:** CV ≤ 1.0% for every pp128 and tg64 test.
2. **e5 encoder passes are repeatable:** CV 0.2% (Q8_0) and 0.7% (F16). bge-small pp32 is not (9.4% across sessions,
   13–20% within): a 0.2 s run is too short for llama-bench, and bge was not chosen (ADR-014).
3. **Startup is noisier:** CV 1–13% across sessions, highest for Q8 files (Qwen3-4B Q8 12.8%), i.e. disk/page-cache
   effects. E1 loads models once, so startup is reported but not part of per-request latency.
4. Peak RSS repeats within 1% (mmap loading as in session 1; E1/E6 load without mmap, D12).
5. Frequency: session 2 logs the pinned-CPU mean (~2020 MHz). Session 1's column averages all 12 CPUs (~1760 MHz), so
   the two are not directly comparable (see session 1, note 4).

Session 3 (morning) is still due for the 3-session check (Week 2 "T").

## Session 3 of 3 (20261023T163212Z): night, boost off, pinned

2026-10-23, 21:57–22:47 IST, after a 5-min idle cooldown and CPU ≤ 50 °C (the session followed a 1.3-h full-CPU
run). Protocol v3 + ADR-012: AC, battery 100%, tuned `throughput-performance`, governor `performance`, boost 0, mask
`555`, no containers. Invalidation rules (PREREG §4): AC-off/boost-on rows 0 of 120; background busy mean 1.14%, max
5.7%; pinned CPU frequency in SLM tests 2003–2057 MHz, worst deviation from the session median 0.95%; max temperature
63.6 °C. **Valid.**

## Repeatability across sessions 1–3 (Week 2 "T")

`python experiments/analysis/e0_summary.py` on the three final-protocol sessions (20261011T101743Z, 20261017T132649Z,
20261023T163212Z; afternoon, evening, night): the CV of the session means is **≤ 1.0% for every SLM's prefill and
decode** (Gemma-4-E2B Q4_0 pp128 0.8%, tg64 1.0%; the others 0.1–0.5%) and **0.2% for the chosen encoder**
(multilingual-e5-small Q8_0 pp32). Higher only where the quantity is short or unused: startup times 0.7–9.0% (1–3.6 s
processes), bge-small-en-v1.5 pp32 9.8% (not selected, ADR-014). Throughput on this machine under the protocol is
repeatable well within the precision the latency results need.
