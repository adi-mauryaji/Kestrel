# Kestrel-CodeTasks-v0 (DRAFT)

Small Python requests for v3's code loop (ADR-036): write a function, write a script, fix broken code, explain
code. Drafted by Claude on 2026-10-27, **not yet reviewed by the human**.

| Part | Where |
|---|---|
| 24 families (6 per type), each one English and one Romanized Hinglish request; hidden checks per family: asserts (function, fix), stdin / stdout cases (script), facts a right explanation holds (explain); a reference solution each | `tasks.toml` |
| dev / test, split whole by family, half of each type; a manifest with the source's SHA-256 | `splits/` |

Build: `python experiments/data/codetasks_v0.py` (`--check` validates only); the output is the same bytes every time.
Every reference solution passes its checks and every broken `fix` code fails them, in the sandbox
(`experiments/v3/test_code_eval.py`). Only the standard library. The test split stays closed until the review.
