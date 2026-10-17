# Tier 4 development runs

## B3 (always-SLM, R0) on Kestrel-Intents-v0 dev, zero-shot (2026-10-17)

`python experiments/tier4/b3_dev.py --model qwen3-1.7b-q4_k_m` → `b3_dev_intents_v0_qwen3-1.7b-q4_k_m_k0.json`.
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

## Few-shot (2026-10-17): tuning R0 / Tier 4 on dev

`--shots k` adds k examples per (label, language) from the **training split**, each from a different template family,
in round-robin label order. They are rendered as earlier ChatML turns inside the KV-cached prefix. Runner context
2048 tokens (k = 2 is ~1250 prompt tokens; at 1024 the runner refused with a clear error).

| Shots (k per label × language) | Val acc | Macro-F1 | en | hi-en | ECE raw → cal | T | p50 / p95 ms |
|---|---|---|---|---|---|---|---|
| 0 | 0.683 | 0.660 | 0.788 | 0.577 | 0.318 → 0.169 | 10.3 | 374 / 623 |
| 18 (k = 1) | 0.875 | 0.861 | 0.942 | 0.808 | 0.126 → 0.082 | 7.4 | 403 / see json |
| 36 (k = 2) | **0.904** | **0.893** | 0.962 | 0.846 | 0.076 → 0.087 | 7.1 | 554 / see json |

- Few-shot removes the collapse to `chitchat` and closes most of the gap. At k = 2, always-SLM matches the Tier-3 head
  (0.885) on Intents-v0 val; k = 1 vs 2 is 3 items apart (n = 104).
- A longer prefix costs latency on every request (each decoded token attends over the whole context): p50 374 → 554 ms.
  This is part of the R0 cost the cascade is compared against.
- Calibration: at k = 2 the cal-fitted T does not help val (0.076 → 0.087; 100 cal and 104 val items).
- Proposed (human to confirm): R0 and Tier 4 use k = 2 with Qwen3-1.7B Q4. Still open for SLM selection (Ch. 12.4):
  Qwen3-4B and Gemma-4-E2B (needs a Gemma chat format) at the same k.
