# Tier 3 development runs

## Week 5 (2026-10-17): encoder comparison, head, calibration, parity

Development splits only (train / cal / val); **the test sets were not loaded** (`experiments/tier3/splits.py` cannot load
them). Raw numbers: `week5_dev.json` (from `python experiments/tier3/train.py`) and `encoder_parity.json`.

Setup:
- Embeddings: `llama-embedding` at llama.cpp 26394b4e (the runtime's engine), model pooling, L2-normalized. Input =
  encoder prefix + `norm_text` (e5: `"query: "`; bge: none).
- Head: multinomial logistic regression (sklearn lbfgs). C is chosen from {0.1, 1, 10, 100, 1000} by val macro-F1.
- Temperature: fitted on cal. Metrics are on single-intent, in-scope val items; ECE uses 15 right-closed bins (D5).

| Dataset | Encoder | C | T | Val acc | Val macro-F1 | ECE raw → calibrated | Extra |
|---|---|---|---|---|---|---|---|
| Intents-v0 (val n = 104) | bge-small-en Q8_0 | 10 | 0.51 | 0.865 | 0.866 | 0.185 → 0.069 | en 0.904 / hi-en 0.827 acc |
| Intents-v0 | multilingual-e5-small Q8_0 | 100 | 0.69 | **0.885** | **0.882** | 0.100 → 0.054 | en 0.923 / hi-en 0.846 acc |
| CLINC150 (val n = 1500 + 200 OOS) | bge-small-en Q8_0 | 100 | 0.78 | **0.975** | **0.975** | 0.035 → 0.011 | OOS AUROC 0.954 |
| CLINC150 | multilingual-e5-small Q8_0 | 100 | 0.67 | 0.960 | 0.960 | 0.055 → 0.012 | OOS AUROC 0.955 |

Findings:
1. **Every head is underconfident before calibration (T < 1).** The L2 penalty shrinks the weights. Temperature scaling
   cuts ECE by 46–79%, and accuracy does not change (by construction).
2. **The two datasets prefer different encoders.** On Intents-v0, e5 is ahead by 2 val items (n = 104), which is not a
   difference. On CLINC150, bge is ahead by 1.5 pp (≈ 22 of 1500 items). Both detect OOS equally well (AUROC 0.954 /
   0.955). The encoder choice is the human's (see below).
3. **Hinglish is harder and less well calibrated** for both encoders. hi-en accuracy is 6–8 pp below en, and hi-en ECE
   is 0.110–0.135 vs 0.074–0.080. This is the H7b direction, but it is dev data with n = 52 per language: no claim.
4. **The head does not flag compound requests.** Its mean top probability is 0.887 on the 24 compound val items vs
   0.879 on single-intent ones (e5), so a confidence threshold will accept them at Tier 3. Under the PREREG scoring rule
   that is wrong. The router needs another signal for them (Week 7; e.g. Tier 1's conjunction flag sends a request
   straight to Tier 4).
5. **Seeds do not matter for this head.** The fit is convex and lbfgs is deterministic, so 5 seeds give the same head.
   If seed-to-seed spread is needed (PREREG §6, δ), it has to come from resampling the training data.
6. **Parity passes.**
   - Rust `LinearHead` vs Python: ≤ 1e-5 on the committed fixture (`datasets/contracts/head_parity/`) and on all four
     trained heads (`KESTREL_HEAD_DIR=… cargo test -p kestrel-nlu -- --ignored`).
   - GGUF e5 vs its PyTorch reference, on 50 en + 50 hi-en texts, against tolerances stated before the run: F16 min
     cosine 0.999999 (≥ 0.9999), Q8_0 0.999813 (≥ 0.999). The "GGUF mismatch → ONNX" risk (Ch. 47.4) is retired for e5.
     bge has no reference check (its source weights aren't downloaded).

Cost on the laptop: embedding 19,032 texts × 2 encoders took 5.5 min. Training took 8 min wall-clock on all 12 threads
(94 CPU-minutes), almost all of it the CLINC150 C grid. The time of one fit was not measured first, as Ch. 33.3 asks.

Encoder choice (human, 2026-10-17, replying "continue" to Claude's recommendation): **multilingual-e5-small for both
datasets**, accepting ~1.5 pp on CLINC150 Tier 3 for one system that covers Hinglish (ADR-014).

### e5 F16 vs Q8_0 (2026-10-17, Ch. 11.4)

After the human chose multilingual-e5-small, both quantizations were run with C fixed at 100 (the C chosen for Q8_0 on
both datasets): `python experiments/tier3/train.py --encoders multilingual-e5-small-f16 --c 100`
(`week5_e5_f16.json`; one fit was timed first: CLINC150 64 s, Intents-v0 2 s).

| Dataset | F16 acc / F1 / ECE (calibrated) | Q8_0 acc / F1 / ECE (calibrated) |
|---|---|---|
| Intents-v0 (n = 104) | 0.894 / 0.889 / 0.085 | 0.885 / 0.882 / 0.054 |
| CLINC150 (n = 1500) | 0.959 / 0.959 / 0.010 | 0.960 / 0.960 / 0.012 |

Q8_0 matches F16 within noise: 1 Intents-v0 item and 1 CLINC150 item apart, ECE within ±0.03 on 104 items. It is half
the size (0.13 vs 0.24 GB) and faster in E0 (≈ 9.0 vs 10.3 ms per 32-token pass). **Recommendation: the Tier-3 runtime
uses multilingual-e5-small Q8_0.**

### Train/serve parity of embeddings (2026-10-17)

The Rust runtime embedder (`LlamaEmbedSession`, one text at a time) differed from the training cache by up to 2.9e-3 per
component (cosine ≈ 0.9999). Cause: llama.cpp's numerics depend on the batch size. `llama-embedding` gives the same
difference between a 100-text batch and a single text (e.g. "query: hi": 2.05e-3), while Rust and the CLI agree to 5e-8
on single texts. The cache built from 2048-text CLI batches was therefore training the head on vectors the runtime never
produces.

Fix: the cache now uses the runtime path (`embed_texts` example, one text at a time; cache key tag `runtime-single-v1`).
19,032 texts took ~3 min. The e5 Q8_0 heads were retrained at C = 100 (`week5_e5_q8_runtime.json`). Accuracy and macro-F1
are unchanged on both datasets; ECE moved by ≤ 0.004 (Intents-v0 0.054 → 0.058, CLINC150 0.012 → 0.010); OOS AUROC
0.9552 → 0.9551. The skew was small, but now train and serve match by construction. The bge rows and the F16 check above
used the CLI-batch cache.
