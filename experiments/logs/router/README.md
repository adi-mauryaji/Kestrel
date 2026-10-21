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

Policies on val, n = 128 (R0 = 0.984, 126 of 128). Each row is the cheapest point with accuracy ≥ R0 − δ (ADR-019's reference, the better of tuned R0 and tuned
R1, is the same 0.984 here: abstaining never helps R0 on Intents-v0). τ is chosen
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
Contents: R3 at δ = 0 with the signal, tiers [1, 3, 4], accept [0, -, 0.964, 0], the temperatures, model files and
hashes, the prompt, Tier 4's context and its 36 few-shot examples, per-tier cost estimates, and the val point it came
from, so the runtime needs nothing else. `config_sha256` covers every other field. The Rust router reads it with
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

## The Rust cascade reproduces the simulation (2026-10-18)

`cargo build --release -p kestrel-cli --features llama && python experiments/router/check_runtime.py` →
`runtime_check_intents_v0.json`. `kestrel route-items` builds the real cascade from the dev config: `RulesTier`,
`EncoderTier` (e5 Q8 + head) and `GenerativeTier` (Gemma, k = 2 shots from the config, prefix primed). It routes the 128
val items, and the result is compared with `policies.route` under the same config.

| | Result |
|---|---|
| Decisions (label + tier where routing ended) | **128 / 128 agree** |
| Max confidence gap, runtime vs simulation | Tier 3 1.6e-7 (f32 head), Tier 4 3.0e-8, Tier 1 0 |
| Accuracy | 0.984 runtime = 0.984 simulation |
| Wall time per request (dev, boost on) | mean 448 ms (simulated 478), p50 598, p95 969 |
| By the tier where routing ended | Tier 1: 9 items, 0.005 ms · Tier 3: 46, 5.0 ms · Tier 4: 73, 783 ms |

So the offline simulation is a faithful model of the runtime on this config. Requests that stay below Tier 4 cost
about 5 ms against ~800 ms for R0, far inside ρ = 0.1 (D6). These are dev timings; E1 measures under protocol v3.

Re-checked 2026-10-21 after the label-generic runtime (ADR-019). `route-items` now routes in the index space of the
config's label list (`models.tier4.labels`), and the rules reach it through `MapLabels`. The config gained that field
(hash 31171dc6…). Result: **128 / 128 agree**, the same confidence gaps, accuracy 0.984. Wall time as found (boost 1,
AC 1): mean 327 ms, p50 434, p95 723; Tier 3 ends 3.8 ms, Tier 4 ends 571 ms, faster than on 2026-10-18 under the same
nominal settings. That difference is why dev timings are not E1. `runtime_check_intents_v0.json` now holds this run.

## CLINC150 dev: tier outputs and policies (2026-10-18, ADR-018)

`python experiments/router/tier_outputs.py --dataset clinc150 --shots 1 --prompt tier4_clinc150_desc_prompt.txt
--n-ctx 8192` (~60 min of full CPU, boost on) → `tier_outputs_clinc150.jsonl.gz`. The plain file is 21 MB and stays
local; gzip -9 brings it to 7.9 MB, and `policies.py` reads either form. All 3200 cal + val items; runtime e5 vectors
equal the cache (gap 0.0); 0 Tier-4 errors; Tier 4 ~1.0 s per item. Then `python experiments/router/policies.py
--dataset clinc150` → `policies_clinc150.json`.

The cascade is Tier 3 → Tier 4: the Kestrel rules cannot answer with CLINC150 labels. The prefix-sum search
(`best_per_tier`) makes the 1700 × 1700 threshold grid exact and fast.

| Val, n = 1700 (200 OOS) | Result |
|---|---|
| Tier 3 (e5 head), in scope | acc 0.960, ECE 0.054 → 0.010 (T = 0.67), 5.6 ms |
| Tier 4 (Gemma, descriptions), in scope | acc 0.925, ECE 0.032 → 0.012 (T = 1.26), 988 ms |
| R0 always Gemma | **0.844** (1434), 988 ms |
| R1 Tier 3 alone, always answers | **0.847** (1440), 6 ms |
| R3 + conjunction signal, δ = 0 (rule before ADR-019) | 0.847 at 59 ms: τ3 = 0, so everything is accepted at Tier 3 except the 4.6% the signal sends to Tier 4. R-rand 0.846; cross-fit 0.846 |
| Selective risk at coverage 0.80 (D4), errors of 1360 answered | R1 34 · R2 30 · **R2c 20** · R3 11 (no signal) / 15 (signal) |

Readings (dev; not results):
- **On CLINC150 the always-SLM baseline is no stronger than the encoder alone.** Gemma is 3.5 pp below the head in scope
  and rarely answers `oos`. So the rule then pre-registered (cheapest point with accuracy ≥ R0 − δ) picks a degenerate
  cascade, Tier 3 only, and H1 would hold trivially. The signal's 78 false positives are its only Tier-4 traffic.
- **H2 direction on CLINC150:** with one shared τ, calibration lowers selective risk at 80% coverage from 30 to 20
  errors (R2 → R2c). Per-tier τ (R3) brings it to 11. On Intents-v0 (n = 128) the two were 2 items apart.
- **Exploratory, beyond the pre-registered rule: Tier 4 as a second opinion that abstains.** With τ4 > 0 the cascade
  declines OOS requests that both tiers are unsure of. Points chosen on val, cross-fit in brackets:

| Exploratory point (val) | Accuracy | Mean cost | Tier-4 rate | Coverage |
|---|---|---|---|---|
| R0 with its own tuned abstention τ4 | 0.872 | 988 ms | 1.00 | — |
| R1 with its own tuned abstention τ3 | 0.912 | 6 ms | 0 | — |
| R3 + signal, accuracy ≥ 0.90 | 0.900 (0.901) | 111 ms (114) | 0.11 | 0.945 |
| R3 + signal, accuracy ≥ 0.93 | 0.930 (0.931) | 175 ms (179) | 0.18 | 0.905 |
| R3 without the signal, accuracy ≥ 0.93 | 0.930 (0.930) | 119 ms (121) | 0.13 | 0.908 |

  The cascade beats both single models in accuracy at about a fifth of R0's cost, and cross-fitting barely moves it
  (n = 1700). These points come from rules the PREREG does not contain. They stay exploratory (ADR-019 chose
  a different rule; see below).

### Decided 2026-10-21 (ADR-019): baselines that abstain; operating point against the best single baseline

R0 and R1 each get one abstention threshold chosen on val for accuracy (Ch. 35 equal effort; abstaining is right on
OOS, D3). A cascade's operating point is the cheapest with accuracy ≥ max(tuned R0, tuned R1) − δ, and H1c tests R3
against the tuned R1. `python experiments/router/policies.py --dataset clinc150` (reads the .gz) now gives:

| Val, n = 1700 | Accuracy | Mean cost | Tier-4 rate | Coverage |
|---|---|---|---|---|
| R0 tuned (τ4 = 0.580) | 0.872 | 988 ms | 1.00 | 0.908 |
| R1 tuned (the reference) | **0.912** | 6 ms | 0 | 0.856 |
| **R3 + signal, δ = 0** | **0.912** (cross-fit 0.914) | **140 ms** (141) | 0.14 | 0.928 |
| R-rand at R3's escalation counts | 0.791 | 151 ms | | |
| R3 without the signal, δ = 0 | 0.912 (0.912) | 86 ms (88) | 0.09 | |
| R3 + signal, δ = 0.02 | 0.892 (0.888) | 104 ms (101) | 0.10 | |
| R2 / R2c + signal (Tier 4 always answers, ADR-017) | no point until δ = 0.03 (0.882 at 224 / 207 ms) | | | |

Readings (dev; not results):
- The chosen point is no longer degenerate: τ3 = 0.601, τ4 = 0.814, so Tier 4 is a second opinion that may abstain.
  It ties the tuned encoder alone in accuracy at 23× its cost, and beats tuned R0 by 4 pp at a seventh of its cost.
  On CLINC150 the case for the SLM tier is therefore H1c (non-inferior to R1) plus the frontier above R1
  (0.93 at ~175 ms, exploratory), not H1 alone.
- R3 beats random escalation at matched cost by 12 pp (H1b's direction).
- The signal costs 54 ms here (140 vs 86 ms), all of it the 78 false positives sent to Tier 4.
- The cascade cannot abstain at Tier 3 without paying for Tier 4, while R1 abstains for free. A Tier-3 reject band
  would be a new design (not decided).
- Intents-v0 is unchanged (same R3 points; the regenerated config differs only in `chosen_on.reference` and the hash).

### The Rust cascade on CLINC150 (2026-10-21, label-generic runtime)

`python experiments/router/make_config.py --dataset clinc150` → `experiments/configs/router_clinc150.json` (tiers
[3, 4], τ3 0.601, τ4 0.814, signal on, 150 shots, n_ctx 8192, labels `clinc150_labels.txt`, hash 66c56882…). Then
`python experiments/router/check_runtime.py --dataset clinc150` → `runtime_check_clinc150.json` (~6 min, boost 1, AC 1).

| | Result |
|---|---|
| Decisions (label + tier where routing ended) | **1700 / 1700 agree** |
| Max confidence gap, runtime vs simulation | Tier 3 2.3e-6, Tier 4 4.4e-8 |
| Accuracy | 0.912 runtime = 0.912 simulation |
| Wall time per request (dev, as found) | mean 111 ms (simulated 140), p50 4.5, p95 817 |
| By the tier where routing ended | Tier 3: 1462 items, 4.6 ms · Tier 4: 238, 764 ms |

So E1 can run end to end on CLINC150, the primary dataset (D8), with the same binary and code path as Intents-v0.
