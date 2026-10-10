# Research questions — draft v0 (2026-10-11)

Week-1 deliverable (blueprint Ch. 52). This adapts the blueprint's RQ1–RQ6 (Ch. 3) to the project decisions: English + Hinglish scope (ADR-005) and the measured model shortlist. Evidence comes from `docs/literature_matrix.csv` (44 entries), which records abstracts only; **no full texts have been read yet**. Hypotheses stay unproven until the PREREG protocol (Ch. 34) runs. The margins δ are set from the pilot, not here.

## Questions

| RQ | Question | Hypothesis | Change from blueprint |
|---|---|---|---|
| **RQ1** (primary) | Can a rule → encoder → local-SLM cascade cut mean latency (and energy, if measurable) compared with always calling the SLM, at non-inferior accuracy, on a commodity CPU? | H1: at a validation-chosen operating point, cascade accuracy is within δ of always-SLM, and mean latency is lower. | Results are reported separately for the English and Hinglish strata. |
| **RQ2** | Does temperature scaling improve routing (selective risk at equal coverage)? | H2: calibrated confidences give lower selective risk than raw softmax. | C3 (lu2024c3) already reports calibration-before-cascade gains, so RQ2 is a **replication** in a new setting, not a novelty claim. |
| **RQ3** | Is each tier needed? | H3: adding Tier 3 to Tier 1+4 is a Pareto improvement; Tier 2 adds nothing beyond noise. | Unchanged. |
| **RQ4** | Does correction-based personalization help? | H4: as in blueprint. | Unchanged (secondary). |
| **RQ5** | Do replay/EWC reduce forgetting of the adaptive head? | H5: as in blueprint. | Unchanged (secondary). |
| **RQ6** | What do quantization, context length and threads cost, and which tasks can the local SLM handle? | H6: the Q4-class quant stays within δ₃ of the Q8 quant while using less RAM and running faster. | Models: Qwen3-1.7B, Qwen3-4B, Gemma-4-E2B, each at 2 quants (ADR-005). |
| **RQ7** (new) | Do the cascade's gains and the calibration's benefit hold for Hinglish (code-mixed) requests? Does a temperature fitted on mixed data stay calibrated per language? | H7: the calibrated cascade stays within δ of always-SLM on Hinglish. Per-language ECE after one shared temperature is higher for Hinglish than for English. | Motivated by C3's claim that confidence distributions vary across languages, and by code-switching degrading NLU (birshert2021larisa). |

## Implications from the search (abstract-level)

1. **Novelty must be narrowed.** "Calibrated model cascade" for NLU exists: C3 (cross-lingual, calibration before cascade) and CascadeBERT (calibrated complete-model cascade). Kestrel may only claim what the comparison table below supports, using the Ch. 5.4 wording ladder.
2. **Tier-4 confidence should come from label probabilities, not the model's own stated (verbalized) confidence.** shen2026certified shows temperature scaling cannot repair verbalized confidence that stays above 0.5 while accuracy falls. This matches the blueprint's constrained-decoding approach (Ch. 11.1).
3. **Safety: Gemma-4-E2B.** kasnesis2026callsite reports it actuated on 87.2% of requests for devices the site does not own. Tool calls must stay allowlisted and validated (Ch. 15), and wrong-target requests belong in Kestrel-Tasks-v0.
4. **Model calibration differs per SLM.** chuang2025ondevice reports that uncertainty distributions depend more on the SLM and the uncertainty method than on the data. Thresholds must therefore be tuned per model, never shared.
5. **Hinglish data:** there's no public Hinglish *intent* benchmark among the hits. MASSIVE has hi-IN, but it's translated, not code-mixed. CST5 (agarwal2022cst5) and code-switching augmentation (krishnan2021mcs) are routes to generating Hinglish items for Kestrel-Intents-v0, which still needs human checking.

## Closest prior work (preliminary, Ch. 4.5 columns, from abstracts)

"?" = not stated in the abstract. To be confirmed from full texts in Week 2.

| Work | What is cascaded | Confidence signal | Calibrated? | Budget-aware? | Resources measured | On-device? | Rule tier? | Eval data |
|---|---|---|---|---|---|---|---|---|
| C3 (lu2024c3) | multilingual PLMs of different sizes | model confidence | **yes** | ? | "inference efficiency" (?) | ? | no | 3 cross-lingual benchmarks |
| CascadeBERT (li2021cascadebert) | complete PLMs of different sizes | class probability (difficulty-aware) | yes (training objective) | speed-up target | speed-up | no | no | 6 classification tasks |
| BabyBear (khalili2022babybear) | cheap model → deep model | confidence threshold | ? | compute cost | compute | no | no | doc classification, CoNLL NER |
| chuang2025ondevice | on-device SLM → stronger LLM | uncertainty (several UQ methods) | calibration-data pipeline | invocation cost | ? | SLM on device | no | 1500+ settings |
| chaganti2026ondevicener | local small → large (NER) | per-span confidence | yes (temperature) | cost-matched routing | latency, output validity | yes | no | 3 NER datasets |
| koul2026hybridinfer | device → edge → cloud LLMs | Q-learning policy (thermal + complexity) | n/a | latency, cost, thermal | latency, cost | first tier only | no | 210 Android prompts |
| **Kestrel (planned)** | **rules → frozen encoder + head → local SLM** | **calibrated probability per tier** | **yes (temperature)** | **latency/RAM budget** | **latency, peak RSS, energy if accessible** | **yes, fully** | **yes** | **CLINC150 + Kestrel-Intents-v0 (EN + Hinglish)** |

**Reading (abstract level only):** no single prior work matches every column. The differences that survive so far:
- a **heterogeneous** cascade (deterministic rules, encoder and generative tiers);
- **fully on-device** on a commodity CPU, with joint latency/RAM/energy reporting;
- **code-mixed Hinglish** evaluation.

Wording for now: "we *evaluate* …". Upgrading to "no prior work in our search reports …" requires the Week-2 full-text checks below.

## Open items (Week 2)

- **Stop rule not met.** Round 3 still added 4 relevant papers. Continue with:
  - ACL Anthology and Semantic Scholar searches (Ch. 4.4 queries);
  - forward chaining from C3, CascadeBERT and chuang2025ondevice;
  - backward chaining from their related-work sections.
- **Read full texts** of C3, CascadeBERT, chuang2025ondevice and chaganti2026ondevicener, and fill in the "?" cells.
- **Venue checks:** confirm each entry marked "venue not verified" before citing it.
- **Write PREREG.md:** RQ7 needs its own stratified analysis plan, with paired tests per language and Holm correction across strata.
