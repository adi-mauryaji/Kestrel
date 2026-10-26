# Kestrel-Memory-v0 (frozen 2026-10-26)

Retrieval evaluation for Kestrel's memory (blueprint Ch. 18; Week 10 "R": a retrieval dev set for recall@k;
ADR-026). Synthetic: drafted by Claude on 2026-10-23, reviewed by the human on 2026-10-26 (every unit OK, experiments/logs/human/dataset_review.json; Personas-v0 and Memory-v0 marked at about 2–3 s per card on the first pass, re-read without new marks) and frozen: any change is a new version with a new fingerprint.

## What is in it

| Part | Count | Where |
|---|---|---|
| Memories (one shared store: preferences, facts, summaries, task history; English and Romanized Hinglish values) | 40 | `memory.toml` |
| of which never retrievable: pending model guesses / expired task history | 3 / 1 | |
| Queries (English 44, Hinglish 22) | 66 | `memory.toml`, `build/queries.jsonl` |
| lexical / paraphrase / negative | 35 / 22 / 9 | |

`now` is fixed at **2026-10-23**: every memory's `updated_at` is `age_days` before it and `expires_at` is
`expires_in_days` after it, so recency and expiry have one answer.

## Query types

- **lexical**: shares a retrieval term with every memory it expects ("When is rent due?" → `rent_due: Rent is due on
  the 5th of every month`).
- **paraphrase**: shares **no** term with the memory it expects ("When was my father born?" → `papa ka birthday 2
  june ko hai`). The lexical baseline cannot find these, so its paraphrase recall is 0 **by construction**. They
  measure what a retriever that matches meaning (e.g. Tier 3's embeddings, Ch. 17.4) would add, not the baseline.
- **negative**: nothing approved and unexpired answers it ("What is my blood group?"); ideally nothing comes back.
  Some negatives only have a pending guess as their "answer" ("Does Asha like spicy food?").

A query's `trap` lists memories that must never come back: the three pending model-inferred guesses and the expired
task-history row. The store guarantees it (Ch. 17.2); the evaluation counts leaks anyway.

The labels are checked mechanically, not by eye: `crates/kestrel-memory/tests/memory_v0.rs` takes the baseline's
own query terms (`kestrel_memory::retrieval::terms`: lowercased whole words, possessives folded, English and
Hinglish stopwords removed, **no stemming**) and requires every lexical query to share a term with each expected
memory and every paraphrase to share none. That makes some "paraphrases" mere word forms: "bullet points" vs
`bullets` (q-pref-04), "designers" vs `design` (q-asha-03). They stay paraphrases because the baseline has no
stemming; a stemmer would turn them into lexical hits, which is the point of measuring.

## Splits

The store is one; queries are split. A **group** (a topic: prefs, asha, rao, kestrel, home, family, travel, college,
tasks, summaries, money, health, work) and all its queries go to one split, so tuning on dev never sees questions
about the test groups' memories. Each negative is its own group (`neg-1` … `neg-6`). Topic groups and negative groups
are each ordered by a seeded hash (`prep.seeded_order`, seed 0) and the first half goes to dev:

| Split | Queries | lexical / paraphrase / negative | en / hi-en |
|---|---|---|---|
| dev | 35 | 19 / 12 / 4 | 22 / 13 |
| test | 31 | 16 / 10 / 5 | 22 / 9 |

The test split is not evaluated until the dataset is reviewed and frozen (`recall_eval --split test` refuses a draft).

## How it is scored

`cargo run -p kestrel-memory --example recall_eval` (from the repository root) seeds an in-memory store through the
public write path (`Memory::propose`, `approve`), ranks each query with the deterministic baseline (Ch. 18.1, default
weights 0.6 / 0.25 / 0.15, half-life 30 days) 10 deep, and reports recall@1/3/5 and MRR by type, by language and by
both; negatives as the share with nothing returned and the mean number returned in the top 5; trap leaks.

**Read the results per type.** The overall recall is set by how many paraphrases the draft contains, which is the
author's choice, not a property of real use; the language rows inherit the same mix (dev has 11 English paraphrases
and 1 Hinglish).

## Building

```
python experiments/data/memory_v0.py --check   # validate memory.toml
python experiments/data/memory_v0.py           # write build/ (store.jsonl, queries.jsonl, manifest.json)
```

`build/manifest.json` has the counts per split and the SHA-256 of `memory.toml`, `store.jsonl` and `queries.jsonl`.

## Review checklist (for the human)

- Are the memories plausible for one user, and is anything in them sensitive enough to drop (Ch. 17.1)?
- Hinglish: are the values and queries natural Romanized Hinglish?
- Do the expected memories answer their queries, and is any other memory an equally good answer (then add it to
  `expect`)? In particular `q-task-03` (expects the week-42 summary, the task row has expired) and `q-rao-03`.
- Negatives: is there really no stored answer?
- Type mix: 35 / 22 / 9 lexical / paraphrase / negative. More paraphrases make the baseline look worse, fewer better.
