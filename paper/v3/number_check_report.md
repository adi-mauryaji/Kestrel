# Independent number and fact check: Kestrel v3 project report

- **Date:** 2026-10-28
- **Document checked:** `paper/v3/kestrel_v3_report.pdf`, read as a reader sees it (`pdftotext -layout`), with its source `kestrel_v3_report.tex`, the generated tables (`generated/table_*.tex`, as rendered), Figure 1 (`experiments/logs/v3/acceptance/page_headed.png`), Figure 2 (`figures/arch.tex`) and the figure images `figures/kinds.png`, `figures/memory.png` and `figures/code.png`. `code.png` is not used in the report, but it agrees with the logs.
- **Scope:** every number in the text, in Table 1 (headline), in Tables 2 to 5 and in the figures. Also every quoted request and answer, every configuration constant, every description of how the system works and how it was built (dates, decision numbers, who decided, review status), the install commands, and the 12 known limitations.
- **Method:** I derived each value myself from the raw logs before opening the generator, using Python with exact fractions, rounding half up, and plain medians:
  - kinds: `experiments/logs/v3/kinds/*.json`
  - notes: per-query and per-question rows in `experiments/logs/v3/notes/*.json`. A note counts as "found" when the closest note is the query's note and its score reaches θ. Hinglish answers were judged with `experiments/v3/writer_lang.py` `is_hinglish`.
  - code: per-request rows in `experiments/logs/v3/code/*.json`, with seconds summed from each attempt's `ms`
  - memory and start-up: `mmap/*.jsonl` medians and `worker/*.tsv` medians
  - acceptance: `acceptance/socket.json`, `page_*.json` and `README.md`
  - throughput: the three E0 CSVs. I took the mean of each session's measured rounds, then the mean over the three sessions.
  - code and setup facts: `apps/cli/src/*.rs`, `scripts/*.sh`, `README.md` and `experiments/v3/*.py`
  - decisions: the vault's `DECISIONS.md` (ADR-036..048), `CHANGELOG.md` and `ROADMAP.md`, read only
  - machine: the "Machine:" line of `experiments/PREREG.md`

  I opened `experiments/analysis/paper_assets_v3.py` and `generated/numbers.tex` only afterwards, and only to explain the two rounding findings.
- **Conventions applied:** shares are k/n. Percentages are rounded half up to one decimal from the exact fraction. Other decimals are rounded half up. Medians are plain medians.

## Result

| Status | Rows | Distinct items |
|---|---|---|
| OK | 188 | — |
| MISMATCH | 3 | 3 |
| ROUNDING | 2 | 2 |
| UNSUPPORTED | 1 | 1 |
| WORDING | 14 | 13 (rows 2 and 22 are one item) |
| **Rows checked** | **208** | **19 non-OK items** |

Every number in Table 1, Tables 2 to 5, Figure 3 and Figure 4 matches the raw logs. The two exceptions are both roundings: "65%" and "5.0 s". Every quoted answer matches `socket.json`, except for one dropped list dash.

## Non-OK items and recommended fixes

1. **MISMATCH (§3, notes by meaning, quoted answer).** The report prints `found 1 note: 2026-09-08-mom-birthday.md (by meaning)`. `socket.json` result 12 has `found 1 note:\n- 2026-09-08-mom-birthday.md (by meaning)`, so the list dash `- ` was dropped. That is more than a LaTeX line break. **Fix:** `\ans{found 1 note: - 2026-09-08-mom-birthday.md (by meaning)}`.
2. **MISMATCH (Figure 2, "task command?" box).** The box lists "list, done, rename, reschedule, delete". Delete is not a task command. `apps/cli/src/commands.rs` says, at line 13 and lines 152–153: "Anything that says delete / remove / hata … stays the cascade's … delete asks y/N". The comment in `assistant.rs` reads "Task commands (list, done, rename, reschedule)". A delete request goes through the kind head to the app path. **Fix:** change the box to "list, done, rename, reschedule (and a time)" and drop "delete". Deleting is the app path, which asks y/N.
3. **MISMATCH (Appendix B).** The appendix says "`paper/v3/number_check.md` records an independent check of every number", but that file does not exist. This check is `paper/v3/number_check_report.md`. **Fix:** change the path to `paper/v3/number_check_report.md`.
4. **ROUNDING (§4.5, "65% less").** The exact value is (3247 − 1144) / 3247 = 64.77%, which is **64.8%** under the report's one-decimal convention. From the unrounded medians, (3247.05 − 1143.71) / 3247.05 = 64.78%. The generator calls `pct(copy - mapped, copy, 0)`, which rounds to a whole percent. **Fix:** print "64.8% less", or state that this one figure is rounded to a whole percent.
5. **ROUNDING (§6.3, "an answer took 5.0 s on average").** The mean of the 25 per-question `s` values in `qa_test.json` is 4.9472 s, which rounds to **4.9 s**. The 5.0 comes from rounding twice: the log's summary `answer_s.mean` = 4.95 is already rounded to two decimals, and the generator rounds it again with `fmt(q['answer_s']['mean'], 1)`. **Fix:** print "4.9 s" (or "about 5 s"). The rows are themselves rounded to 0.01 s, so the true mean lies in [4.945, 4.952].
6. **UNSUPPORTED (§6.1, "5 skill requests were sent the old way (and refused)").** `fallback_test_v2.json` records only the routing decision ("take app"). No log shows what the app path then answered. For example, "payment pending hai, uska reminder mail bana do" may well become a reminder task rather than a refusal. **Fix:** drop "(and refused)", or write "(where the old path refuses them or takes them for something else)".
7. **WORDING (§1 "nine app actions … and refused everything else"; §2 "outside its nine actions … got I can't do that").** `datasets/contracts/intents.txt` has nine intents, and one of them is `unsupported`, which is the refusal itself. That leaves eight actions. ADR-037 also says such requests ended "as `unsupported` or in a wrong intent". **Fix:** "sorted requests into nine intents: eight app actions (…) and *unsupported*, which it refused" and "outside its eight actions … usually got *I can't do that*".
8. **WORDING (§1 bullet "writing, fixing and explaining small Python programs, checked in a sealed sandbox").** Explaining code goes to *chat* and is never checked in the sandbox. Sources: rule 5 of the Kinds-v2 fallback prompt, ADR-044 (2), and socket case 30 ("code: explain is chat"), whose answer ends with the chat caveat. **Fix:** "writing and fixing small Python programs, checked in a sealed sandbox, and explaining code".
9. **WORDING (Table 1 and §6.4, "explanations 6/6"; "the three English requests to explain code were answered in Hinglish").** In `experiments/v3/code_eval.py`, explain items are answered with the harness's own `EXPLAIN_SYSTEM` prompt. In the service, an explain request is answered by *chat*, with `Kind::Chat`'s prompt and caveat, and that path was not measured on CodeTasks. **Fix:** add a clause, for example "(explanations measured with the evaluation's explain prompt; the service answers them as chat)".
10. **WORDING (§3, "When it finds nothing, the note closest in meaning is added").** In `act.rs` (`WithMeaning`) and in ADR-039, notes found by meaning are added whenever the keyword search finds fewer than min(limit, k = 3) notes, provided the note reaches θ and is not already listed. **Fix:** "When it finds fewer than three notes, the note closest in meaning is added if it is close enough".
11. **WORDING (§3, "although no word of the request is in that note's title").** The note's title is its first line that is not "tags:" (`meaning.rs` `chunks`). For this note that line is "Maa's birthday, 20 September", which contains the request's "maa". **Fix:** "although the keyword search, which needs every word of the query in the note, found nothing".
12. **WORDING (§3, "Answers from the language model appear as they are written").** Answers from notes are not streamed: `notes_qa.rs` says "the answer is not streamed" because it is checked first. **Fix:** add "(answers from the notes appear after their check)".
13. **WORDING (§3, "Translation into Hindi is written in Devanagari script; Hinglish stays in Latin letters").** This is true of the script. However, translation *into* Hinglish mostly returned English: `writer/lang_final.json`, translate-hinglish, 1/4. **Fix (optional):** "Hinglish stays in Latin letters (translation into Hinglish is weak: 1/4 on the writer's dev items)".
14. **WORDING (§4.2 step 3, "Kestrel asks: *do you want me to …?*").** The real questions are of the form "Answer this as a chat reply?", "Write this as an email or message draft?", "Rewrite the text in your request?", "Translate the text in your request?", "Answer this from your notes?" and "Write this as Python code?" (`kinds.rs` `Kind::question`). **Fix:** "Kestrel asks, for example, *Write this as an email or message draft?* (y/N)".
15. **WORDING (§5, "the v3 parts live in the command-line app").** v3 also changed `crates/kestrel-inference`: in the diff from `stage2-frozen` to `86cb3a7`, `llama.rs` gained about 150 lines and `tier.rs` about 28, for the memory-mapped load and the saved prompt state (`LlamaSession::prime_cached`). **Fix:** "…live in the command-line app, with the memory-mapped loading and the saved prompt in the inference crate".
16. **WORDING (§6.2, "Two of the misses found their note just below the threshold; “festival shopping” found the Goa trip note").** Under the report's own definition, "found" means closest *and* at or above θ. These are closest notes below θ, and nothing was added:
    - goa: 0.8283 and 0.8298, against θ = 0.83
    - festival shopping, in both languages: the Goa trip note at 0.8131

    **Fix:** "For two of the misses the right note was the closest, just below the threshold; for “festival shopping” (in both languages) the closest was the Goa trip note, also below the threshold, so nothing was added."
17. **WORDING (Table 4 rows "first harness (dev)" next to the §6.4 text "both right 22/24").** The three first-harness rows use the checks *before* the review: 19, 17 and 23, which equal `strict_before` in `dev_rescored_review.json`. The text quotes the rescored 22 and 22. **Fix:** label these rows "first harness, first checks (dev)", or add a "reviewed checks" column (Gemma 22, Qwen3-1.7B 18, Qwen3-4B 22).
18. **WORDING (Appendix A, ADR-044).** The appendix line omits ADR-044's headline decision: "Qwen3-4B writes and fixes code, Gemma explains". ADR-046 superseded that point. **Fix:** "ADR-044: Qwen3-4B to write and fix code (superseded by ADR-046); the code loop retries only on the sandbox's own errors; one review for the code and Kinds-v2 sets."
19. **WORDING (Appendix B, "Every number in this report is generated from the committed logs … its tests fail if a document uses a number that the logs do not give").** Many numbers are typed into the .tex. Examples: 10 idle minutes; 512 MiB, 10 s, 10 MiB and 64; "at most twice"; "two closest notes"; "thirteen records"; `86cb3a7`; the dates; "(6 per type)"; "three jokes"; "Two of the misses"; "the three English requests". `test_every_number_a_v3_document_uses_is_generated` only checks that every `\kv{}` macro exists. The appendix also lists only `experiments/logs/v3/…`, but the throughput numbers (12.1, 58 and 8.2 tok/s) come from `experiments/logs/E0/` and the machine from `experiments/PREREG.md`. **Fix:** "Every result number (each `\kv` macro) is generated from the committed logs … its tests fail if the document uses a macro the logs do not give; configuration constants and counts in the prose are typed and were checked by hand (`number_check_report.md`). The logs are under `experiments/logs/v3/` (…) and `experiments/logs/E0/`; the machine is from `experiments/PREREG.md`."

## Row-by-row check

Abbreviations:
- `kt1`, `kt2`: `kinds/fallback_test_v1.json`, `kinds/fallback_test_v2.json`
- `fd2`: `kinds/fallback_dev_v2.json`
- `mtt`: `notes/meaning_test_translated.json`
- `md`, `mdt`: `notes/meaning_dev.json`, `notes/meaning_dev_translated.json`
- `qt`, `qd`: `notes/qa_test.json`, `notes/qa_dev.json`
- `ct`: `code/test_gemma-4-e2b.json`
- `sock#N`: `acceptance/socket.json` results[N], 0-based
- `accR`: `acceptance/README.md`

| # | Section | Claim as printed | Value | Source | Independent value | Status |
|---|---|---|---|---|---|---|
| 1 | Title | version tag v3-frozen; 28 October 2026 | v3-frozen | `git rev-parse v3-frozen` → ad920d5, tagged 2026-10-27 17:37 | tag exists, on ad920d5 | OK |
| 2 | §1 | sorted requests into nine app actions … and refused everything else | 9 | `datasets/contracts/intents.txt` | 9 intents, 1 of them `unsupported` (8 actions) | WORDING |
| 3 | §1 | 6-core laptop with 14.95 GiB | 6; 14.95 | PREREG "Machine:" line | Ryzen 5 5500U (6C/12T), 14.95 GiB | OK |
| 4 | §1 | two models already used in v1: small multilingual encoder, 4-bit Gemma-4-E2B | — | PREREG lines 52–53; `configs/router_intents_v0.json` | e5-small Q8_0; gemma-4-E2B-it-Q4_0 | OK |
| 5 | §1 | writing, fixing and explaining small Python programs, checked in a sealed sandbox | — | rule 5 of `configs/kinds_fallback_v2.json`; ADR-044 (2); sock#30 | explaining goes to chat, no sandbox | WORDING |
| 6 | Table 1 | kinds test n = 280: 256/280 right | 256/280 | kt2 all.head_gemma.right / n | 256/280 | OK |
| 7 | Table 1 | on the 24 never-scored requests 23/24 | 23/24 | kt2 fresh.head_gemma; Kinds-v2 manifest test.fresh_items | 23/24; 24 fresh | OK |
| 8 | Table 1 | a skill started that was not asked for: 6 times | 6 | kt2 all.head_gemma.skill_not_asked_for; wrong list | 6 | OK |
| 9 | Table 1 | right note by meaning (test) 14/18 | 14/18 | mtt rows: best note in notes and score ≥ θ 0.83 | 14/18 | OK |
| 10 | Table 1 | wrong note added to a query with no note 0/2 | 0/2 | mtt: none-queries with best ≥ 0.83 | 0/2 (0.8007, 0.7957) | OK |
| 11 | Table 1 | notes Q&A test: 19/19 answered right | 19/19 | qt rows not qa-none, grade right | 19/19 | OK |
| 12 | Table 1 | unanswerable declined 6/6 | 6/6 | qt qa-none rows, answer NOT IN NOTES, grade right | 6/6 | OK |
| 13 | Table 1 | functions, fixes, explanations 6/6, 6/6, 6/6 | 6/6 ×3 | ct results pass_loop by type | 6/6, 6/6, 6/6 | OK |
| 14 | Table 1 | scripts that read input 3/6 | 3/6 | ct results type script | 3/6 | OK |
| 15 | Table 1 | memory not reclaimable 3247 → 1144 MiB | 3247; 1144 | `mmap/…jsonl` medians of anon_kib, copy/map cold, measured rounds | 3247.05; 1143.71 MiB | OK |
| 16 | Table 1 | time to ready 19.1 → 2.0 s | 19.1; 2.0 | `worker/…tsv` medians v2 / hit | 19.13; 2.02 | OK |
| 17 | Table 1 | acceptance 34/36 passed | 34/36 | socket.json results ok | 34 of 36 | OK |
| 18 | Table 1 | page checks 8/8, 8/8 | 8/8; 8/8 | `page_headless.json`, `page_headed.json` steps ok | 8/8; 8/8 | OK |
| 19 | Table 1 | automated tests 448, 510 passed | 448; 510 | accR results table | cargo 448 passed; pytest 510 | OK |
| 20 | §1 | frozen on 27 October 2026 after the acceptance run, 12 known limitations | 27 Oct; 12 | tag date; accR numbered list; ADR-048 | 2026-10-27; 12 items | OK |
| 21 | §1 | scripts that read input fail half the time | 1/2 | ct scripts | 3/6 fail | OK |
| 22 | §2 | a request outside its nine actions … got *I can't do that* | — | intents.txt; ADR-037 context | 8 actions; "unsupported or a wrong intent" | WORDING (same item as row 2) |
| 23 | §2 | goal decided 26 October 2026; list of goals; v4 a bigger machine | 2026-10-26 | ADR-036 | same date, same list; v4 = the Mac or a bigger machine | OK |
| 24 | §2 | no cloud, no paid model, no new download, no change to v1 | — | ADR-036 consequences; ADR-044 rejects a coder-model download | consistent | OK |
| 25 | §2 | 14.95 GiB shared with everyday use | 14.95 | PREREG | 14.95 GiB | OK |
| 26 | §2 | Gemma-4-E2B writes about 12.1 tokens per second | 12.1 | E0 gemma-4-e2b-it-q4_0 tg64, mean of 3 session means | 12.180, 12.011, 12.236 → 12.142 | OK |
| 27 | §2 | reads a prompt at about 58 tokens per second | 58 | E0 gemma q4_0 pp128 | 58.458, 58.685, 57.829 → 58.32 | OK |
| 28 | §3 | examples copied from the acceptance log; scratch copy with sample notes | — | accR "Where" | scratch serve, the 18 fixture notes copied | OK |
| 29 | §3 | kal shaam 6 baje bill bharna yaad dila dena → created task #2: bill bharna (due 2026-10-28 18:00) | — | sock#2 | identical | OK |
| 30 | §3 | move task 2 to friday → task #2 now due 2026-10-30 | — | sock#7 | identical | OK |
| 31 | §3 | Deleting asks first | — | sock#8 asked ['delete task 3?'] | asks | OK |
| 32 | §3 | a timed task pops up as a desktop notification at its time | — | accR timed-reminder row | rang 17:31:02, notification sent | OK |
| 33 | §3 | When it finds nothing, the note closest in meaning is added | — | `act.rs` WithMeaning (fewer than min(limit, k)); ADR-039 | added when fewer than 3 are found | WORDING |
| 34 | §3 | search my notes for maa ke liye tohfa → found 1 note: 2026-09-08-mom-birthday.md (by meaning) | — | sock#12 text | `found 1 note:\n- 2026-09-08-mom-birthday.md (by meaning)` | MISMATCH |
| 35 | §3 | no word of the request is in that note's title | — | `datasets/kestrel_notesq_v0/notes/2026-09-08-mom-birthday.md`; `meaning.rs` chunks (title = first line not "tags:") | title "Maa's birthday, 20 September" contains "Maa" | WORDING |
| 36 | §3 | maa ka birthday kab hai? → Maa ka birthday 20 September hai. (from 2026-09-08-mom-birthday.md) | — | sock#26 | identical (a newline before "(from") | OK |
| 37 | §3 | goa wale hotel ka naam kya hai? → The notes do not say this: so the reply is not available for this information | — | sock#27; `notes_qa.rs` NOT_IN_YOUR_NOTES | identical (no final period in either) | OK |
| 38 | §3 | an answer is shown only if every number and name in it occurs in the notes | — | `notes_qa.rs` answer() (ungrounded → not shown); grounding check_facts | as described | OK |
| 39 | §3 | manager ko … mail likh do → Hinglish draft starting Subject: Friday ki chutti ke liye request | — | sock#20 | identical first line | OK |
| 40 | §3 | make this polite: send me the file now → Could you please send me the file now? | — | sock#22 | identical | OK |
| 41 | §3 | translate to english: aaj bahut garmi hai → It is very hot today | — | sock#24 | identical | OK |
| 42 | §3 | translation into Hindi is written in Devanagari | — | sock#23 'सुप्रभात, कल मिलते हैं' | Devanagari | OK |
| 43 | §3 | Hinglish stays in Latin letters | — | `writer/lang_final.json`; `kinds.rs` translate prompt | Latin script always; into Hinglish 1/4 | WORDING |
| 44 | §3 | every chat answer ends with (Written offline by a small model: it may be wrong.) | — | `kinds.rs` CAVEAT; sock#18, #19, #30, #35 | identical; pushed last | OK |
| 45 | §3 | what's the weather in delhi today? is refused | — | sock#34 | "I can't do that. …" | OK |
| 46 | §3 | fix this function … → fixed function, (I checked it in a safe sandbox: no errors.), (Written offline by a small model: try it before you rely on it.) | — | sock#29; `code.rs` RAN, CAVEAT | identical | OK |
| 47 | §3 | on the page, code appears in a box with a Copy button | — | `page_*.json` steps 6–7 (codebox, "Copied") | as described | OK |
| 48 | §3 | Buy rice … add this to notes asks Save this as a new note, 2026-10-27-buy-rice-dal-and-oil-on-saturday-from.md (10 words)? | 10 words | sock#31, #32 asked; word count | identical; 10 words | OK |
| 49 | §3 | saves the text exactly as typed; never writes over an existing file | — | `notes_save.rs` header, OpenOptions create_new; CHANGELOG b714e3f "saved verbatim" | as described | OK |
| 50 | §3 | kestrel open or the applications menu; kestrel chat and kestrel say | — | `main.rs` USAGE; `install_service.sh` .desktop entry | all exist | OK |
| 51 | §3 | answers from the language model appear as they are written | — | `notes_qa.rs` ("the answer is not streamed") | notes answers are not streamed | WORDING |
| 52 | §4.1 | kestrel serve runs from login and keeps the rules and encoder loaded | — | `install_service.sh` (systemd user unit, WantedBy=default.target); `service.rs` header | as described | OK |
| 53 | §4.1 | worker starts on the first request that needs it; stops after 10 idle minutes | 10 | `main.rs` serve: `let mut idle = 10u64`; `remote.rs` reap_if_idle | 10 min | OK |
| 54 | §4.1 | local socket and a web server on 127.0.0.1 that needs a key | — | `service.rs` socket_path; `web.rs` (401 without the token) | as described | OK |
| 55 | §4.2 | the kinds: app, chat, email, rewrite, translate, notes, code | 7 | `kinds.rs` KINDS | 7 kinds | OK |
| 56 | §4.2 | every request that is not a task command first gets a kind | — | `assistant.rs` handle order: save-note, aliases, task commands, kinds | consistent (save-note comes first, as Figure 2 shows) | OK |
| 57 | §4.2 | probabilities calibrated by temperature scaling on held-out data | — | `final_kinds_v2.json` temperature 0.9307, cross-fitted 5 folds by family | as described | OK |
| 58 | §4.2 | τ = 0.8 | 0.8 | `configs/kinds_fallback_v2.json` tau; fd2 chosen.tau; `service.rs` fb.tau | 0.8 | OK |
| 59 | §4.2 | τg = 0.7 | 0.7 | same config tau_g; fd2 chosen.tau_g | 0.7 | OK |
| 60 | §4.2 | Gemma's choice takes about 523 ms | 523 | fd2 gemma_ms.p50 | 523.457693 → 523 | OK |
| 61 | §4.2 | Kestrel asks: *do you want me to …?* | — | `kinds.rs` Kind::question | "Answer this as a chat reply?" etc. | WORDING |
| 62 | §4.2 | if the head's top kind is app, the request takes the old path | — | `kinds.rs` decide() | top app → Choice::App | OK |
| 63 | §4.2 | thresholds chosen by a rule written before the run: fewest wrong, then fewest questions | — | fd2 chosen.rule; my re-sort of fd2 val_grid by (wrong, asks, lower τ, higher τg); `kind_head.py` docstring | 0.8/0.7 (6 wrong, 4 asked) | OK |
| 64 | Fig 2 | "add this to notes"? is checked first, then task command? | — | `assistant.rs` lines 105–131 | that order | OK |
| 65 | Fig 2 | task command → list, done, rename, reschedule, delete | — | `commands.rs` lines 13, 152–153 | delete is the cascade's, not a command | MISMATCH |
| 66 | Fig 2 | head (e5 + linear, T) ≥ τ; Gemma ≥ τg; else the head decides (app path / ask y/N); app = v1 cascade; chat/email/rewrite/translate streamed; notes = 2 closest → grounded; code ≤ 2 retries | 2; 2 | `kinds.rs` decide_fallback; `notes_qa.rs` N = 2; `code.rs` RETRIES = 2 | as described | OK |
| 67 | §4.3 | chunks = title and one paragraph; vectors cached and recomputed only for changed notes | — | `meaning.rs` chunks(), FNV hash cache | as described | OK |
| 68 | §4.3 | θ = 0.83 | 0.83 | `configs/notes_meaning_v0.json`; mtt theta | 0.83 | OK |
| 69 | §4.3 | the closest note is added | 1 | config `added` = 1; ADR-040 | 1 | OK |
| 70 | §4.3 | a query with common Hindi words goes into English first; "maa ke liye tohfa" became "gift for mother" | — | `meaning.rs` needs_translation; mdt q-mom/hi-en/0 asked | "gift for mother" | OK |
| 71 | §4.3 | two closest notes to Gemma; answer only from them, in the question's language, or exactly NOT IN NOTES; grounding check | 2 | `notes_qa.rs` N, SYSTEM; `grounding.rs` check_facts | as described | OK |
| 72 | §4.4 | bubblewrap: no network, no home folder, read-only system, empty temp folder | — | `sandbox.rs` (--unshare-all, ro-bind /usr, tmpfs /tmp, no /home bind) | as described | OK |
| 73 | §4.4 | 512 MiB, 10 s CPU, 10 MiB files, 64 processes | — | `sandbox.rs` prlimit --as=536870912 --cpu=10 --fsize=10485760 --nproc=64 | 512 MiB, 10 s, 10 MiB, 64 | OK |
| 74 | §4.4 | code is imported; code waiting for input is only compiled | — | `code.rs` check() | import, then compile on EOFError or an input timeout | OK |
| 75 | §4.4 | errors sent back at most twice | 2 | `code.rs` RETRIES | 2 | OK |
| 76 | §4.4 | the answer always says what was checked | — | `code.rs` closing() | the check line is always added | OK |
| 77 | §4.5 | 3247 to 1144 MiB | 3247; 1144 | `mmap/…md`; jsonl medians | 3247.05; 1143.71 | OK |
| 78 | §4.5 | (65% less) | 65% | 2103/3247 | 64.77% → 64.8% | ROUNDING |
| 79 | §4.5 | loading 3656 to 2477 ms from a cold cache | 3656; 2477 | jsonl median load_ms, copy/map cold | 3655.98; 2476.68 | OK |
| 80 | §4.5 | a 1132-token prompt of examples | 1132 | `worker/README.md` | 1132 tokens | OK |
| 81 | §4.5 | ready in 2.0 s instead of 19.1 s (medians of 3 starts) | 2.0; 19.1; 3 | tsv rows hit 2.02, 2.04, 2.00; v2 19.30, 19.13, 18.01 | 2.02; 19.13; n = 3 | OK |
| 82 | Fig 3 (left) | copied 3247; mapped 1144 plus the grey mapped file | 3247; 1144 | `mmap/…md` (file MiB 8 and 2702) | 3247 + 8; 1144 + 2702 | OK |
| 83 | Fig 3 (right) | v2 19.1 s; v3 computed 17.4 s; v3 loaded 2.0 s (median of 3) | — | tsv medians | 19.13; 17.36 (17.30, 17.36, 17.91); 2.02 | OK |
| 84 | §5 | thirteen records, from the goal on 26 October to the freeze on 27 October 2026 | 13 | DECISIONS ADR-036..048 | 13; 2026-10-26 to 2026-10-27 | OK |
| 85 | §5 | the author made each decision, mostly by accepting a stated recommendation | — | each ADR's "Decision (human, …)"; ADR-047 went against the recommendation | consistent | OK |
| 86 | §5 | the v3 parts live in the command-line app | — | `git diff --stat stage2-frozen 86cb3a7 -- crates` | kestrel-inference changed (~287 lines) | WORDING |
| 87 | §5 | llama.cpp; Python evaluation; bubblewrap; one self-contained HTML page; Chrome DevTools | — | `web.rs` PAGE = include_str(page.html), "nothing fetched from elsewhere"; `experiments/v3/page_check.mjs` (CDP) | as described | OK |
| 88 | §5 | no paid service; only models already on the laptop | — | ADR-036; ADR-044 "Rejected: a download" | consistent | OK |
| 89 | §5 | sets in English and Hinglish, families, split by family, test run once | — | manifests (by family; en and hi-en); ADRs | consistent | OK |
| 90 | §5 | most sets reviewed by the author; review waived for the code set and the third kinds set; reviewed by the AI with a separate agent | 3 of 5 | manifests status; ADR-045 | reviewed: Kinds-v1, NotesQ, NotesQA; claude-reviewed: Kinds-v2, CodeTasks | OK |
| 91 | Table 2 | Kinds-v1: 6 kinds, 920 / 115 / 136 / 256, reviewed | — | `kestrel_kinds_v1` manifest | 6 labels; 920/115/136/256; "reviewed" | OK |
| 92 | Table 2 | Kinds-v2: 7 kinds (+ code), 1000 / 123 / 152 / 280 (24 fresh), claude reviewed | — | `kestrel_kinds_v2` manifest | 7; 1000/123/152/280; fresh 24; "claude-reviewed" | OK |
| 93 | Table 2 | NotesQ-v0: 18 fixture notes, dev 22 (4 none) / test 20 (2 none), reviewed | — | manifest; `find notes -name '*.md'` | 18 notes; 22/4; 20/2; "reviewed" | OK |
| 94 | Table 2 | NotesQA-v0: same notes, dev 26 (6 unans.) / test 25 (6 unans.), reviewed | — | manifest (source kestrel_notesq_v0/notes/) | 26/6; 25/6; "reviewed" | OK |
| 95 | Table 2 | CodeTasks-v0: 4 types, dev 24 / test 24 (24 families), claude reviewed | — | manifest (12 + 12 families, 6 per type) | 24/24; 24 families; "claude-reviewed" | OK |
| 96 | §6.1 | 256/280 right (91.4%) | 91.4% | kt2 | 256/280 = 91.43% → 91.4% | OK |
| 97 | §6.1 | 13 requests were asked about | 13 | kt2 all.head_gemma.asked | 13 | OK |
| 98 | §6.1 | 5 skill requests were sent the old way | 5 | kt2 skill_sent_old_way; wrong list | 5 | OK |
| 99 | §6.1 | (and refused) | — | no log of the app path's answers for these items | not in any log | UNSUPPORTED |
| 100 | §6.1 | 6 times a skill was started that the request did not ask for: three jokes and a web search as chat, a time question to notes, a loop fix to rewrite | 6 | kt2 wrong list | chat-07 ×3 and oos-web-01 → chat; app-14 → notes; code-10 → rewrite | OK |
| 101 | §6.1 | English and Hinglish 130/140 and 126/140 | — | kt2 by_lang head_gemma | 130/140; 126/140 | OK |
| 102 | §6.1 | without the second opinion 221/280; many more asked | 221 | kt2 all.head_alone | 221/280; 37 asked against 13 | OK |
| 103 | §6.1 | 256 of these 280 had been used to test the previous kinds set | 256 | kt2 seen.n; Kinds-v1 manifest test items | 256; 256 | OK |
| 104 | §6.1 | new training families written after its mistakes were known | — | ADR-043 (Kinds-v1 test 968764b) → ADR-044 (4) Kinds-v2 | consistent | OK |
| 105 | §6.1 | 23/24 fresh right | 23/24 | kt2 fresh.head_gemma | 23/24 | OK |
| 106 | §6.1 | chat and translation always; code 7/8, email 15/16, notes 12/16, rewrite 8/20, most of the rest asked | — | kt2 by_gold head_gemma | 20/20, 16/16, 7/8, 15/16, 12/16, 8/20; rewrite 10 of 12 others asked | OK |
| 107 | Table 3 | Kinds-v1, head alone: 179/180, 2/16, –, 12/16, 1/12, 3/16, 9/16 | — | kt1 by_gold head_alone | identical | OK |
| 108 | Table 3 | Kinds-v1, head + Gemma: 180/180, 11/16, –, 13/16, 1/12, 4/16, 11/16 | — | kt1 by_gold head_gemma | identical | OK |
| 109 | Table 3 | Kinds-v2, head alone: 181/184, 6/20, 1/8, 12/16, 7/16, 6/20, 8/16 | — | kt2 by_gold head_alone | identical | OK |
| 110 | Table 3 | Kinds-v2, head + Gemma: 178/184, 20/20, 7/8, 15/16, 12/16, 8/20, 16/16 | — | kt2 by_gold head_gemma | identical | OK |
| 111 | Fig 4 | Kinds-v1, head alone: 27 / 17 / 32, n = 76 | — | kt1 by_gold over non-app kinds | 27 right, 17 asked, 32 old way, 0 not asked for; n = 76 | OK |
| 112 | Fig 4 | Kinds-v1, head + Gemma: 40 / 11 / 25, n = 76 | — | same | 40, 11, 25, 0 | OK |
| 113 | Fig 4 | Kinds-v2, head alone: 40 / 35 / 20 (+ a sliver), n = 96 | — | kt2 by_gold over non-app kinds | 40, 35, 20, 1 (code) | OK |
| 114 | Fig 4 | Kinds-v2, head + Gemma: 78 / 12 (+ unlabelled 5 and 1), n = 96 | — | same | 78, 12, 5, 1 | OK |
| 115 | §6.2 | every query in this set misses its note by keywords | — | md, mdt, mtt rows, `keyword` field | empty in all 62 rows | OK |
| 116 | §6.2 | before translation only 1/9 Hinglish dev queries found their note | 1/9 | md rows at that run's θ 0.85 | 1/9 (2/9 at θ 0.83; the run's own θ is used) | OK |
| 117 | §6.2 | with it, 9/9 | 9/9 | mdt rows at θ 0.83 | 9/9 | OK |
| 118 | §6.2 | test: 14/18 (7/9 English, 7/9 Hinglish) | — | mtt rows | 14/18; 7/9; 7/9 | OK |
| 119 | §6.2 | no note added to the 2 queries that have none | 2 | mtt none rows | 0 of 2 | OK |
| 120 | §6.2 | two misses found their note just below the threshold; "festival shopping" found the Goa trip note | — | mtt q-goa (0.8283, 0.8298); q-diwali (goa-trip 0.8131) | closest notes below θ; nothing added | WORDING |
| 121 | §6.3 | test: all 19/19 answered with the right facts; 6/6 got the not-in-notes reply | — | qt rows | 19/19; 6/6 | OK |
| 122 | §6.3 | an answer took 5.0 s on average | 5.0 | mean of qt rows' `s` (25) | 4.9472 → 4.9 (summary 4.95 → 5.0, rounded twice) | ROUNDING |
| 123 | §6.3 | dev: 4/6 unanswerable declined; twice the cake's price in the model's own words | 4/6 | qd qa-none rows | 4/6; qa-none-cake-price en and hi-en | OK |
| 124 | §6.3 | test (before a fix): 4/10 answers to Hinglish questions were Hinglish | 4/10 | qt hi-en rows not NOT IN NOTES, is_hinglish | 4/10 | OK |
| 125 | §6.3 | with the fix, 9/12 on dev | 9/12 | qd hi-en answered rows, is_hinglish; qd answer_language | 9/12 | OK |
| 126 | §6.3 | the test run was before the fix | — | qa_test.json 12:39 vs e12cb18 HINGLISH_NOTE (CHANGELOG; `notes_qa.rs` comment) | before | OK |
| 127 | §6.4 | three models already on the laptop compared on dev | 3 | `code/dev_*.json` | Gemma-4-E2B, Qwen3-1.7B, Qwen3-4B | OK |
| 128 | §6.4 | after review, Gemma and Qwen3-4B both 22/24 | 22; 22 | `dev_rescored_review.json` rescored | 22; 22 | OK |
| 129 | §6.4 | 11.1 s per request against 29.0 s | 11.1; 29.0 | sum of attempts' ms / 24 in dev_gemma and dev_qwen3-4b | 11.141; 28.987 | OK |
| 130 | §6.4 | no second model in memory, so Gemma does code too | — | ADR-046 | consistent | OK |
| 131 | §6.4 | test: 21/24, every function, fix and explanation, 3/6 scripts | — | ct | 21/24; 6, 6, 6; 3/6 | OK |
| 132 | §6.4 | failed scripts: read until input ended (crashed), stopped at the first empty line, reversed the letters | 3 | ct sc-count-lines/en/0, sc-count-lines/hi-en/0, sc-reverse-words/hi-en/0 | `while True: input()`; `iter(input, '')`; `line[::-1]` | OK |
| 133 | §6.4 | a request took 8.9 s on average | 8.9 | ct attempts ms / 24 | 8.922 | OK |
| 134 | §6.4 | the sandbox found no error, so the retry loop never ran | — | ct attempts | 24 attempts for 24 requests | OK |
| 135 | §6.4 | the three English explain requests were answered in Hinglish | 3 | ct ex-*/en/0 `hinglish` | all 3 true | OK |
| 136 | §6.4 | one explanation stated a wrong output and still passed | 1 | ct ex-list-comp/hi-en/0; tasks.toml code | says 0…324; real output [0, 4, 16, 36, 64]; passed | OK |
| 137 | §6.4, Table 1 | explanations 6/6 (as the service's ability) | — | `experiments/v3/code_eval.py` EXPLAIN_SYSTEM vs `kinds.rs` Chat prompt | measured with the harness's prompt; the service uses chat | WORDING |
| 138 | Table 4 | Gemma, first harness: 6/6, 1/6, 6/6, 6/6, 19/24, 11.1 | — | `dev_gemma-4-e2b.json` rows | identical (11.141) | OK |
| 139 | Table 4 | Qwen3-1.7B: 5/6, 1/6, 5/6, 6/6, 17/24, 10.4 | — | `dev_qwen3-1.7b.json` | identical (10.424) | OK |
| 140 | Table 4 | Qwen3-4B: 6/6, 5/6, 6/6, 6/6, 23/24, 29.0 | — | `dev_qwen3-4b.json` | identical (28.987) | OK |
| 141 | Table 4 | Gemma, final harness (dev): 6/6, 5/6, 6/6, 6/6, 23/24, 9.1 | — | `dev_gemma-4-e2b_adr044.json` | identical (9.066) | OK |
| 142 | Table 4 | Gemma, final harness (test): 6/6, 3/6, 6/6, 6/6, 21/24, 8.9 | — | ct | identical (8.922) | OK |
| 143 | Table 4 vs §6.4 | first-harness rows next to "after the checks were reviewed … 22/24" | 19, 17, 23 | `dev_rescored_review.json` strict_before vs rescored | rows use the pre-review checks | WORDING |
| 144 | §6.5 | every goal feature end to end, scratch service, same socket, y/N answered | — | accR | as described | OK |
| 145 | §6.5 | 34/36 requests passed | 34/36 | socket.json | 34/36 | OK |
| 146 | §6.5 | page 8/8 and 8/8; the step list | 8 | page jsons (8 named steps) | 8/8, 8/8; steps match | OK |
| 147 | §6.5 | reminder due 17:31 popped up at 17:31:02 | — | accR | made 17:29:29, rang 17:31:02 | OK |
| 148 | §6.5 | 448 Rust, 510 Python, 6 sandbox, 4 model-worker tests | — | accR | 448; 510; 6/6; 4/4 | OK |
| 149 | §6.5 | "how much is my rent?" refused: both the classifier and Gemma took it for app | — | accR failure 1; sock#25 | head app 0.62 / notes 0.35 < τ; Gemma app; refused | OK |
| 150 | §6.5 | asked as a notes question, the same answer component gets it right | — | qd qa-rent/en/0 | "Your rent is 12000.", grade right | OK |
| 151 | §6.5 | "which preferences have i set?" found nothing: memory search matches stored words | — | accR failure 2; sock#17, #16 | as described | OK |
| 152 | Table 5 | add a task with a time 0.3 | 0.3 | sock#1 s | 0.3 | OK |
| 153 | Table 5 | add a task, Hinglish 4.8 | 4.8 | sock#2 | 4.8 | OK |
| 154 | Table 5 | keyword search 0.3 | 0.3 | sock#10 | 0.3 | OK |
| 155 | Table 5 | search by meaning 0.3 | 0.3 | sock#11 | 0.3 | OK |
| 156 | Table 5 | search by meaning, Hinglish 2.6 | 2.6 | sock#12 | 2.6 | OK |
| 157 | Table 5 | summary of a note 7.9 | 7.9 | sock#13 | 7.9 | OK |
| 158 | Table 5 | chat: a short poem 10.3 | 10.3 | sock#19 | 10.3 | OK |
| 159 | Table 5 | chat: an explanation 23.2 | 23.2 | sock#18 | 23.2 | OK |
| 160 | Table 5 | translate into Hindi 2.8 | 2.8 | sock#23 | 2.8 | OK |
| 161 | Table 5 | translate into English 3.2 | 3.2 | sock#24 | 3.2 | OK |
| 162 | Table 5 | rewrite politely 3.6 | 3.6 | sock#22 | 3.6 | OK |
| 163 | Table 5 | email (Hinglish / English) 8.5 / 10.8 | — | sock#20, #21 | 8.5; 10.8 | OK |
| 164 | Table 5 | answer from notes 9.4 | 9.4 | sock#26 | 9.4 | OK |
| 165 | Table 5 | write a function 14.0 | 14.0 | sock#28 | 14.0 | OK |
| 166 | Table 5 | fix a function 4.3 | 4.3 | sock#29 | 4.3 | OK |
| 167 | Table 5 | a very long essay (cut off) 58.3; "seconds until the whole answer" | 58.3 | sock#35; `experiments/v3/acceptance.py` (t0 → done) | 58.3; whole answer | OK |
| 168 | §7.1 | some notes questions refused ("how much is my rent?"); others answered ("maa ka birthday kab hai?") | — | accR limitation 1 | same | OK |
| 169 | §7.2 | "which preferences have i set?" finds nothing although a preference is stored | — | accR 2 | same | OK |
| 170 | §7.3 | "write a program to solve X" goes to chat, whole program, no sandbox check | — | accR 3 | same | OK |
| 171 | §7.4 | rewrite requests often asked about first | — | accR 4 | same | OK |
| 172 | §7.5 | scripts that read input to its end are Gemma's weak spot | — | accR 5 | same | OK |
| 173 | §7.6 | jokes → chat; "what time is it in London" → notes | — | accR 6 | same | OK |
| 174 | §7.7 | Markdown in chat answers shown as typed | — | accR 7; Figure 1 screenshot | same | OK |
| 175 | §7.8 | notes answers to Hinglish questions are sometimes English | — | accR 8 | same | OK |
| 176 | §7.9 | standard library only; checked by import or compile, never run with input | — | accR 9; `sandbox.rs` `-E -S`; `code.rs` | same | OK |
| 177 | §7.10 | once code did not show on the author's page; not reproduced | — | accR 10 | same | OK |
| 178 | §7.11 | the page can be shared on the wifi over plain HTTP when installed that way (off now) | — | accR 11 ("on until … installed without it"); CHANGELOG top entry "wifi sharing turned off"; installed unit has no --web-lan | off now (accR is older than the change) | OK |
| 179 | §7.12 | no memory of the conversation | — | accR 12 | same | OK |
| 180 | §7 | these 12 were written down at the freeze and not fixed | 12 | accR; ADR-048 | 12; not fixed | OK |
| 181 | §8 | setup follows the README: venv, models fetched and checked, encoder converted, heads trained, release build with llama | — | `README.md` lines 13–17 | same steps | OK |
| 182 | §8 | `python experiments/v3/kind_head.py --dataset kinds_v2` | — | `kind_head.py` argparse choices (kinds_v1, kinds_v2); docstring usage | exists as written | OK |
| 183 | §8 | `scripts/install_service.sh ~/notes` installs the always-on service for a notes folder | — | `scripts/install_service.sh` usage | exists as written | OK |
| 184 | §8 | `scripts/install_reminders.sh` adds the morning reminder | — | `scripts/install_reminders.sh` (timer at 08:00 and after login) | exists | OK |
| 185 | §8 | kestrel open / chat / say "mere tasks btao" / stop (frees all of its memory) | — | `main.rs` USAGE and dispatch; `README.md` | all exist | OK |
| 186 | §8 | installed service runs from its own worktree at commit 86cb3a7 | 86cb3a7 | `git worktree list` (~/Kestrel-v3 86cb3a7, detached); unit WorkingDirectory; ADR-043; CHANGELOG | 86cb3a7 | OK |
| 187 | §9 | next round from the author's misrouted requests: new kinds and code sets, reviewed, fresh test | — | ADR-048 consequences | same | OK |
| 188 | §9 | v4 = a bigger machine | — | ADR-036 | "the Mac or a bigger machine" | OK |
| 189 | §9 | Qwen3-4B writes only 8.2 tokens per second | 8.2 | E0 qwen3-4b-q4_k_m tg64 | 8.238, 8.216, 8.203 → 8.219 | OK |
| 190 | §9 | from the week of 1 November 2026 research continues (personalization, quantization, final statistics); heavy runs never overlap | — | ROADMAP lines 3 and 8; PREREG (E4 personalization, E6/H6 quantization, Holm) | same | OK |
| 191 | App. A | ADR-036: v3's goal on this laptop, fully offline; v4 a bigger machine | 2026-10-26 | DECISIONS ADR-036 | matches | OK |
| 192 | App. A | ADR-037: kind decision before the research cascade; the first kinds data set | — | ADR-037 (Kinds-v0) | matches | OK |
| 193 | App. A | ADR-038: chat answers timeless explanations with a caveat; more families before review | — | ADR-038 | matches | OK |
| 194 | App. A | ADR-039: notes by meaning after the keyword search, in the app only | — | ADR-039 | matches | OK |
| 195 | App. A | ADR-040: Hinglish queries through English; only the closest note | — | ADR-040 | matches | OK |
| 196 | App. A | ADR-041: notes kind; answers only from notes, grounded, "not in your notes" | — | ADR-041 | matches | OK |
| 197 | App. A | ADR-042: after the notes review freeze the notes sets, test once on Kinds-v1, more notes families | — | ADR-042 | matches | OK |
| 198 | App. A | ADR-043: v3 becomes the always-on service, run from its own worktree | — | ADR-043 | matches | OK |
| 199 | App. A | ADR-044: retries only on the sandbox's errors; one review for code and Kinds-v2 | — | ADR-044 title and decision (1)–(4) | omits (1): Qwen3-4B writes code | WORDING |
| 200 | App. A | ADR-045: author waives review; AI reviews with an independent agent | — | ADR-045 | matches | OK |
| 201 | App. A | ADR-046: Gemma writes and fixes code too (one model in the service) | — | ADR-046 | matches | OK |
| 202 | App. A | ADR-047: page may be shared on the wifi (off by default) | — | ADR-047 ("on only when installed with KESTREL_WEB_LAN") | matches | OK |
| 203 | App. A | ADR-048: v3 frozen after the acceptance run (tag v3-frozen) | — | ADR-048 | matches | OK |
| 204 | App. B | every number generated from the committed logs; tests fail on a number the logs do not give; logs under experiments/logs/v3/ | — | .tex (hand-typed constants and counts); `test_paper_assets_v3.py` (checks only `\kv` keys); E0 and PREREG sources | not every number; tests check macros only; E0/PREREG not listed | WORDING |
| 205 | App. B | `paper/v3/number_check.md` records an independent check | — | file system | no such file; this is `number_check_report.md` | MISMATCH |
| 206 | App. B | the v3 logs are committed | — | `git ls-files experiments/logs/v3` | committed (generator and paper are untracked) | OK |
| 207 | Fig 1 | headed Chrome; day strip with the kestrel at the current time; request box with examples; task list; chat answer with caveat; code box with Copy and the sandbox's check below | — | `page_headed.png`; accR | all visible (17:29 marker; check line at the bottom edge) | OK |
| 208 | §6.1, Table 1 | test splits run once, after settings chosen on dev | — | ADR-043/046 and CHANGELOG (each test once); the NotesQA language fix came after its test, as §6.3 says | consistent | OK |

## What could not be checked, or only partly

- **Whether the 5 "sent the old way" Kinds-v2 test requests were refused.** No log records the app path's answer to them (row 99).
- **The exact mean answer time on the NotesQA test.** The per-question times are rounded to 0.01 s, so the true mean is only known to lie in [4.945, 4.952] s. It rounds to 4.9 from the rows; 5.0 is reachable only by rounding twice (row 122).
- **The test counts (448, 510, 6, 4), the reminder times (17:29:29 and 17:31:02) and the "installed binary byte-identical" claim.** These rest on `acceptance/README.md`. No raw test output or notification log is committed.
- **The 523 ms Gemma kind-decision time.** This is the dev run's median and is logged as "dev timing on the laptop as found; not a measurement".
- **The `acceptance/README.md` text of limitation 11.** It is now stale: it says sharing is on. I accepted "(off now)" from the CHANGELOG entry "wifi sharing turned off" and from the installed unit file, which has no `--web-lan`.
- **The claim that v2 answered "write an email to my landlord" with "I can't do that".** No v2 log of that request exists. ADR-037 says such requests ended as `unsupported` or in a wrong intent (row 22).
