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

Rows marked † were checked against the **full text** on 2026-10-11 (arXiv PDFs: C3 v1, CascadeBERT, chuang2025 v1, chaganti2026 v1); the others are still abstract-level. "?" = not stated.

| Work | What is cascaded | Confidence signal | Calibrated? | Budget-aware? | Resources measured | On-device? | Rule tier? | Eval data |
|---|---|---|---|---|---|---|---|---|
| C3 (lu2024c3) † | multilingual encoders of increasing size (DistilBERT → mBERT → XLM-R base → XLM-R large); also a Llama-2 cascade | max class probability after logit normalization + per-model temperature (fit on validation data) | **yes** | speed-up targets 2×/3×/4× via the threshold | **FLOPs only** (speed-up ratio); no wall-clock, RAM or energy | no (cloud compute; motivation only) | no | XNLI, PAWS-X, QAM; GSM8k + TabMWP (500 items, machine-translated) — no code-mixed data |
| CascadeBERT (li2021cascadebert) † | complete BERT models of increasing depth | max class probability, shaped by a difficulty-aware regularizer; ECE reported | yes (training objective) | speed-up target via the threshold | **FLOPs / layers executed** (estimated speed-up) | no | no | 6 GLUE classification tasks |
| BabyBear (khalili2022babybear) | cheap model → deep model | confidence threshold | ? | compute cost | compute | no | no | doc classification, CoNLL NER |
| chuang2025ondevice † | SLM → stronger LLM (8 SLMs, 2 LLMs) | 8 uncertainty-quantification methods | calibration-data pipeline for routing generalization | fraction routed to the LLM (invocation cost) | none on a device: run on 4× A100 80 GB ("on-device" is the setting being emulated) | **emulated** | no | 14 reasoning / QA / MMLU-style datasets; no intent data |
| chaganti2026ondevicener † | GLiNER-small → GLiNER-large (encoder → encoder, NER) | per-span confidence, document-level gate | yes (temperature; ECE 0.24–0.47 roughly halved) | compared at matched budgets vs cost-matched random routing | per-document latency, output validity; no RAM or energy | yes (Apple M4 Pro, Ollama) | no | CoNLL-2003, WNUT-17, RSS-News (silver gold) |
| koul2026hybridinfer | device → edge → cloud LLMs | Q-learning policy (thermal + complexity) | n/a | latency, cost, thermal | latency, cost | first tier only | no | 210 Android prompts |
| arora2024intentllm (abstract) | SetFit encoder → LLM (intent detection) | uncertainty of the encoder | ? | latency | latency (−50% vs LLM-only within 2% accuracy) | ? | no | intent benchmarks incl. OOS |
| zhang2026cauc (abstract) | heterogeneous models (accept / escalate / combine) | post-hoc calibrated confidence per model | **yes** (calibrated thresholds = selective-risk meaning) | validation-chosen policy per budget | strong-model calls, GFLOPs | no | no | 6 language benchmarks + images |
| sadhu2026neurosymbolic (abstract) | deterministic solvers → SLM (learned DFA router) | none (deterministic routing) | n/a | token budget | latency **and energy** | **yes** (Raspberry Pi 4B) | **yes** (symbolic) | 100 maths/logic prompts |
| mane2026cascadens (abstract) | symbolic semigraph → neural module | calibrated symbolic score | yes | ? | ? | ? | **yes** (symbolic) | product-review sarcasm |
| alkiek2026harissa (abstract) | cheapest-first answering modes → deferral | hidden-state self-checks | trained | latency | latency | yes (single device) | no | 6 task/setting pairs |
| **Kestrel (planned)** | **rules → frozen encoder + head → local SLM** | **calibrated probability per tier** | **yes (temperature)** | **latency/RAM budget** | **latency, peak RSS, energy if accessible** | **yes, fully** | **yes** | **CLINC150 + Kestrel-Intents-v0 (EN + Hinglish)** |

**Reading (updated 2026-10-11 after full texts of the four † rows and rounds 4–5):** no single prior work matches every column, but each planned difference now has close prior work on its own:
- *Calibrated cascade with validation-chosen thresholds:* C3, CascadeBERT and especially **zhang2026cauc**, which also shows calibrated thresholds have a selective-risk meaning. RQ2 is a **replication in Kestrel's setting**, not a new idea.
- *Encoder → LLM routing for intent detection with latency gains:* **arora2024intentllm** (SetFit → LLM, −50% latency within 2% accuracy). RQ1's Tier 3 → Tier 4 part is close to it; its hardware and calibration are not in the abstract (full text needed).
- *A deterministic tier before a model:* **sadhu2026neurosymbolic** (on a Raspberry Pi, with latency and energy) and mane2026cascadens (calibrated symbolic → neural).
- *On-device resource measurement:* sadhu2026neurosymbolic, alkiek2026harissa, chaganti2026ondevicener.

What still differs, as a **combination** (Ch. 4.5: don't claim novelty because components are combined):
- the full rules → calibrated encoder head → local generative SLM cascade **for intent routing**, measured end to end on a commodity x86 CPU (latency + peak RSS);
- **code-mixed Hinglish**: no cascade, routing or calibration study in the search evaluates code-mixed input (RQ7). This is the most distinctive part.

Wording: "we *evaluate* …" / "we *replicate* … on-device". Claims of the form "no prior work in our search reports …" are only for the Hinglish cascade/calibration evaluation, and only after the stop rule is met. **The contribution framing is the human's decision.**

## Open items (Week 2)

- **Stop rule not met.** Round 4 (8 OpenAlex queries) added 21 relevant papers and round 5 (forward chaining) added 8; almost all are from 2025–26. With a literal "two rounds with nothing new" rule, the search will not end in this field. **Proposal for the human:** count as "new relevant" only works that match ≥ 3 of the 8 Ch. 4.5 columns (or are code-mixed intent/calibration work); others go to the matrix as background. Next: backward chaining from the TMLR routing survey (moslem2026routingsurvey) and from the full texts below.
- Done 2026-10-11: full texts of C3, CascadeBERT, chuang2025ondevice, chaganti2026ondevicener (table rows †).
- **Read full texts next:** arora2024intentllm, zhang2026cauc, sadhu2026neurosymbolic (hardware, calibration, budget columns).
- Search notes: Semantic Scholar keyword search was rate-limited and DBLP blocks scripts; ACL venues were covered through OpenAlex.
- **Venue checks:** confirm each entry marked "venue not verified" before citing it.
- PREREG draft v0: `experiments/PREREG.md` (2026-10-11).
