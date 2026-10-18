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

## SLM comparison at k = 2 (2026-10-17, Ch. 12.4)

The human confirmed k = 2 ("continue" after the proposal). The same prompt contract and shots, all three shortlisted SLMs
at Q4. The chat format comes from the GGUF `general.architecture`. The Gemma 4 format was checked against the GGUF's
own Jinja template, rendered on a sample chat with thinking off.

| SLM | Val acc | Macro-F1 | en | hi-en | ECE raw → cal | T | mean / p50 / p95 ms |
|---|---|---|---|---|---|---|---|
| Qwen3-1.7B Q4_K_M | 0.904 | 0.893 | 0.962 | 0.846 | 0.076 → 0.087 | 7.1 | 687 / 554 / 804 |
| Gemma-4-E2B-it Q4_0 | 0.981 | 0.978 | 0.981 | 0.981 | 0.028 → 0.024 | 0.85 | 681 / 550 / 676 |
| Qwen3-4B Q4_K_M | 0.990 | 0.989 | 1.000 | 0.981 | 0.010 → 0.018 | 3.9 | 1762 / 1402 / 2242 |

- Gemma-4-E2B matches Qwen3-4B within 1 item (n = 104) at Qwen3-1.7B's latency, with no Hinglish gap. Its remaining
  errors are chitchat → unsupported (2). Qwen3-1.7B's errors are spread over several labels.
- Calibration: Gemma and Qwen3-4B are already close to calibrated (raw ECE 0.028 / 0.010). Qwen3-1.7B is not.
- Licenses per the manifest: all Apache-2.0 (verified 2026-10-10).
- Caveats: dev only, n = 104, timing as found (boost on). Gemma-4-E2B is the model kasnesis2026callsite found unsafe in
  tool calls, so the allowlist and validation of Ch. 15 stay mandatory whichever SLM is chosen.
- **Proposed (human to decide): Tier-4 SLM = Gemma-4-E2B-it Q4_0.** With an always-SLM baseline this strong, H1 becomes
  a real test: the cascade must escalate the hard requests to keep accuracy within δ of 0.98.

## CLINC150, Gemma-4-E2B, k = 1, fixed 300-item subsets (2026-10-17, ADR-016)

`python experiments/tier4/b3_dev.py --model gemma-4-e2b-it-q4_0 --dataset clinc150 --shots 1 --subset 300` →
`b3_dev_clinc150_gemma-4-e2b-it-q4_0_k1_n300.json`. Prompt contract `datasets/contracts/tier4_clinc150_prompt.txt`
(built by `experiments/tier4/clinc_prompt.py`): the 150 train intents by name, plus `oos` ("none of the listed intents").
CLINC150 ships no definitions, and a test keeps every dataset item out of the prompt. There is one example per intent
from train (150 shots). `oos` has none, because train has no OOS items. The prefix (~3k tokens) is primed into the KV
cache once at load (`LlamaSession::prime`), so it does not count against the first request. Runner context: 6144.
Subsets: a seeded sample of 300 from cal (0 OOS; cal has none) and 300 from val (37 OOS).

| | n | Accuracy | Macro-F1 | ECE raw → calibrated |
|---|---|---|---|---|
| cal | 300 | 0.827 | | (T = 1.35 fitted here) |
| val, all | 300 | 0.767 | 0.776 | 0.128 → 0.071 |
| val, in scope | 263 | 0.806 (212) | | |
| val, OOS | 37 | recall 0.486 (18) | | |

- On CLINC150 the always-SLM baseline is weak. The Tier-3 head scores 0.960 on all 1500 in-scope val items. Tier 4 is
  about 15 pp below that, on a subset. This is the opposite of Intents-v0, where Gemma beats the head.
- OOS: 19 of 37 OOS items get an intent (fun_fact 8; calculator and time 2 each). Only 4 of 263 in-scope items are
  answered `oos`. So the model rarely abstains, and the router's abstention will have to catch OOS.
- In-scope errors mostly fall on near-duplicate intent names: income → w2 4, freeze_account ↔ account_blocked 3,
  travel_notification → travel_alert, calendar → calendar_update, reminder_update → reminder,
  shopping_list → shopping_list_update, meal_suggestion → recipe (2 each). With label names alone, some pairs are
  ambiguous.
- Dev timing (boost on, not E1): mean 806 ms, p50 816, p95 1197. Gemma on Intents-v0 was mean 681 ms, p50 550, so the
  longer prefix costs every request time.
- Caveats: only 300 + 300 items; the temperature was fitted on cal, which has no OOS items.
- Open (Ch. 35 equal effort, before the PREREG freeze): CLINC150 R0 is not yet tuned the way Intents-v0 was.
  Candidates: k = 2 (300 shots, ~5–6k-token prefix, slower per request), or one-line intent descriptions written from
  the label names (no dataset text). Either one is tried on these same subsets and logged here.

## CLINC150 R0 tuning: three variants on the same subsets (2026-10-18, ADR-018)

Same 300 cal + 300 val subsets (37 val OOS) and Gemma-4-E2B Q4_0, with runner context 8192. Context size changes what
fits, not the results. Dev timing, boost on, as found. Selection rule fixed beforehand (ADR-018): higher val accuracy.

| Variant | Prefix tokens | cal acc | val acc | val macro-F1 | in scope | OOS recall | ECE raw → cal | mean / p95 ms |
|---|---|---|---|---|---|---|---|---|
| k = 1, label names (`tier4_clinc150_prompt.txt`) | ~3k (estimate; not logged) | 0.827 | 0.767 | 0.776 | 0.806 | 0.486 | 0.128 → 0.071 | 806 / 1197 |
| k = 2, label names | 7341 | 0.887 | 0.820 | 0.855 | 0.894 | 0.297 | 0.116 → 0.080 | 1208 / 1814 |
| **k = 1 + descriptions** (`tier4_clinc150_desc_prompt.txt`) | 5485 | 0.923 | **0.833** | 0.888 | **0.924** | 0.189 | 0.098 → 0.063 | 974 / 1416 |

- **Chosen: k = 1 + descriptions.** It is the most accurate on val, both overall and in scope (0.924 vs the Tier-3 head's
  0.960), and faster than k = 2. `b3_dev_clinc150_gemma-4-e2b-it-q4_0_k1_n300_tier4_clinc150_desc_prompt.json`.
- The price is OOS: the better the in-scope prompt, the less often Gemma answers `oos` (0.486 → 0.297 → 0.189). Most
  remaining errors are OOS items labelled `definition` (8) or `fun_fact` (8). Abstention below a Tier-4 threshold is what
  can catch them.
- Optimism: the description rules target the pairs the k = 1 run confused on this val subset, and the choice is made on
  the same subset. The full-dev run (tier outputs, 3200 items) and the frozen test are the checks.
- Priming the prefix takes 2–3 min at load (5485 tokens in 128 s). This is a load-time cost, not a per-request one.
