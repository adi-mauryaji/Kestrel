# Kestrel pre-registration (PREREG) — draft v0

Status: **DRAFT, not frozen** (2026-10-11). Written by Claude from the blueprint (Ch. 34–38), the Week-1/2 data and the
human's decisions so far (ADR-005, ADR-009–ADR-012). Items marked **[decide]** need the human. Freezing = commit + git tag
**before any test set is evaluated** (Ch. 34.6). After freezing, any change is a new, logged version and the old one stays.

## 1. Questions, hypotheses, experiments

| RQ | Hypothesis | Experiment | Systems | Role |
|---|---|---|---|---|
| RQ1 | **H1**: at a validation-chosen operating point the calibrated cascade is non-inferior in accuracy (margin δ) **and** faster than always-SLM | E1 | R3 vs R0 (R4 if built) | **primary** |
| RQ2 | H2: calibrated confidences give lower selective risk at equal coverage than raw softmax. A replication in Kestrel's setting (C3, CAUC) | E2 | R3 vs R2 (with / without temperature) | secondary |
| RQ3 | H3: adding Tier 3 to {1, 4} is a Pareto improvement; Tier 2 adds nothing beyond noise | E3 | tier sets {4}, {1,4}, {3,4}, {1,3,4} (+{1,2,3,4} only if Tier 2 is built) | secondary |
| RQ6 | H6: the Q4-class quantization stays within δ₃ of Q8 while using less RAM and running faster | E6 | each shortlisted SLM × {Q4, Q8}; baseline = Q8 of the same model | secondary |
| RQ7 | H7a: the calibrated cascade stays within δ of always-SLM **on Hinglish**; H7b: with one shared temperature, ECE is higher on Hinglish than on English | E1/E2 stratified by language | R3, R0, R2 | secondary |
| RQ4, RQ5 | as blueprint (personalization, forgetting) | E4, E5 | B6–B8 | secondary, first to drop (Ch. 52.1) |

Systems (Ch. 13, 35): **R0** always-generative (B3), **R1** single encoder + τ (B2), **R2** fixed cascade, raw softmax, one τ
(B4), **R3** calibrated cascade, per-tier temperature + per-tier τ tuned on validation (B5), **R4** adaptive (only if built).
B1 (rules only) is reported with coverage and precision. R0 and Tier 4 inside the cascade use the **same** prompt template,
model, quantization, grammar and thread count.

## 2. Data (frozen inputs)

| Dataset | Sets used | Fingerprint / source | Notes |
|---|---|---|---|
| Kestrel-Intents-v0 | train 604 / cal 100 / val 128 / **test 252** | full set `5de96809…e37ca314`; per-split in `datasets/kestrel_intents_v0/splits/manifest.json` | split by template family (EN + Hinglish together); OOS split by domain (test domains music, sports unseen); compound items in val/test only. Synthetic, Claude-drafted, human-reviewed — label as synthetic in every table |
| CLINC150 (full) | train 15000 / cal 1500 / val 1700 (200 OOS) / **test 5500 (1000 OOS)** | raw SHA-256 `36923c37…`; per-split in `datasets/clinc150/manifest.json` | official test intact; 24 texts occur in two sets (4 with conflicting labels; 14 test items) |
| Kestrel-Tasks-v0, -Personas-v0 | — | not built yet | added to this file before E4/E6 run |

**[decide] D1:** CLINC150 test keeps its 14 items whose text also appears in train/cal/val (primary, comparable with the
literature) and a sensitivity analysis drops them. **[decide] D2:** the 100 official *train* OOS items join val (threshold
tuning) instead of being unused.

## 3. Scoring rules and metrics

- **Per-item correctness** (`experiments/analysis/metrics.py`): an in-scope item is correct iff the prediction equals the gold
  label; **abstention is wrong** (Ch. 35 fairness rule 2). An **out-of-scope item** (CLINC150 `oos`; Kestrel-Intents
  `unsupported`) is correct iff the system abstains **or** predicts the OOS label (Ch. 8.3). **[decide] D3:** Ch. 35 says
  abstentions are errors without an OOS exception; read literally, no cascade could ever be right on CLINC150's OOS items,
  which have no head class (Ch. 32). This draft resolves it with the OOS exception above.
- **Compound items** (Kestrel-Intents, val/test only): correct iff not accepted as a single intent below Tier 4 (ADR-010).
  Whether Tier 4's plan covers the intents is evaluated in Kestrel-Tasks-v0, not here.
- **Accuracy, macro-F1** (mean over gold classes; abstention counts as predicting the OOS label), **coverage**, **selective
  risk** at coverage **0.80** **[decide] D4**, **AURC**, **ECE** with **15 equal-width bins, right-closed** `((m−1)/M, m/M]`
  **[decide] D5**, reliability diagrams. Accuracy-on-answered is reported next to end-to-end accuracy.
- **Latency:** end-to-end wall time per request on a monotonic clock (`kestrel-bench::measure`); per item, the median over
  ≥ 5 timed repetitions after 5 warm-up requests is the unit; mean, median and p95 (nearest rank) over items.
- **Peak RSS:** `VmHWM` per process at the end of a run, MiB. **Energy: not measured** — the only counter (RAPL) is root-only on
  this machine; tables use the Ch. 37.4 template sentence.

## 4. Measurement protocol (latency experiments: E0, E1, E3, E6)

Machine: Ryzen 5 5500U (6C/12T), 14.95 GiB RAM, Fedora (kernel 7.2.6), Python 3.14.7, rustc 1.99.0, llama.cpp 26394b4e
(`llama-cpp-2 =0.1.157`). Per session: AC power with the battery full; tuned `throughput-performance` (governor
`performance`); **turbo boost off** (ADR-012); threads **pinned one per physical core** (ADR-011; mask `555` here); 6 threads;
the user's Docker containers stopped; ≥ 5 min cooldown and CPU ≤ 50 °C before starting; warm page cache (stated). Systems are
interleaved (A, B, A, B, …).

**Invalidation rules (pre-declared; an invalidated run is rerun completely and logged):**
1. AC power lost or boost on at any point (the harness stops the run).
2. Background CPU busy > 10% in any cooldown window.
3. Mean frequency of the pinned CPUs for a process deviates > 10% from that session's median for the same model and test.
4. CPU temperature > 95 °C after any process.
5. A crash or a model-file hash mismatch.

## 5. Targets from E0 (Ch. 6.4)

E0 session 1 under this protocol (20261011T101743Z; sessions 2–3 pending): Qwen3-1.7B Q4 prefill 90.6 tok/s, decode 18.2
tok/s, so a Tier-4 classification with a 128-token prompt and ~8 output tokens costs ≈ 1.4 s + 0.44 s ≈ **1.9 s** before any
load time; an encoder pass over 32 tokens costs ≈ **10.5 ms** (e5-small Q8), ≈ 175× less. These are llama-bench estimates, not
end-to-end measurements.

- **ρ (latency target for cascade-handled requests):** `median_latency ≤ 0.1 × median_latency(R0)` — one order of magnitude,
  which E0 supports with a wide margin. **[decide] D6**; recheck after E0 sessions 2–3 and the first end-to-end runs.
- **β (RAM ceiling):** `peak_RSS(all loaded models) ≤ 0.5 × 14.95 GiB = 7.47 GiB`. Largest shortlisted pair:
  Gemma-4-E2B Q8 (4.75 GiB) + e5-small F16 (0.38 GiB) = 5.1 GiB, so every pair fits. **[decide] D7**
- **δ (non-inferiority margin):** set after the Week-8 pilot (20–30% of dev data, ≥ 3 seeds) as
  `max(2 × seed-to-seed SD of R0 accuracy, smallest effect of interest agreed with the advisor)`. **TBD — must be written here
  before the test sets are touched.** δ₃ (E6) is set the same way.

## 6. Analysis plan

- **Primary (H1)**, an intersection–union test on paired item-level data (Ch. 34.1): (1) the lower bound of the 95%
  paired-bootstrap CI of Δacc = mean(correct_R3 − correct_R0) is > −δ, **and** (2) the upper bound of the CI of
  Δlat = mean(lat_R3 − lat_R0) is < 0. 10 000 bootstrap resamples over items, seed 0. No α-splitting is needed for the
  primary. **[decide] D8:** primary dataset = CLINC150 test (n = 5500); Kestrel-Intents-v0 test (n = 252) enters the secondary
  family, because its CI will be wide.
- **Secondary family** (Holm–Bonferroni at α = 0.05): H2, H3, H6, H7a, H7b (and H4, H5 if run). Paired correctness: exact
  McNemar; latency: Wilcoxon signed-rank on per-item medians; effect sizes with CIs.
- **RQ7:** Kestrel-Intents-v0 test stratified by `lang` (en / hi-en, 126 items each): per-language Δacc and Δlat CIs (H7a);
  per-language ECE with one temperature fitted on the pooled cal set, paired bootstrap of ECE(hi-en) − ECE(en) (H7b). A
  per-language temperature is **exploratory**. The small n per language is a stated limitation.
- **Seeds:** ≥ 5 per trained component (Tier-3 heads); all reported. Thresholds and temperatures are tuned on val / cal only.
- Anything decided after seeing test data is reported as **exploratory**.

## 7. Stopping

Sample sizes are the frozen test sets above. No optional stopping. Every run, including failed and invalidated ones, is
logged in `experiments/logs/` with hashes. Every pre-registered comparison is reported, including those that fail.

## 8. Open decisions (collected)

D1 CLINC150 duplicates · D2 train-OOS into val · D3 OOS abstention rule · D4 coverage 0.80 · D5 ECE 15 right-closed bins ·
D6 ρ = 0.1 · D7 β = 0.5 · D8 primary dataset · δ, δ₃ after the pilot · contribution framing given the prior work in
`docs/research_questions.md` (the calibrated cascade itself is not new; the Hinglish evaluation is the most distinctive part).
