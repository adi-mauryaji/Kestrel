# Kestrel-Kinds-v1 (frozen)

Which skill a request asks for, for v3's decision layer: Kestrel-Kinds-v0's kinds (`app`, `chat`, `email`,
`rewrite`, `translate`; ADR-037) and `notes`, a question the user's own notes answer (ADR-041). Labels:
`datasets/contracts/kinds_v1.txt`. Drafted by Claude on 2026-10-27; **reviewed by the human** the same day, all 34
families OK (`experiments/logs/v3/notes/review.json`), and frozen (tag `kinds-v1-frozen`).

| Part | Where |
|---|---|
| Kestrel-Kinds-v0 (frozen, tag `kinds-v0-frozen`): every row keeps the split it was frozen with | `../kestrel_kinds_v0/` |
| 24 `notes` families (who / when / where / which / how much, about the user's life, with and without the word "notes"), 6 `app` boundary families (finding, summarizing or opening notes; what Kestrel remembers; world facts and live prices) and 4 `chat` ones (the same topics in general), each in English and Romanized Hinglish | `notes.toml` |
| notes-15..24, app-v1-05..06 and chat-v1-03..04, written after the first dev head's val errors, pinned to train (`split = "train"`) so cal / val / test stay as they were | `notes.toml` |
| train / cal / val / test (the new families split whole by kind) and a manifest with every source's SHA-256 | `splits/` |

Build: `python experiments/data/kinds_v1.py` (`--check` validates only); the output is the same bytes every time.
Dev heads: `python experiments/tier3/train.py --datasets kinds_v1 --encoders multilingual-e5-small-q8_0 --out
experiments/logs/v3/kinds/dev_v<n>_kinds_v1.json`; dev_v2's val is optimistic (the pinned families were written after
looking at val's errors), and val holds one `notes` family only. The test split is run once, with the final head.

Review: `python experiments/v3/notes_review.py` writes `experiments/human/notes_review.html` (its own browser storage
and file, `kestrel-notes-review.json`); `python experiments/v3/notes_review.py ingest FILE` merges the marks.

A question about the world (who leads a country, a match, a price today) stays `app` (unsupported), as in Kinds-v0;
the same question about the user's own people, plans or things is `notes`.
