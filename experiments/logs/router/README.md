# Router development runs (Week 7)

## Kestrel-Intents-v0 dev: tier outputs logged once, policies simulated offline (2026-10-18)

`python experiments/router/tier_outputs.py` → `tier_outputs_intents_v0.jsonl` (+ `.meta.json`): every tier's output for
all 228 cal + val items. Tier 1 = Rust `RuleSet::default_set` (`rules_items`). Tier 3 = e5-small Q8_0 + Week-5 head (raw
logits); the runtime vectors equal the training cache exactly (max gap 0.0). Tier 4 = Gemma-4-E2B-it Q4_0, k = 2
(ADR-015/016); 0 prompt-builder rejections. `python experiments/router/policies.py` → `policies_intents_v0.json`.
Timing: boost on, as found. These are dev costs, not E1.

Calibration (fitted on cal): Tier-1 confidence = the rules' precision on cal, **1.0 from only 3 matches** (val: 9 of
10; the miss is a compound accepted as `summarize_notes`). Tier-3 T = 0.69 (head, NLL). Tier-4 T = 0.85 (BCE). Scoring as in PREREG §3: abstaining is wrong in scope,
`unsupported` is right when abstained, and a compound is right iff it is not accepted as one intent below Tier 4
(ADR-010).

| Tier (val, 104 single-intent items) | Coverage | Accuracy | ECE raw → cal | Mean cost |
|---|---|---|---|---|
| 1 rules | 0.087 (9 items, all right) | | | 0.003 ms |
| 3 e5 head | 1.0 | 0.885 | 0.105 → 0.058 | 5.8 ms |
| 4 Gemma | 1.0 | 0.981 | 0.028 → 0.024 | 812 ms |

**Compound signal.** On val, a conjunction token ("and", "then", "aur", "phir", "fir"; `has_conjunction`) flags all 24
compound items and 0 of the 104 single ones. The rules' `Ambiguous` outcome flags only 3 of 24. Tier 3 alone accepts
compounds confidently: with no signal, the only way to keep them away from Tier 3 is τ3 ≈ 0.999.
**Caveat:** the compound items are Claude-drafted templates that always use a conjunction. Human-written requests (the
Request Lab) and CLINC150's single-intent items are the real test of this signal.

Policies on val, n = 128 (R0 = 0.984, 126 of 128). Each row is the cheapest point with accuracy ≥ R0 − δ. τ is chosen
on val and scored on val, so the numbers are optimistic. "Cross-fit" chooses τ on one half of val (split by template
family) and scores the other half, both ways.

| Policy | δ | Accuracy | Mean cost | Tier-4 rate | R-rand at matched cost | Cross-fit acc / cost |
|---|---|---|---|---|---|---|
| R0 always Gemma | — | 0.984 | 812 ms | 1.00 | | |
| R1 Tier 3 alone (always answers) | — | 0.719 | 6 ms | 0 | | |
| R3, no signal | 0 | 0.984 | 737 ms | 0.90 | 0.958 | 0.922 / 600 ms |
| R3, no signal | 0.02 | 0.969 | 669 ms | 0.82 | 0.948 | 0.883 / 540 ms |
| **R3 + conjunction → Tier 4** | 0 | 0.984 | **478 ms** | 0.57 | 0.946 | **0.953 / 412 ms** |
| R3 + conjunction → Tier 4 | 0.02 | 0.969 | 386 ms | 0.45 | 0.933 | 0.953 / 406 ms |
| R3 + conjunction → Tier 4 | 0.05 | 0.938 | 304 ms | 0.34 | 0.921 | 0.922 / 276 ms |
| R2 raw, shared escalation τ, + conjunction | 0 | 0.984 | 467 ms | 0.55 | | |
| R2c calibrated, shared escalation τ, + conjunction | 0 | 0.984 | 478 ms | 0.57 | | |

Readings (dev, small n; not results):
- The cascade's cost is almost all Tier 4 (812 ms vs 6 ms), so the saving is roughly 1 − the Tier-4 rate. With the
  conjunction signal, about half the requests stay below Tier 4 at R0's accuracy, in-sample.
- In-sample τ is optimistic by about 3 pp (0.984 → 0.953 cross-fitted, with the signal). One random family split was
  used, so this estimate is rough.
- **R3 beats random escalation at matched cost** by 1.5–3.8 pp in-sample (H1b's direction), with 1000 seeds.
- **Calibration cannot change this frontier.** Tier 1's confidence is one constant and Tier 4 always answers (every
  R3 optimum has τ4 = 0). So the only real decision is one threshold on Tier 3, and a temperature (monotone) only moves
  where that threshold sits. That is why R2, R2c and R3 match. H2 remains testable as written (selective risk at
  coverage 0.80 with one τ for every tier, Tier 4 included): R2 0.049 vs R2c 0.068 risk on ~103 answered items, a
  2-item difference.
- Hinglish costs more (D11). At δ = 0 with the signal: Tier-4 rate en 0.48 vs hi-en 0.66; mean cost 395 vs 560 ms.
  Both languages have accuracy 0.984.
- Tier-1 rules hardly matter here: 8.7% coverage, all of it from 4 template families, and their confidence is
  fitted on 3 matches.

Decided 2026-10-18 (ADR-017): the conjunction signal is part of every pre-registered cascade, and R2/R2c on the
frontier use one shared escalation τ with Tier 4 always answering. Still open: δ (after the pilot); the Tier-1 confidence
estimate (3 cal matches; kept for now, limitation stated).

Thresholds are midpoints between neighbouring observed confidences (`_grid`). On val this gives the same outcomes as
thresholds at the observed values, but no val item sits on a threshold, so f32 vs f64 rounding in the Rust router cannot
flip a decision. Switching to midpoints changed only the no-signal cross-fit costs (613 → 600, 545 → 540 ms).

## Dev router config (2026-10-18)

`python experiments/router/make_config.py --signal conjunction --delta 0.0` → `experiments/configs/router_intents_v0.json`.
Contents: R3 at δ = 0 with the signal, tiers [1, 3, 4], accept [0, -, 0.964, 0], the temperatures, model and prompt
hashes, and the val point it came from. `config_sha256` covers every other field. The Rust router reads it with
`RouterConfig::from_json` (tested against the committed file). It is marked DEV: δ is not set, so it gets regenerated
after the pilot. JSON, not router.toml: TOML in Rust needs the `toml` crate, which is not approved yet.

## The multi-intent signal on each dataset (2026-10-18, ADR-017)

`python experiments/router/signal_check.py` → `signal_check.json`. This uses the Rust rules runner over the dev splits; no
model is needed.

| Dev split | Single-intent flagged | OOS flagged | Compound flagged |
|---|---|---|---|
| Intents-v0 cal | 0 / 88 | 0 / 12 | — |
| Intents-v0 val | 0 / 88 | 0 / 16 | 24 / 24 |
| CLINC150 cal | 74 / 1500 (4.9%) | — | — |
| CLINC150 val | 74 / 1500 (4.9%) | 4 / 200 | — |

All the CLINC150 hits are "and" inside one request, for example "one hundred and fifty bucks" or "pto for march 2 and 3".
CLINC150 has no compound items, so there the signal can only cost: 4.9% of requests skip Tier 3 (0.960 in scope) for
Gemma (0.806 in scope on the 300-item subset), roughly −0.7 pp accuracy and ~5% more Tier-4 traffic. Under ADR-017 this
is reported, not tuned away: changing the signal after seeing CLINC150 dev data would need a new decision before the
freeze.
