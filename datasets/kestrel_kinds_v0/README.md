# Kestrel-Kinds-v0 (DRAFT)

Which skill a request asks for, for v3's decision layer (ADR-037): `app` (the existing assistant: tasks, notes,
memory, small talk, refusals), `chat`, `email` (a draft to someone; sending stays `app`), `rewrite`, `translate`.
Labels: `datasets/contracts/kinds.txt`. Drafted by Claude on 2026-10-26, **not yet reviewed by the human**.

| Part | Where |
|---|---|
| 48 families of the four new kinds and 10 `app` boundary families, each in English and Romanized Hinglish | `kinds.toml` |
| Kestrel-Intents-v0's train / cal / val items, all `app` (its test split is never read) | borrowed at build time |
| train / cal / val / test, split whole by family, and a manifest with every source's SHA-256 | `splits/` |

Build: `python experiments/data/kinds_v0.py` (`--check` validates only); the output is the same bytes every time.
Head (dev only, test closed): `python experiments/tier3/train.py --datasets kinds_v0 --encoders
multilingual-e5-small-q8_0 --out experiments/logs/v3/kinds/dev_v0.json`.

Open: may `chat` answer timeless explanations (the `concepts` families) in v3, or do they stay `app` as
Intents-v0's review decision 2 keeps general knowledge? Live or current facts and calculations stay `app`.
