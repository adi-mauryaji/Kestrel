# E5 logs (head updates; ADR-027, PREREG §4 E5 and §5 ε)

Dev only (CLINC150 train → val, in-scope, e5 Q8_0 cache). Not test results: the test run (Week 14) uses the selected
configurations unchanged.

## ε pilot (2026-10-23) — `epsilon_pilot.json`

`python experiments/continual/epsilon_pilot.py` (260 s). The Tier-3 head retrained with C = 100 on 5 bootstrap resamples of
train, T on cal, val in-scope: macro-F1 0.956 ± 0.0024, ECE 0.0121 ± 0.0015 (sample SD). **ε₁ = 0.0048, ε₃ = 0.0029**
(2 × SD, the rule committed before the run, 535e953).

## Sweep (2026-10-23) — `sweep_dev.json`

`python experiments/continual/e5_sweep.py --jobs 5`: 570 runs (12 configurations per method × 3 domain orders × 5 seeds +
2 controls), 419 s wall on 5 processes, AC on. Every R matrix is in the log. No configuration was refused as unstable.

| Method (selected configuration) | Avg accuracy | Avg forgetting | BWT | Buffer | s / run |
|---|---|---|---|---|---|
| naive (lr 10, 10 epochs) | 0.190 ± 0.006 | 0.952 ± 0.008 | −0.952 | — | 3.5 |
| replay (50 per class, lr 3, 10 epochs) | **0.926 ± 0.004** | **0.001 ± 0.008** | +0.007 | 9.3 MB | 7.2 |
| EWC (λ 100, lr 3, 10 epochs) | 0.184 ± 0.004 | 0.936 ± 0.008 | −0.936 | — | 6.6 |
| joint (upper bound; naive's lr / epochs) | 0.933 ± 0.005 | 0.014 ± 0.006 | −0.011 | all data | 7.8 |
| stage0 (lower bound) | 0.188 ± 0.006 | 0.000 | 0.000 | — | 0.6 |

Mean ± SD over the 15 order × seed runs. **ε₂ = 2 × SD of the selected naive configuration's forgetting = 0.0159.**

What the numbers say (dev):

- **Naive forgets almost everything.** Each stage brings 30 new classes; after it, the earlier stages' items are predicted
  as new classes (last rows like `[0, 0, 0, 0, 0.88]`). All 12 naive configurations land between 0.12 and 0.19 average
  accuracy, i.e. near "only the last stage", so selecting the grid's corner (largest lr and epochs) changes nothing material.
- **EWC does not help here at any λ in the grid.** For every lr × epochs, λ = 10, 100 and 300 give naive's numbers to three
  decimals (largest |ΔR| between naive and EWC at λ 300: 0.0033, one item in 300); the penalty is applied (the toy test
  shows it acting), it is just negligible. The empirical Fisher is small (max ≈ 5e-4 per stage), and SGD's stability bound
  lr × λ × max F < 2 caps λ near 330 at lr 3. Parameter-regularisation methods are reported weak in class-incremental
  settings; that literature is not yet in docs/literature_matrix.csv (only kirkpatrick2017ewc) and should be added before
  this is written up.
- **Replay works, and its cost is the buffer.** At lr 3 / 10 epochs: 5 per class → 0.378 (0.9 MB), 20 → 0.884 (3.7 MB),
  50 → 0.926 (9.3 MB), against joint's 0.933. The selected configuration is the grid's corner and the trend continues past it
  (100 per class is all the data, i.e. joint). Replay also needs training: with a small buffer and little training it keeps
  the probe's recency bias (5 per class, lr 1 / 5 epochs: 0.153, forgetting 0.850, old stages near 0); with 50 per class the
  same training gives 0.761 (forgetting 0.093) against 0.926 at lr 3 / 10 epochs.
- Replay at 50 per class stores half of every earlier class's training items: a privacy cost under Ch. 23.1 ("store
  (text hash or short text, label) … bounded buffer"), to be stated next to the accuracy.

## Forgetting or interference? (2026-10-23, Week 12) — `confusion_dev.json`

`python experiments/continual/e5_confusion.py --jobs 5` (81 s): the four selected configurations rerun with the same
orders and seeds, recording where each stage's items go after every stage (Ch. 22.4). The stage sets are fixed across
stages, so (a) a test shift is excluded by design; joint and stage0 are the bounds (b); this is (c), label-space
interference. Shares of the old stages' items, mean over old stages and 15 runs:

| Method | After the last stage: correct / into newer classes / other wrong | After the next stage |
|---|---|---|
| naive | 0.003 / **0.997** / 0.000 | 0.011 / **0.989** / 0.000 |
| EWC | 0.000 / **1.000** / 0.000 | 0.001 / **0.999** / 0.000 |
| replay (50 per class) | 0.936 / 0.013 / 0.051 | 0.926 / 0.011 / 0.062 |
| joint | 0.938 / 0.014 / 0.048 | 0.948 / 0.003 / 0.050 |

Naive's and EWC's "forgetting" is interference: one update later, nearly every old item is predicted as one of the newest
stage's classes, and almost none drift to another old class. Their old logits lose to the new classes', which says nothing
yet about whether the old-class information is gone from the weights (a task-masked accuracy, argmax within a stage's own
classes, would tell; not run). Replay's remaining errors have joint's shape (~1% into newer classes, ~5% ordinary
confusions), so it removes the interference rather than trading it for another error.
