# E4 logs (personalization through taught aliases; ADR-028)

Dev only (Kestrel-Personas-v0 DRAFT, dev personas p1, p4, p7, p8; 4 × 30 requests × 4 conditions). Synthetic data: label it
so in every table. Not test results: the test split waits for the dataset's review and freeze.

## Gold intents (primary) — `e4_dev.json`

`kestrel e4-eval` (0.5 s, no model). Per persona and condition: a fresh memory; K taught events through the real path
(alias detection, a yes, the store); each request alias-resolved, planned by the template planner with its gold intent, run
on a fresh copy of the fixture notes, judged like Kestrel-Tasks-v0. Mean over the 4 personas:

| Condition | Personalized, exact (n 15 / persona) | Personalized, variant (n 5) | General (n 10) |
|---|---|---|---|
| off | 0.00 | 0.00 | 1.00 |
| K = 1 | 0.20 | 0.00 | 1.00 |
| K = 3 | 0.60 | 0.00 | 1.00 |
| K = 5 | **1.00** | **0.00** | 1.00 |

All 20 dev events were detected. Alias resolution costs ~µs per request (`resolve_us_mean`).

Read this as a check of the mechanism, not a size of benefit: exact requests are in the planner's reach by construction
(dataset card), so each taught alias unlocks exactly its own requests (success grows K / 5), and general requests do not
regress. The finding is the **variants: 0 in every condition**. A literal phrase matcher misses plurals, Hinglish oblique
forms ("doston", "adde pe") and spelling variants; matching them needs normalization or embeddings (Tier 3's encoder could
match an alias by meaning, not tested here). "Off" fails by an unknown tag (nothing to summarize, 32), an unfound document
(12), the wrong file ("thesis wala doc" read a dated thesis note, 4) or a title that keeps the literal alias.

## Tier 1 + Tier 3 routing (secondary) — `e4_dev_cascade.json`

`kestrel e4-eval --cascade experiments/configs/router_intents_v0.json` (release build, 2 s): the same run with each
alias-resolved request routed by the dev router's Tier 1 and Tier 3 (τ3 0.964); Tier 4 is left out, so an abstention fails.

| Condition | Personalized, exact | Variant | General |
|---|---|---|---|
| off | 0.00 | 0.00 | 0.45 |
| K = 1 | 0.17 | 0.00 | 0.45 |
| K = 3 | 0.27 | 0.00 | 0.45 |
| K = 5 | 0.50 | 0.00 | 0.45 |

At K = 5 no request was routed to a wrong intent: every cascade-only failure is an abstention below Tier 3's threshold,
far more in Hinglish (general: 16 of 20 abstain vs 6 of 20 in English), the per-language gap E1's dev runs show. With Tier
4 those requests would go to Gemma (ADR-028 left the full cascade out).
