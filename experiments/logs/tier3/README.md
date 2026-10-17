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

Open for the human: **which encoder** (one for both datasets, or one per dataset). If e5 is chosen, an F16 vs Q8_0 check
of macro-F1 and ECE is still due (Ch. 11.4).
