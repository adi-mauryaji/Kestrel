# Kestrel-Intents-v0 (dataset card)

**Status: v0 frozen 2026-10-11** (templates reviewed by the human; κ not yet measured). Synthetic: label it as synthetic in every table (Ch. 31.2).
Intent routing on Kestrel's own 9-intent ontology (`docs/intent_ontology.md`, ADR-010), in English and
Romanized Hinglish, including compound and out-of-scope requests.

## Files

| File | What |
|---|---|
| `templates.toml` | 196 families: 20 per action/chitchat intent, 24 out-of-scope across 12 domains, 12 compound |
| `fillers.toml` | slot values per language (`{topic}`, `{task}`, `{when}`, …) |
| `../contracts/intents.txt` | the label list (= core `Intent::ALL`) |

Build: `python experiments/data/intents_v0.py --out DIR` writes `{train,cal,val,test}.jsonl` and
`manifest.json` (counts per label × language, SHA-256 of every source file, fingerprints per split).
Validate only: `--check`. The frozen build is in `splits/` (seed 0, 3 fills per template); its
`manifest.json` holds the fingerprints to cite in every experiment manifest. Rebuilding must reproduce them.

## Construction

- A **family** is one request written as English and Hinglish templates. Each template is filled
  with up to 3 distinct filler combinations, chosen by a seeded hash (same output on any machine).
- **Splits** (Ch. 32): 0.6 / 0.1 / 0.1 / 0.2 for train / cal / val / test, assigned per family and
  stratified by intent, so a template and its translation never land in different sets.
  Out-of-scope items are split by **domain**, so cal/val/test hold OOS from domains never seen in
  training. **Compound** items go to val (1/3) and test (2/3) only: they never train the head or
  fit temperature (ADR-010).
- Normalization and fingerprints use `experiments/data/prep.py`, whose `norm_text` matches the Rust
  normalizer (shared cases in `../contracts/normalize_cases.json`).

Row fields: `id, text, label, intents, lang (en | hi-en), family, group, stratum, reviewed`.
`label` joins compound intents with `|`.

## Provenance and caveats

- Templates and fillers were **drafted by Claude (an LLM)** on 2026-10-11 and reviewed and accepted by the
  human the same day (`reviewed` per family). None of the evaluated SLMs (Qwen3, Gemma) drafted any.
- The drafter knew the Tier-1 rule set, and some families deliberately use rule-shaped phrasing
  ("remind me to …") while others are near-misses. **Tier-1 coverage on this set is therefore not an
  unbiased estimate of real-world coverage**; report it as coverage on Kestrel-Intents-v0 only.
- Fillers are shared across splits (names, files, topics). Only templates are held out.
- κ: a second human annotator labels ≥ 20% using `docs/intent_ontology.md`; not done yet.
- License: Kestrel-owned data, same terms as the repository.
