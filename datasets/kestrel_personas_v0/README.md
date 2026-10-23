# Kestrel-Personas-v0 (DRAFT)

Synthetic users who teach Kestrel their own words, for E4 (H4: does correction-based personalization through
structured memory help on a user's held-out requests without hurting the others? Ch. 1, 22.2, 34; ADR-028).
Drafted by Claude on 2026-10-23, **not yet reviewed by the human**. Freezing needs that review; after freezing, any change
is a new version with a new fingerprint. **Label as synthetic in every table.**

## What is in it

| Part | Count | Where |
|---|---|---|
| Personas (4 English, 4 Romanized Hinglish) | 8 | `personas.toml` |
| Aliases per persona, one of each kind but one: document, group of people, note tag, note count, place or date | 5 | `personas.toml` |
| Teaching events (one per alias, in order; K = 1, 3, 5 taught) | 40 | `build/events.jsonl` |
| Personalized requests (per persona: 4 per alias = 3 exact + 1 variant) | 160 | `build/items.jsonl` |
| General requests (per persona: the 10 of its language, no alias) | 80 | `build/items.jsonl` |
| Fixture notes (8 undated documents, 14 dated notes tagged scrum / college / family / gym / thesis / home) | 22 files | `fixtures/notes/` |

Today is fixed at **2026-10-23 (a Friday)**, so "tomorrow", "parso", "on saturday", "somvar ko" have one answer.

## How it is built (the persona simulator)

`python experiments/data/personas_v0.py` expands each persona through shared request patterns: every pattern of each
alias's kind in the persona's language, with `{a}` the alias ("open my standup doc"), `{v}` its variant ("open my standup
docs") and `{t}` what the user means ("open my team sync"). Teaching phrasings rotate over three per language ("when I say
X, I mean Y", "no, by X I mean Y", "actually when I say …"; "jab main X bolun to mera matlab Y hai", "nahi, X se mera
matlab Y hai", "jab main X kahun toh matlab Y").

**Expectations are hand-written templates** (a document read, the N newest notes with a tag, a task title and due date),
filled from `today` and the fixture notes; nothing is computed by the planner. The Rust check
(`apps/cli/src/e4.rs`, tests) then certifies them: every request's canonical form (what the user means, said plainly)
reaches its expectation with the template planner and gold intent, every event is detected as exactly the alias it teaches,
and with all 5 aliases taught, an exact request resolves to its canonical form, a variant does not, and a general request
is left unchanged. A deliberately broken expectation fails the check (verified).

**Variants** are natural forms an exact matcher misses: plurals ("standup docs", "office note"), Hinglish oblique forms
("doston", "gharwalon", "adde pe", "lab walon"), spelling variants ("kantin", "pichhle", "taze"), small rewordings ("my
gang", "the deadline"). One variant contains its alias whole ("lease papers" for "lease"), so resolution mangles it
instead of missing it.

## Splits

Personas are split whole, per language, by a seeded hash (seed 0): **dev p1, p4 (en), p7, p8 (hi-en); test p2, p3 (en),
p5, p6 (hi-en)**: 120 items each (80 personalized + 40 general). The test split is not evaluated until the dataset is
reviewed and frozen (`kestrel e4-eval --split test` refuses a draft).

## What it can and cannot show

- The patterns are inside the planner's reach when spoken canonically (the check above requires it), so "memory, all
  aliases taught" succeeds on exact requests by construction if detection and resolution work: E4 on this set verifies the
  memory pathway and measures how success grows with K, what variants lose, and whether general requests regress. It does
  not estimate how often real users teach aliases or how they phrase them.
- "Off" fails on personalized requests almost by design (an unknown tag, a document name the planner cannot find, a title
  that keeps the literal alias); one alias was left guessable on purpose ("thesis wala doc": the planner reads a dated thesis
  *note* instead of the outline, which is still a failure).
- The teaching phrasings and the alias detector were written by the same author (Claude); the test personas use the same
  phrasings with different aliases. Free-form teaching is out of scope.

## Review checklist (for the human)

- Are the aliases and requests plausible for these personas, and natural in Romanized Hinglish (p5–p8)?
- Are the expected titles what you would want ("riya and kabir ko call karna", "sharma tea stall pe chai peena")?
- Are the variants fair ("adde" for "adda", "gharwalon" for "ghar wale", "kantin" for "canteen")?
- Should some aliases be guessable without memory (like "thesis wala doc"), or none?
