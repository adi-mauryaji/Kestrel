# Kestrel-Kinds-v0 (frozen)

Which skill a request asks for, for v3's decision layer (ADR-037): `app` (the existing assistant: tasks, notes,
memory, small talk, refusals), `chat`, `email` (a draft to someone; sending stays `app`), `rewrite`, `translate`.
Labels: `datasets/contracts/kinds.txt`. Drafted by Claude on 2026-10-26; **reviewed by the human** on 2026-10-26
and 2026-10-27, all 113 families OK (`experiments/logs/v3/kinds/review.json`), and frozen (tag `kinds-v0-frozen`).

| Part | Where |
|---|---|
| 80 families of the four new kinds (20 each) and 15 `app` boundary families, each in English and Romanized Hinglish | `kinds.toml` |
| Third draft: 8 `chat`, 6 `email` and 4 `app` boundary families written after dev_v2's val errors, pinned to train (`split = "train"`) so val and test stay unchanged | `kinds.toml` |
| Kestrel-Intents-v0's train / cal / val items, all `app` (its test split is never read) | borrowed at build time |
| train / cal / val / test, split whole by family, and a manifest with every source's SHA-256 | `splits/` |

Build: `python experiments/data/kinds_v0.py` (`--check` validates only); the output is the same bytes every time.
Head (dev only, test closed): `python experiments/tier3/train.py --datasets kinds_v0 --encoders
multilingual-e5-small-q8_0 --out experiments/logs/v3/kinds/dev_v0.json`. dev_v2 → dev_v4 share val and test;
dev_v3 is dev_v2 with `--class-weight balanced` (worse on val; not used). dev_v4's val is optimistic: the third draft
was written after looking at val's errors. The closed test split is the measure.

Review: `python experiments/v3/kinds_review.py` writes `experiments/human/kinds_review.html` (its own browser storage
and file, `kestrel-kinds-review.json`); `python experiments/v3/kinds_review.py ingest FILE` merges the marks into
`experiments/logs/v3/kinds/review.json`.

`chat` may explain timeless concepts in v3, with a note that an offline small model wrote it (ADR-038); current
facts, live data and calculations stay `app`, and Kestrel-Intents-v0 keeps its own labels.
