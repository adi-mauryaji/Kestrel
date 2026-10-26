# Kestrel-NotesQ-v0 (DRAFT)

Searches of a notes folder in other words than the notes use, for v3's notes by meaning (ADR-039).
Drafted by Claude on 2026-10-26, **not yet reviewed by the human**.

| Part | Where |
|---|---|
| 18 fixture notes (a student/developer's month: budget, gym, a birthday, a laptop, a trip, a doctor, ...; not the human's notes) | `notes/` |
| 21 families: the notes a search should find (none for the 3 `q-none` families), one English and one Romanized Hinglish query, written as the search tool receives them | `queries.toml` |
| dev / test, split whole by family (found and no-note families in both), and a manifest with the sources' SHA-256 | `splits/` |

Build: `python experiments/data/notesq_v0.py` (`--check` validates only); the output is the same bytes every time.
Dev run: `python experiments/v3/notes_meaning.py [--translate]` → `experiments/logs/v3/notes/`. The test split
stays closed until the review.
