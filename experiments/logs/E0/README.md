# E0 sessions

| Session | Date (UTC) | Status | Protocol |
|---|---|---|---|
| `20261010T194613Z` | 2026-10-10 19:46 | **Pilot.** Don't pool it with later sessions. | 3 rounds, no warm-up round; no CPU-frequency or background-load logging (added afterwards) |

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
