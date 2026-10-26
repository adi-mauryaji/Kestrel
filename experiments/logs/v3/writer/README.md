# v3 writer: language and script (experiments/v3/writer_lang.py)

23 rewrite / translate requests (experiments/v3/writer_lang_items.jsonl) through the service's own writer
(`kestrel tier4-worker`, greedy Gemma-4-E2B, src/kinds.rs prompts); the check is the answer's script and,
for Latin text, Hinglish vs English (datasets/contracts/hinglish_words.txt). Dev runs on 2026-10-26; the
prompts were changed while looking at these items, so the final score is optimistic.

| tag | what changed | pass |
|---|---|---|
| baseline | the prompts of f257a4b | 14 / 23 (rescored after the word list grew) |
| prompts-a | rewrite and translate system prompts say which language, with an example | 17 |
| prompts-a-note | + a language note after the request (kinds.rs `language_note`) | 19 |
| prompts-a-note2 | Hinglish note: "Translate it into Hindi, and write the Hindi in Latin letters ..." | 19 |
| prompts-a-note3 | Hinglish note with a worked pair ("see you tomorrow" -> "kal milte hain") | 19 |
| final | prompts-a-note's code, rebuilt: the same 23 answers | 19 |

By group (final): Hinglish text rewritten 5/6, English text rewritten 5/5, into Hindi (Devanagari) 4/4,
into Hinglish 1/4 (English comes back unchanged; "roman hindi mein likho" worked), into English 4/4.
