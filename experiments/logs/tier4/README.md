# Tier 4 development runs

## B3 (always-SLM, R0) on Kestrel-Intents-v0 dev, zero-shot (2026-10-17)

`python experiments/tier4/b3_dev.py --model qwen3-1.7b-q4_k_m` → `b3_dev_intents_v0_qwen3-1.7b-q4_k_m.json`.
Setup: Qwen3-1.7B Q4_K_M; ChatML with thinking off; zero-shot prompt contract
`datasets/contracts/tier4_intent_prompt.txt` (label definitions + decision rules, no dataset items; a test enforces
that); normalized request text; constrained label decoding; temperature fitted on cal by BCE. Cal + val only; the
test set was not touched.

| | n | Accuracy | Macro-F1 | ECE raw → calibrated |
|---|---|---|---|---|
| val (single intent) | 104 | 0.683 | 0.660 | 0.318 → 0.169 (T = 10.3) |
| val en | 52 | 0.788 | 0.754 | 0.221 (calibrated) |
| val hi-en | 52 | 0.577 | 0.544 | 0.255 (calibrated) |

- The most frequent errors are collapses to `chitchat`: set_preference → chitchat 10, retrieve_memory → chitchat 7,
  unsupported → chitchat 6. Then create_task → unsupported 3.
- The model is very overconfident (raw ECE 0.32; T = 10.3 needed).
- Hinglish is 21 pp below English.
- On compound items (n = 24), the chosen label is always one of the gold intents.
- Dev timing (boost on, not E1): mean 423 ms, p50 374, p95 623 per request. The 9-label prompt prefix is KV-cached.

**Reading:** this zero-shot R0 is far below the Tier-3 head (val accuracy 0.885). Taken as the baseline, it would make
H1 trivially true. Ch. 35's fairness rule asks for equal tuning effort, so R0/Tier 4 must be tuned on dev before the
PREREG freeze. Candidates: few-shot examples from train (cheap: the KV-cached prefix makes them free per request),
other shortlisted SLMs (Qwen3-4B, Gemma-4-E2B), thinking on (slow). Every variant tried is logged here.
