# Kestrel pre-registration (PREREG) — draft v0

Status: **DRAFT, not frozen** (2026-10-11). Written by Claude from the blueprint (Ch. 34–38), the Week-1/2 data and the
human's decisions so far (ADR-005, ADR-009–ADR-012). Items marked **[decide]** need the human; **[accepted 2026-10-17]** marks the human's acceptance of Claude's recommendation ("continue what you recommended"). Freezing = commit + git tag
**before any test set is evaluated** (Ch. 34.6). After freezing, any change is a new, logged version and the old one stays.

## 1. Questions, hypotheses, experiments

| RQ | Hypothesis | Experiment | Systems | Role |
|---|---|---|---|---|
| RQ1 | **H1**: at a validation-chosen operating point the calibrated cascade is non-inferior in accuracy (margin δ) **and** faster than always-SLM | E1 | R3 vs R0 (R4 if built) | **primary** |
| RQ2 | H2: calibrated confidences give lower selective risk at equal coverage than raw softmax. A replication in Kestrel's setting (C3, CAUC) | E2 | **R2c vs R2** (temperature on / off, same shared-τ policy; D9). R3 vs R2c (per-tier vs shared τ) is exploratory | secondary |
| RQ1 | **H1b** (D10): at matched expected cost, R3 is more accurate than random escalation (R-rand) | E1 | R3 vs R-rand | secondary |
| RQ1 | **H1c** (ADR-019): at the same operating point, R3 is non-inferior in accuracy (margin δ) to the single encoder with its tuned threshold | E1 | R3 vs R1 | secondary |
| RQ3 | H3: adding Tier 3 to {1, 4} is a Pareto improvement; Tier 2 adds nothing beyond noise | E3 | tier sets {4}, {1,4}, {3,4}, {1,3,4} (+{1,2,3,4} only if Tier 2 is built) | secondary |
| RQ6 | H6: the Q4-class quantization stays within δ₃ of Q8 while using less RAM and running faster | E6 | each shortlisted SLM × {Q4, Q8}; baseline = Q8 of the same model | secondary |
| RQ7 | H7a: the calibrated cascade stays within δ of always-SLM **on Hinglish**; H7b: with one shared temperature, ECE is higher on Hinglish than on English | E1/E2 stratified by language | R3, R0, R2 | secondary |
| RQ4, RQ5 | as blueprint (personalization, forgetting) | E4, E5 | B6–B8 | secondary, first to drop (Ch. 52.1) |

Systems (Ch. 13, 35): **R0** always-generative (B3), **R1** single encoder + τ (B2), **R2** fixed cascade, raw softmax, one τ
(B4), **R2c** calibrated cascade with one shared τ (D9; offline from the same tier outputs), **R3** calibrated cascade,
per-tier temperature + per-tier τ tuned on validation (B5), **R-rand** random escalation at R3's per-tier escalation
rates (D10; expectation over ≥ 1000 seeds, offline), **R4** adaptive (only if built).
B1 (rules only) is reported with coverage and precision. R0 and Tier 4 inside the cascade use the **same** prompt template,
model, quantization, grammar and thread count.

**[accepted 2026-10-21, ADR-019] Baselines that abstain, and the operating point.** R0 and R1 each get one abstention
threshold (R0 on its calibrated Tier-4 confidence, R1 on the calibrated Tier-3 confidence), chosen on val to maximise
accuracy (ties: higher coverage). This is the same tuning effort the cascade gets (Ch. 35 rule 3), and an abstention is
correct on an OOS item (D3). The untuned R0 (always answers) is reported next to it; H1 and H7a use the tuned R0. The
**operating point** of R3 for H1, H1b, H1c and H7a is the cheapest point on val (per-tier τ, with the routing signal)
whose accuracy is ≥ max(acc(tuned R0), acc(tuned R1)) − δ; ties go to higher accuracy. The whole accuracy–cost frontier
is reported regardless. Chosen after the CLINC150 dev runs, where the earlier rule (≥ R0 − δ, R0 always answering)
picked Tier 3 alone; no test data was seen.
**[accepted 2026-10-22, ADR-021] Selection tolerance 0.** The operating point is the cheapest point on val whose accuracy
is ≥ max(acc(tuned R0), acc(tuned R1)), with no − δ: δ is only the test margin. A point chosen at max − δ sits on the
margin's edge, so its non-inferiority test fails even if test behaves exactly like val (dev check at δ = 0.02: CLINC150
R3 − tuned R1 = −0.020 on val with a test CI half-width ≈ 0.007, so H1c's lower bound ≈ −0.027; Intents-v0 R3 − tuned R0
= −0.016, lower bound ≈ −0.031). Points cheaper than the operating point stay on the reported frontier as exploratory.
Dev points (unchanged configs): CLINC150 0.912 at 140 ms, Intents-v0 0.984 at 478 ms.

**[accepted 2026-10-18, ADR-017] Routing signal and R2/R2c.** Every cascade (R2, R2c, R3, R-rand) sends a request that
contains a conjunction token (`and`, `then`, `aur`, `phir`, `fir`; `kestrel_core::features::has_conjunction` on the
normalized text) straight to Tier 4. Lower tiers are not consulted, and if Tier 4 fails the request abstains. Its
false-positive rate on single-intent items is reported per dataset and language. On the accuracy–cost frontier, R2 and R2c
use one shared escalation τ for the tiers below Tier 4, and Tier 4 always answers. H2's risk–coverage curve uses one τ
for every tier, Tier 4 included (D9). Dev finding (experiments/logs/router/README.md): in this cascade, temperature
scaling cannot move the accuracy–cost frontier, so H2 is a statement about abstention quality only.

**Selected components (dev runs, 2026-10-17):** Tier-3 encoder multilingual-e5-small Q8_0 (ADR-014). Tier-4 / R0 SLM
Gemma-4-E2B-it Q4_0 (ADR-016). Intents-v0 prompt = `datasets/contracts/tier4_intent_prompt.txt` + k = 2 shots per
(label, language) from train (ADR-015). Constrained label decoding gives the Tier-4 confidence; its temperature is fitted
on cal by BCE. CLINC150: the label set adds an explicit `oos` answer (ADR-016); prompt = `datasets/contracts/tier4_clinc150_desc_prompt.txt` (one-line description per intent, rules) + k = 1 shot per intent from train, runner context 8192 (chosen on the 300 + 300 dev subsets over k = 2 and label names only, ADR-018). The CLINC150 cascade is Tier 3 → Tier 4 (the Kestrel rules cannot answer with CLINC150 labels).

## 2. Data (frozen inputs)

| Dataset | Sets used | Fingerprint / source | Notes |
|---|---|---|---|
| Kestrel-Intents-v0 | train 604 / cal 100 / val 128 / **test 252** | full set `5de96809…e37ca314`; per-split in `datasets/kestrel_intents_v0/splits/manifest.json` | split by template family (EN + Hinglish together); OOS split by domain (test domains music, sports unseen); compound items in val/test only. Synthetic, Claude-drafted, human-reviewed — label as synthetic in every table |
| CLINC150 (full) | train 15000 / cal 1500 / val 1700 (200 OOS) / **test 5500 (1000 OOS)** | raw SHA-256 `36923c37…`; per-split in `datasets/clinc150/manifest.json` | official test intact; 24 texts occur in two sets (4 with conflicting labels; 14 test items) |
| Kestrel-Tasks-v0, -Personas-v0 | — | not built yet | added to this file before E4/E6 run |
| CLINC150 domains (E5) | the 150 in-scope intents of the rows above, grouped | `domains.json` SHA-256 `b947b579…` at the same pinned commit (datasets/MANIFEST.toml) | 10 domains × 15 intents; E5's stages (§4, ADR-027) |

**[accepted 2026-10-17] D1:** CLINC150 test keeps its 14 items whose text also appears in train/cal/val (primary, comparable with the
literature) and a sensitivity analysis drops them. **[accepted 2026-10-17] D2:** the 100 official *train* OOS items join val (threshold
tuning) instead of being unused.

## 3. Scoring rules and metrics

- **Per-item correctness** (`experiments/analysis/metrics.py`): an in-scope item is correct iff the prediction equals the gold
  label; **abstention is wrong** (Ch. 35 fairness rule 2). An **out-of-scope item** (CLINC150 `oos`; Kestrel-Intents
  `unsupported`) is correct iff the system abstains **or** predicts the OOS label (Ch. 8.3). **[accepted 2026-10-17] D3:** Ch. 35 says
  abstentions are errors without an OOS exception; read literally, no cascade could ever be right on CLINC150's OOS items,
  which have no head class (Ch. 32). This draft resolves it with the OOS exception above.
- **Compound items** (Kestrel-Intents, val/test only): correct iff not accepted as a single intent below Tier 4 (ADR-010).
  Whether Tier 4's plan covers the intents is evaluated in Kestrel-Tasks-v0, not here.
- **Accuracy, macro-F1** (mean over gold classes; abstention counts as predicting the OOS label), **coverage**, **selective
  risk** at coverage **0.80** **[accepted 2026-10-17] D4**, **AURC**, **ECE** with **15 equal-width bins, right-closed** `((m−1)/M, m/M]`
  **[accepted 2026-10-17] D5**, reliability diagrams. Accuracy-on-answered is reported next to end-to-end accuracy.
- **Latency:** end-to-end wall time per request on a monotonic clock (`kestrel-bench::measure`); per item, the median over
  ≥ 5 timed repetitions after 5 warm-up requests is the unit; mean, median and p95 (nearest rank) over items.
  **[accepted 2026-10-22, ADR-020]** Kestrel-Intents-v0 test: all 252 items × 5 repetitions. CLINC150 test: 5 timed
  repetitions on a fixed 20% latency subset (1100 items, seeded hash order of the ids, key `e1-latency`) and 1 timed run on
  every other item; Δlat (H1, H1b) and ρ use the subset, accuracy uses all 5500 items. (Full repetitions would cost
  ≈ 10.5 h of full CPU against ≈ 3.8 h, and H1's latency part is not close at dev numbers.) The decision of the first timed
  repetition is the one scored; every repetition's decision is logged and any disagreement is reported (greedy decoding:
  none expected).
- **Peak RSS:** `VmHWM` per process at the end of a run, MiB, with every GGUF model loaded **without mmap** (D12), so
  `VmHWM` is the real footprint; `RssAnon` and `RssFile` are logged next to it. **Energy: not measured** — the only counter (RAPL) is root-only on
  this machine; tables use the Ch. 37.4 template sentence.

## 4. Measurement protocol (latency experiments: E0, E1, E3, E6)

Machine: Ryzen 5 5500U (6C/12T), 14.95 GiB RAM, Fedora (kernel 7.2.6), Python 3.14.7, rustc 1.99.0, llama.cpp 26394b4e
(`llama-cpp-2 =0.1.157`). Per session: AC power with the battery full; tuned `throughput-performance` (governor
`performance`); **turbo boost off** (ADR-012); threads **pinned one per physical core** (ADR-011; mask `555` here); 6 threads;
the user's Docker containers stopped; ≥ 5 min cooldown and CPU ≤ 50 °C before starting; warm page cache (stated). Systems are
interleaved (A, B, A, B, …).

**[accepted 2026-10-22, ADR-020] E1 harness.** One process loads the models once and builds R0, R1 and R3 from their own
configs, which must agree on every model, prompt, shot and temperature field (one shared Tier-3 and Tier-4 instance, so
R0 and the cascade's Tier 4 are the same object). Per item and repetition the systems run in rotated order (item *i*,
repetition *r* starts at system (*i* + *r*) mod 3). Each row records the decision, the tier where routing ended, the
confidence, the wall time and each consulted tier's time. The test items, in a fixed seeded order, are cut into chunks of
about one hour; each chunk is one session with its own pre-checks and 5 warm-up requests per system (fixed train items),
and is the unit the invalidation rules below apply to: an invalid chunk is rerun completely. During a chunk, AC, boost,
the pinned CPUs' frequency and temperature, and the busy share of the CPUs outside the pinning mask are sampled every
10 s. Peak RSS per system comes from one short process per config that loads only that system's models.

**Invalidation rules (pre-declared; an invalidated run is rerun completely and logged):**
1. AC power lost or boost on at any point (the harness stops the run).
2. Background CPU busy > 10% in any cooldown window (E1: also the mean busy share of the CPUs outside the mask over any
   1-min window of a chunk).
3. Mean frequency of the pinned CPUs for a process deviates > 10% from that session's median for the same model and test
   (E1: a chunk's mean vs the median over that dataset's chunks).
4. CPU temperature > 95 °C after any process.
5. A crash or a model-file hash mismatch.

**E5 (head updates; accuracy only, so no latency protocol) — [accepted 2026-10-23, ADR-027], written before any sweep
result.** Data: CLINC150 in-scope items, e5 Q8_0 embeddings from the Week-5 cache (runtime route); the 10 domains of the
pinned `domains.json` (datasets/MANIFEST.toml), 5 stages × 2 domains in 3 seeded domain orders (`e5-order:1..3`), each
stage's train items for the update, every stage's items for the matrix R[i][j]. Methods: naive, replay (`per_class` stored
items of every earlier class), online EWC (summed diagonal Fisher, W only); controls joint (upper bound) and stage0 (lower
bound) at naive's selected lr / epochs. Minibatch SGD (batch 64) with seeds 1–5. Budget: 12 configurations per method; naive's
lr × epochs grid {0.3, 1, 3, 10} × {2, 5, 10} contains every pair the mitigations use ({1, 3} × {5, 10}; replay
`per_class` {5, 20, 50}; EWC λ {10, 100, 300}, from the measured Fisher scale). An EWC step with lr × λ × max F ≥ 2 is refused
as unstable and its configuration is not eligible. Dev selection: per method, the best mean average accuracy on val over
its 15 runs; ties → the earlier configuration. Reported per method: average accuracy, average forgetting, BWT (Ch. 22.3),
update seconds and buffer bytes, mean ± SD over the 15 runs, and every R matrix (`experiments/logs/E5/sweep_dev.json`).
The test-set run (Week 14) uses the selected configurations unchanged.

## 5. Targets from E0 (Ch. 6.4)

E0 session 1 under this protocol (20261011T101743Z; sessions 2–3 pending): Qwen3-1.7B Q4 prefill 90.6 tok/s, decode 18.2
tok/s, so a Tier-4 classification with a 128-token prompt and ~8 output tokens costs ≈ 1.4 s + 0.44 s ≈ **1.9 s** before any
load time; an encoder pass over 32 tokens costs ≈ **10.5 ms** (e5-small Q8), ≈ 175× less. These are llama-bench estimates, not
end-to-end measurements.

- **ρ (latency target for cascade-handled requests):** `median_latency ≤ 0.1 × median_latency(R0)` — one order of magnitude,
  which E0 supports with a wide margin. **[accepted 2026-10-17] D6**; recheck after E0 sessions 2–3 and the first end-to-end runs.
- **β (RAM ceiling):** `peak_RSS(all loaded models) ≤ 0.5 × 14.95 GiB = 7.47 GiB`. Largest shortlisted pair:
  Gemma-4-E2B Q8 (4.75 GiB) + e5-small F16 (0.38 GiB) = 5.1 GiB, so every pair fits. **[accepted 2026-10-17] D7**
- **δ (non-inferiority margin):** set after the Week-8 pilot (20–30% of dev data, ≥ 3 seeds) as
  `max(2 × seed-to-seed SD of R0 accuracy, smallest effect of interest agreed with the advisor)`. **δ = 0.0263, set
  2026-10-23 from the pilot below, before any test set was touched.** CLINC150 tuned-R0 accuracy on the 425 pilot val items,
  seeds 1–5: 0.861, 0.885, 0.878, 0.880, 0.854; mean 0.872, sample SD 0.0132, 2 × SD = 0.0263 > SESOI 0.02, so 2 × SD binds
  (`experiments/logs/router/delta_pilot.json`). Intents-v0, reported alongside: every seed 1.000 on its 32 pilot val items
  (SD 0). If the mentor sets the SESOI above 0.0263, δ becomes the SESOI. δ₃ (E6) is set the same way.
  **[accepted 2026-10-22, ADR-020] Pilot design.** R0 decodes greedily, so a seed draws R0's few-shot examples from train
  (`select_shots(..., seed)`: k per label and language, distinct families), the one arbitrary choice inside R0; item-sampling
  noise is left to the bootstrap CI. Pilot items: a fixed 25% of cal and of val per dataset (seeded hash order, key
  `delta-pilot`). Seeds 1–5; per seed, Tier 4 on the pilot items with the selected prompt, model and context, T by BCE on the
  pilot cal items, one abstention τ on the pilot val items (ADR-019), tuned-R0 accuracy on the pilot val items. One δ for every
  hypothesis, from CLINC150 (primary, D8); Intents-v0's SD is reported next to it. Smallest effect of interest: **0.02**
  (to be confirmed with the mentor). Code and logs: `experiments/router/delta_pilot.py`, `experiments/logs/router/delta_pilot*`.
- **ε₁–ε₃ (Ch. 23 gates for an updated head):** **[accepted 2026-10-22, ADR-022]** not set in Week 8. They gate updated
  Tier-3 heads, which only E4/E5 produce; both are outside the minimum system (Ch. 53) and early in the drop order
  (Ch. 52.1). If E4/E5 stay in scope, the ε values are set from a pilot and written here before either runs. ε₂ needs E5's
  stage matrix, and the head (convex lbfgs, no random state) has no seed variance, so that pilot must first name its
  variance source (e.g. bootstrap resamples of train).
  **[accepted 2026-10-23, ADR-027] Variance source and rules, written before any E5 sweep result.** ε₁ (macro-F1 drop on
  the frozen regression suite) and ε₃ (ECE rise) = 2 × the sample SD over 5 bootstrap resamples of CLINC150 train, each
  retraining the Tier-3 head with Week 5's C (100), T fitted on cal, scored on val in-scope with 15 ECE bins
  (`experiments/continual/epsilon_pilot.py`, seed 0). ε₂ (average forgetting) = 2 × the sample SD of the selected naive
  configuration's average forgetting over its 15 E5 dev runs (§4, `e5_sweep.py`). Values (filled from the logs; the rules
  do not change once they are read): **ε₁ = 0.0048, ε₃ = 0.0029** (2026-10-23; 5 retrains: val macro-F1 0.956 ± 0.0024,
  ECE 0.0121 ± 0.0015; `experiments/logs/E5/epsilon_pilot.json`), **ε₂ = TBD** (from the sweep).

## 6. Analysis plan

- **Primary (H1)**, an intersection–union test on paired item-level data (Ch. 34.1): (1) the lower bound of the 95%
  paired-bootstrap CI of Δacc = mean(correct_R3 − correct_R0) is > −δ, **and** (2) the upper bound of the CI of
  Δlat = mean(lat_R3 − lat_R0) is < 0. 10 000 bootstrap resamples over items, seed 0. No α-splitting is needed for the
  primary. **[accepted 2026-10-17] D8:** primary dataset = CLINC150 test (n = 5500); Kestrel-Intents-v0 test (n = 252) enters the secondary
  family, because its CI will be wide.
- **Secondary family** (Holm–Bonferroni at α = 0.05): H1b, H1c, H2, H3, H6, H7a, H7b (and H4, H5 if run). H1c is tested
  like H1's accuracy part: the lower bound of the 95% paired-bootstrap CI of mean(correct_R3 − correct_R1) is > −δ. Paired correctness: exact
  McNemar; latency: Wilcoxon signed-rank on per-item medians; effect sizes with CIs.
- **RQ7:** Kestrel-Intents-v0 test stratified by `lang` (en / hi-en, 126 items each): per-language Δacc and Δlat CIs (H7a);
  per-language ECE with one temperature fitted on the pooled cal set, paired bootstrap of ECE(hi-en) − ECE(en) (H7b). Per-language
  escalation rate to Tier 4 and mean latency are reported **descriptively** (D11; no test). A per-language temperature is
  **exploratory**. The small n per language is a stated limitation.
- **Seeds:** ≥ 5 per trained component (Tier-3 heads); all reported. Thresholds and temperatures are tuned on val / cal only.
  **[2026-10-22, ADR-020]** The Tier-3 head is a convex softmax regression fitted by lbfgs with no random state, so head
  seeds give identical heads and there is nothing to vary; the seeded component is R0's few-shot draw (δ pilot, §5).
- Anything decided after seeing test data is reported as **exploratory**.

## 7. Stopping

Sample sizes are the frozen test sets above. No optional stopping. Every run, including failed and invalidated ones, is
logged in `experiments/logs/` with hashes. Every pre-registered comparison is reported, including those that fail.

## 8. Open decisions (collected)

D1 CLINC150 duplicates · D2 train-OOS into val · D3 OOS abstention rule · D4 coverage 0.80 · D5 ECE 15 right-closed bins ·
D6 ρ = 0.1 · D7 β = 0.5 · D8 primary dataset: **all accepted 2026-10-17**. Still open: δ₃ (before E6).
δ's method, the smallest effect of interest (0.02, mentor to confirm) and the E1 harness and latency subset: accepted
2026-10-22 (ADR-020); **δ = 0.0263** from the pilot's 5 CLINC150 seeds (2026-10-23, §5). ε₁–ε₃: rules fixed before E5
(ADR-027, §5); values from their pilots.
**Contribution framing (accepted 2026-10-17):** a measured evaluation study ("we evaluate / replicate … on-device"); the
calibrated cascade is not claimed as new; the code-mixed Hinglish evaluation is the distinctive part.

Proposed 2026-10-17 from round-6 reading, accepted by the human the same day:

- **[accepted 2026-10-17] D9: add R2c, a calibrated cascade with one shared τ** (per-tier temperature, a single threshold chosen on val;
  the CAUC design, zhang2026cauc). R3 vs R2 changes the temperature *and* the threshold policy at once, so H2 cannot tell
  which helped. With R2c: R2c vs R2 isolates calibration (H2), and R3 vs R2c isolates per-tier thresholds (exploratory or a
  new secondary hypothesis). Cost: offline only (same logged tier outputs); no extra measurement runs.
- **[accepted 2026-10-17] D10: add a cost-matched random-escalation baseline** (R-rand) to E1/E2: at each tier, escalate a seeded random
  subset with the same escalation rate as R3. wang2026misguided finds that no commercial router beats random routing at
  matched cost; chaganti2026ondevicener reports the same comparison. Computed offline from logged per-tier outputs and
  measured per-tier costs (expectation over ≥ 1000 seeds). Resolved: tested as secondary hypothesis H1b (§1).
- **[accepted 2026-10-17] D11: report per-language escalation rate and cost under RQ7**: the share of requests reaching Tier 4 and the
  mean latency, per `lang`. moslem2026routingsurvey (ethics section) warns that cost-optimized routing can give
  non-standard language worse quality and a higher cost; jitkrittum2023deferral shows confidence deferral is weakest
  under distribution shift. Resolved: descriptive only (§6); n = 126 per language is too small for another test.
- **[accepted 2026-10-17] D12: fix the Tier-4 weight load mode and the RSS metric before E1/E6.** The H6 check (2026-10-17,
  `experiments/logs/E0/README.md`) shows that with mmap, llama.cpp's Q4 weight repacking makes `VmHWM` count the repacked
  weights twice. Qwen3-1.7B: Q4 saves 10% RAM vs Q8 with mmap, 39% without. Resolved: (a) load without mmap
  (`use_mmap = false`), so `VmHWM` is the real footprint (startup reads the whole file), **and** log `RssAnon` /
  `RssFile` next to `VmHWM` (§3). Either way the same mode applies to every system, and β and H6 are judged under
  it.
