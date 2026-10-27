# Kestrel-NotesQA-v0 (DRAFT)

Questions a notes folder answers, or does not, for v3's notes Q&A (ADR-041). Drafted by Claude on 2026-10-27,
**not yet reviewed by the human**.

| Part | Where |
|---|---|
| The notes: Kestrel-NotesQ-v0's 18 fixture notes (not the human's notes) | `../kestrel_notesq_v0/notes/` |
| 24 families: the note that answers the question and the facts a right answer holds (each fact in any of its ways), or none (6 families the notes do not answer: the right answer is that they do not); one English and one Romanized Hinglish question each | `questions.toml` |
| dev / test, split whole by family (answered and unanswered in both), and a manifest with the sources' SHA-256 | `splits/` |

Build: `python experiments/data/notesqa_v0.py` (`--check` validates only); the output is the same bytes every time.
Dev run: `python experiments/v3/notes_qa.py` → `experiments/logs/v3/notes/qa_dev.json`. The test split stays closed
until the review.
