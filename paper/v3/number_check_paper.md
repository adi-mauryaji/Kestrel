# Number and claim check: kestrel_v3.pdf (Kestrel v3 paper)

Date: 2026-10-28. Checked: the PDF as a reader sees it (`pdftotext -layout kestrel_v3.pdf -`, 14 pages): abstract, body,
captions, Tables 1 to 6, Figures 2 to 4 (PNG files in `paper/v3/figures/`), typed configuration constants and the
qualitative claims the logs or the code can settle. Figure 1 (a diagram) was checked against the code.

How: every value was derived again from the raw sources before the generator was looked at. Counts were recounted
row by row from the per-item logs (`experiments/logs/v3/...`), not taken from summary fields where rows exist;
Wilson intervals were recomputed (z = 1.959964) and every percentage and decimal was rounded half up from the exact
value. Medians were recomputed from the raw rows (mmap `.jsonl`, worker `.tsv`, Gemma label latency from
`fallback_outputs_v2.jsonl`). E0 throughput is the mean over sessions 20261011T101743Z, 20261017T132649Z and
20261023T163212Z of each session's mean over measured rounds. Two claims were re-run rather than read: the
rescoring of the first-harness code answers (the reviewed hidden checks of `tasks.toml`, run in the bubblewrap
sandbox through `experiments/v3/code_eval.py`), and the failure modes of the script answers (each logged answer run
on its hidden stdin cases). Language of notes answers was recounted with the word-list check of
`experiments/v3/writer_lang.py` (`datasets/contracts/hinglish_words.txt`). `experiments/analysis/paper_assets_v3.py`,
`paper/v3/generated/numbers.tex` and `numbers.json` were opened only after every derivation was done, and only to
explain the mismatches (a grep for the keys behind items 2, 9 and 10). Paths in the table are relative to
`/home/Adi_mauryaji/Kestrel` unless they start with `KYROS/` (= `/home/Adi_mauryaji/KYROS/KYROS/Projects/kestrel/`).

Result: 214 items checked: 188 OK, 8 MISMATCH, 2 ROUNDING, 3 UNSUPPORTED, 13 WORDING. Every count, share,
interval, table cell and figure label derived from a per-item log matched; the problems are in qualitative
sentences, two roundings, one wrong count of runs and two wrong set counts or ranges.

Build state: the PDF checked was built at 07:56 IST. `paper/v3/generated/*` and `paper/v3/figures/*` were
regenerated at 08:14 to 08:15 IST, while this check ran; the regenerated `numbers.tex` already holds 4.9 (s) and
64.8% for the two ROUNDING items below, so those two need only a rebuild of the PDF. Everything else below is still
open in the `.tex` text.

## Items that are not OK, with the fix

### MISMATCH

1. **Sec. 4, "made validation ECE worse in three of five development runs".** The five Kinds-v0 dev runs
   (`experiments/logs/v3/kinds/dev_v0..dev_v4.json`, `val_raw.ece` vs `val_calibrated.ece`) got worse in four:
   v1 0.0659 to 0.1029, v2 0.0444 to 0.1207, v3 0.0795 to 0.0801, v4 0.0629 to 0.0907; only v0 improved
   (0.0511 to 0.0507). Fix: "made validation ECE worse in four of five development runs".
2. **Sec. 8.1, "On the validation items where the Kinds-v2 head was unsure, Gemma was right 21 times out of 26, the
   head 10 times."** `fallback_dev_v2.json` `val_head_unsure` is computed at the script's own `tau` = 0.7 (top-level
   field), not at the chosen τ = 0.8. Below τ = 0.8 there are 38 validation items (`fallback_outputs_v2.jsonl`, val
   rows, top head probability < 0.8): Gemma right 30, head right 19. Fix: "On the 38 validation items below the chosen
   τ = 0.8, Gemma was right 30 times, the head 19 times" (or keep 21/26 and 10 and say "below 0.7").
3. **Sec. 8.4, "Gemma's misses were scripts (1/6) that printed input prompts or extra lines".** Of the five misses
   (`dev_gemma-4-e2b.json`), three printed a prompt or extra words (sc-celsius en and hi-en, sc-even-odd hi-en), but
   two (sc-sum-lines en and hi-en) crashed: `while True: input()` with only `ValueError` caught raises an uncaught
   EOFError at the end of the case input (re-run on the hidden stdin "3\n4\n10\n": exit 1, `EOFError`). These two are
   also the two that stay wrong after rescoring. Fix: "Gemma's misses were scripts (1/6): three printed input prompts or
   extra words, two crashed at the end of the input (an uncaught end-of-file error)".
4. **Sec. 10, "Every set is small (24 to 280 test items)".** Kestrel-NotesQ-v0's test split has 20 queries
   (`datasets/kestrel_notesq_v0/splits/manifest.json`, `meaning_test_translated.json` `queries_n`). Fix: "(20 to 280
   test items)".
5. **Sec. 1, contribution 6, "An evaluation on six small English–Hinglish sets with one test run each".** Five sets had a
   test run (Kinds-v1, Kinds-v2, NotesQ-v0, NotesQA-v0, CodeTasks-v0; Table 1 lists exactly these five). Kinds-v0 was
   never tested and the 23 writer requests are development only. Fix: "on five small English–Hinglish sets with one
   test run each".
6. **Fig. 2 caption, "The fallback mostly turns questions to the user into right answers."** By the decision rule
   (`apps/cli/src/kinds.rs` `decide_fallback`), the requests asked with the fallback are a subset of those asked by the
   head alone. On Kinds-v1 only 17 − 11 = 6 skill questions were resolved at all, while right decisions rose by 13
   (27 to 40), so at least 7 of the 13 came from requests the head alone sent the old way (32 to 25). On Kinds-v2
   at most 23 of the 38 gained come from questions. Fix: "The fallback turns questions to the user, and requests sent
   the old way, into right answers."
7. **Sec. 9, "the SLM fallback turns most questions into right decisions".** Same subset argument: on the Kinds-v1
   test, 18 questions became 11, so at most 7 of 18 were turned into anything; on Kinds-v2, 37 became 13 (24 of 37).
   Fix: "the SLM fallback resolves most questions on Kinds-v2 (37 to 13 on test), fewer on Kinds-v1 (18 to 11)".
8. **Sec. 9, "Refusing a skill request on the old path is the most common failure".** True on Kinds-v1 (25 sent the
   old way, 0 skills not asked for), but not with the Kinds-v2 head and settings the service uses: 5 sent the old way
   against 6 skills not asked for (`fallback_test_v2.json` `all.head_gemma`). Fix: "Refusing a skill request on the old
   path was the most common failure on Kinds-v1 (25 of 25 wrong decisions) and is still the costliest for the user,
   since nothing tells them that a skill exists; on Kinds-v2 it fell to 5, next to 6 skills not asked for."

### ROUNDING

9. **Sec. 8.3, "An answer took 5.0 s on average".** The 25 per-question times in `qa_test.json` average 4.947 s, which
   rounds to 4.9; 5.0 comes from rounding the logged two-decimal `answer_s.mean` = 4.95 a second time. The unrounded
   mean is somewhere in [4.945, 4.952], so one decimal is not determinable. Fix: "4.9 s on average" as the
   regenerated `numbers.tex` (08:14) now says, from the logged rows; or "4.95 s" / "about 5 s". Rebuild the PDF.
10. **Sec. 8.6, "(2103 MiB, 65%)".** 2103.34 / 3247.05 = 64.78%; the paper's stated convention is one decimal. Fix:
    "(2103 MiB, 64.8%)" (already the value of `mmap.anon.cut` in the regenerated `numbers.tex`; rebuild the PDF).

### UNSUPPORTED

11. **Sec. 9, "The jump in skill requests (40/76 to 78/96) came from data (notes, rewrite and poem families), not from the
    method."** No run separates the data from the other changes between the two runs: τ / τg were chosen again
    (0.7 / 0.9 to 0.8 / 0.7), the Gemma prompt changed (`tier4_kind_prompt_v1.txt` to `_v2.txt`), and 96 includes 20
    fresh skill requests (8 of them code). On the 76 skill requests both test splits share, right decisions went from
    40 to 59 (78 minus the 19 fresh skill requests that were right). Fix: "On the 76 skill requests both test splits
    share, right decisions rose from 40 to 59, with Kinds-v2's notes, rewrite, poem and code families and re-chosen
    thresholds; no run separates the two."
12. **Sec. 7, "Timings come from the laptop as found (AC power, CPU boost on, other software idle)".** The logs record
    AC power, boost on and Kestrel's own service idle (`experiments/logs/v3/mmap/README.md`); the same README records
    load1 1.2 to 2.6, and no log records other software as idle. Fix: "(AC power, CPU boost on, Kestrel's own service
    idle)".
13. **Reproducibility paragraph: "every number in this paper is generated from the committed logs by
    experiments/analysis/paper_assets_v3.py, and paper/v3/number_check.md records an independent re-derivation of
    each".** (a) `paper/v3/number_check.md` does not exist (this record is `paper/v3/number_check_paper.md`);
    (b) `paper_assets_v3.py` and all of `paper/v3/` are untracked (`git status`), so they are not "in the repository at
    tag v3-frozen" (tag v3-frozen = ad920d5 = HEAD); (c) several numbers are typed in the `.tex`, not generated: 512 /
    384 / 640 tokens, the sandbox limits, N = 2, ten idle minutes, "84 to 103", "three of five", the rent head score
    0.62, "6 per type", "median of 3" (Table 6 caption), 15 GiB. Fix: correct the path to `number_check_paper.md`, commit
    the generator and the paper (or name the commit that holds them), and say "every result from the logs is generated
    ...; configuration constants are typed and were checked against the code".

### WORDING

14. **Abstract, "a cached prompt prefix cut its start from 19.1 to 2.0 s".** 19.1 s is the v2 worker (copied weights,
    prefix computed); the v3 worker with mapped weights and the prefix computed took 17.4 s; the cached prefix alone
    took it from 17.4 to 2.0 s (`worker/20261026T142304Z.tsv` medians 19.13 / 17.36 / 2.02). Fix: "and memory-mapping
    with a cached prompt prefix cut the worker's start from 19.1 to 2.0 s".
15. **Abstract, "when it is unsure, the language model scores the kinds, and when that is unsure too, the user is
    asked"; Sec. 1 contribution 1, "below a second threshold the user is asked".** When Gemma is also unsure and the
    head's top kind is app, the request takes the app path without a question (`kinds.rs` `decide`; Sec. 4 says this
    correctly). Fix: "... and when that is unsure too, a request for a skill is put to the user as a question".
16. **Fig. 1 caption, "Saving a note and task commands are checked first, on the text as typed."** Only saving a note
    reads the text as typed; task commands are parsed on the normalized request (chat spellings, joined words, the
    user's aliases resolved: `apps/cli/src/assistant.rs` `respond`, `commands::parse(&req.text, ...)`). Fix: "Saving a
    note is checked first, on the text as typed, then task commands."
17. **Sec. 3, "answers from the SLM are streamed as they are written".** Notes answers are not streamed: they are shown
    only after the grounding check (`apps/cli/src/notes_qa.rs` header: "the caller shows only a grounded answer, so the
    answer is not streamed"). Fix: "answers from the SLM are streamed as they are written, except notes answers, which
    are shown after the grounding check".
18. **Sec. 4, "The kinds were added in three dataset versions (Table 1)".** Table 1 lists two of them (Kinds-v1 and
    Kinds-v2); Kinds-v0 appears only in Sec. 7. Fix: "in three dataset versions (Sec. 7; Table 1 lists the two that were
    tested)".
19. **Sec. 6, "Qwen3-4B's own asserts failed on right code and caused 8 retries".** 8 is right, but only 4 retries came
    from wrong asserts (fn-second-largest en and hi-en, e.g. `assert second_largest([9, 2, 4, 7, 5]) == 5`); the other 4
    came from asserts written where the first harness wanted a sample input, which the script then read as its input and
    crashed on (sc-celsius/en, sc-even-odd/en: `ValueError: could not convert string to float: 'assert ...'`;
    `dev_qwen3-4b.json`). Fix: "Qwen3-4B's own checks (wrong asserts, or asserts where a sample input was asked for)
    failed on right code and caused 8 retries".
20. **Sec. 6, "Code is imported (so it compiles and its top level runs); code that waits for input at its top level is
    only compiled".** This is the service (`apps/cli/src/code.rs` `check`). The harness that produced Table 5 and
    Fig. 3 decided by task type instead: every script only compiled, functions and fixes imported
    (`git show 70a0a31:experiments/v3/code_eval.py`, `self_check`). Fix: add "(the evaluation harness compiled every
    script and imported other code)".
21. **Sec. 7, "Kinds-v2 added code and more notes, rewrite and poem families".** Kinds-v2 adds 32 families: 12 code, 6
    chat (poems and explaining code), 6 notes, 5 rewrite and 3 app (families in `datasets/kestrel_kinds_v2/splits/`
    not in `kestrel_kinds_v1`). Fix: "Kinds-v2 added code families and more notes, rewrite, chat (poems, explaining code)
    and app families".
22. **Table 5, "the mean seconds per request" (and Sec. 8.4 "A request took 8.9 s on average").** The values are
    generation time only (the sum of each request's attempt `ms`). Wall time per request, which includes the sandbox
    checks, is 11.2, 10.5, 29.1, 9.2 and 9.0 s (`wall_s` / 24), so the final-harness dev row would read 9.2, not 9.1.
    Fix: caption "the mean model (generation) seconds per request"; Sec. 8.4 "Gemma took 8.9 s per request".
23. **Sec. 8.3, "On development data the same system declined only 4/6 unanswerable questions."** `qa_dev.json` (26
    questions) was run after the test, with the Hinglish note added (e12cb18, KYROS/CHANGELOG.md); the earlier dev run
    `qa_dev_first.json` (24 questions) also declined 4/6, with the same two misses. Fix: "On development data, before
    and after the language note, the system declined only 4/6 unanswerable questions."
24. **Sec. 8.7, "17/36 requests (47.2%) were answered in under a second: task commands, searches, memory and saving a
    note".** The 17 also include the greeting (0.1 s), creating a task with a time (0.3 s), opening a document (0.3 s)
    and the live-data refusal (0.9 s); the Hinglish search by meaning took 2.6 s (`socket.json` `s`). Fix: "small talk,
    creating and changing tasks, keyword and English searches, opening a note, memory, saving a note and a refusal".
25. **Sec. 9, "the Kinds-v2 head's test ECE stayed at 0.072".** After scaling, validation ECE was 0.055; 0.072 on test is
    higher, not the same. Fix: "and the Kinds-v2 head's test ECE was 0.072".
26. **Sec. 12, "exposes the notes to anyone on that network who has the link and can read its traffic".** Either is
    enough: the key travels over plain HTTP, so reading the traffic gives it (KYROS/DECISIONS.md ADR-047). Fix: "to anyone
    on that network who has the link or can read its traffic".
Table rows for the items above: 1 = row 49, 2 = 116, 3 = 157, 4 = 201, 5 = 23, 6 = 97, 7 = 192, 8 = 194,
9 = 149, 10 = 172, 11 = 195, 12 = 89, 13 = 213, 14 = 16, 15 = 18, 16 = 27, 17 = 32, 18 = 44, 19 = 75, 20 = 72,
21 = 86, 22 = 144, 23 = 151, 24 = 190, 25 = 199, 26 = 210.

## Table of every checked item

Status key: OK; MISMATCH (value or fact differs); ROUNDING (right count, wrong rounding); UNSUPPORTED (the logs
or code cannot back it, or contradict part of it); WORDING (numbers right, statement inexact).

| # | Section | Claim as printed | Value | Source | Independent value | Status |
|---|---|---|---|---|---|---|
| 1 | Abstract | 6-core laptop with 14.95 GiB of RAM | 6 cores, 14.95 GiB | experiments/PREREG.md "Machine:" line | Ryzen 5 5500U (6C/12T), 14.95 GiB | OK |
| 2 | Abstract | a single 4-bit SLM (Gemma-4-E2B) | 4-bit | code logs `model_file` | gemma-4-E2B-it-Q4_0.gguf | OK |
| 3 | Abstract | held-out test split (n = 280) | 280 | fallback_test_v2.json `n`; kinds_v2 manifest test items | 280 | OK |
| 4 | Abstract, Sec. 8.1, Table 2 | right for 256/280 (91.4%, 95% CI [87.6%, 94.2%]) | 256/280 | fallback_test_v2.json `all.head_gemma.right` | 256/280 = 91.43%; Wilson [87.56, 94.17] | OK |
| 5 | Abstract | started a skill nobody asked for 6 times | 6 | `all.head_gemma.skill_not_asked_for` | 6 (wrong list: 3 jokes, google, London, code-10 loop fix) | OK |
| 6 | Abstract | asked the user 13 times | 13 | `all.head_gemma.asked` | 13 | OK |
| 7 | Abstract, Sec. 10 | 256 of those had been scored once before | 256 | kinds_v1 vs kinds_v2 test ids; `seen.n` | 256 shared ids, 24 fresh | OK |
| 8 | Abstract, Sec. 8.1 | on the 24 fresh ones right for 23/24 | 23/24 | `fresh.head_gemma.right` | 23/24 (only miss code-10/hi-en/1) | OK |
| 9 | Abstract | found the right note for 14/18 test queries | 14/18 | meaning_test_translated.json rows: best note = query's note and score ≥ 0.83 | 14/18 | OK |
| 10 | Abstract | added no note to queries that had none | 0 | same rows, `q-none-*` best 0.8007, 0.7957 < 0.83 | 0/2 | OK |
| 11 | Abstract | raised Hinglish from 1/9 to 9/9 (dev) | 1/9, 9/9 | meaning_dev.json vs meaning_dev_translated.json rows | 1/9, 9/9 | OK |
| 12 | Abstract, Sec. 8.3 | 19/19 answerable correct, 6/6 unanswerable declined | 19/19, 6/6 | qa_test.json rows `grade`, ids `qa-none*` | 19/19, 6/6 | OK |
| 13 | Abstract, Sec. 8.4 | 24 held-out code requests, right 21/24, only 3/6 on scripts that read input | 21/24, 3/6 | test_gemma-4-e2b.json `results[].pass_loop`; tasks.toml: every script reads stdin | 21/24; scripts 3/6 | OK |
| 14 | Abstract, Sec. 8.4 | retry loop never fired | 0 retries | test log `attempts` per result | 24 attempts for 24 requests; every `error` empty | OK |
| 15 | Abstract, Sec. 8.6 | non-reclaimable memory 3247 to 1144 MiB | 3247, 1144 | mmap/20261026T140055Z.jsonl measured rows, median `anon_kib`/1024 | 3247.05, 1143.71 | OK |
| 16 | Abstract | a cached prompt prefix cut its start from 19.1 to 2.0 s | 19.1, 2.0 | worker/20261026T142304Z.tsv medians | v2 19.13; v3 computed 17.36; v3 loaded 2.02 | WORDING |
| 17 | Abstract, Sec. 8.7 | acceptance run passed 34/36 | 34/36 | acceptance/socket.json `results[].ok` | 34/36 | OK |
| 18 | Abstract; Sec. 1 item 1 | when that is unsure too, the user is asked | - | apps/cli/src/kinds.rs `decide`, `decide_fallback` | top kind app below τ goes the app path, no question | WORDING |
| 19 | Sec. 1 | v1's primary hypothesis was supported on CLINC150 | supported | experiments/logs/E1/analysis_clinc150_test.json `h1.outcome` | "supported" | OK |
| 20 | Sec. 1 item 1 | old app path and six skills | 6 | kinds_v2 labels | chat, code, email, notes, rewrite, translate | OK |
| 21 | Sec. 1 item 4, Sec. 6 | at most two retries | 2 | apps/cli/src/code.rs `RETRIES` | 2 | OK |
| 22 | Sec. 1 item 5 | 15 GiB of RAM | 15 | PREREG Machine line | 14.95 GiB | OK |
| 23 | Sec. 1 item 6 | six small English–Hinglish sets with one test run each | 6 | Table 1; test logs present | 5 sets test-run once | MISMATCH |
| 24 | Sec. 1 item 6 | scripts that read input fail half the time | 1/2 | test log scripts | 3/6 failed | OK |
| 25 | Sec. 2 | encoder does not place Romanized Hindi queries near English notes | - | meaning_dev.json rows | right note closest: English 9/9, Hinglish 2/9 | OK |
| 26 | Sec. 2, 3 | Gemma-4-E2B at 4-bit with llama.cpp; Qwen3 models compared on dev | - | dev code logs | Q4_0; Qwen3-1.7B / 4B Q4_K_M dev only | OK |
| 27 | Fig. 1 caption | Saving a note and task commands are checked first, on the text as typed | - | apps/cli/src/assistant.rs `respond` | note save on raw text; task commands on normalized `req.text` | WORDING |
| 28 | Fig. 1 | order: save note, task command, kind head, τ, Gemma τg, head decides, skill asks y/N | - | assistant.rs; kinds.rs `handle` | same order and branches | OK |
| 29 | Fig. 1 | notes: the 2 closest notes; code: at most 2 retries | 2, 2 | notes_qa.rs `N`; code.rs `RETRIES` | 2, 2 | OK |
| 30 | Sec. 3 | encoder multilingual-e5-small, 8-bit | q8_0 | fallback configs `encoder` | multilingual-e5-small-q8_0 | OK |
| 31 | Sec. 3 | worker stopped after ten idle minutes | 10 | apps/cli/src/main.rs `idle = 10`; installed unit ExecStart has no --idle-minutes | 10 min | OK |
| 32 | Sec. 3 | answers from the SLM are streamed as they are written | - | notes_qa.rs header | notes answers not streamed | WORDING |
| 33 | Sec. 3 | every chat answer ends with a note that an offline small model wrote it and may be wrong | - | kinds.rs `CAVEAT`, `handle`; socket.json chat answers | "(Written offline by a small model: it may be wrong.)" | OK |
| 34 | Sec. 3 | current facts, live data, calculations stay on the app path and are refused | - | kinds_fallback_v2.json `system` rule 2; socket.json case 34 | app; weather request refused | OK |
| 35 | Sec. 3 | "hindi" in Devanagari, Hinglish in Latin letters (note after request) | - | kinds.rs `language_note` | as stated | OK |
| 36 | Sec. 3 | caps 512 tokens chat and email, 384 rewrite and translate; capped answer says so | 512, 384 | kinds.rs `max_tokens`, `CUT` | 512, 384; CUT line | OK |
| 37 | Sec. 3 | Gemma-4-E2B decodes 12.1 tok/s | 12.1 | E0 CSVs, tg64 gemma-4-e2b-it-q4_0, mean of session means | 12.180, 12.011, 12.236: mean 12.142 | OK |
| 38 | Sec. 3 | reads a prompt at 58 tok/s | 58 | E0 pp128, same sessions | 58.458, 58.685, 57.829: mean 58.32 | OK |
| 39 | Sec. 3, Sec. 8.6 | 1132-token few-shot prompt prefix | 1132 | worker/README.md | 1132 tokens (README only; not in the .tsv) | OK |
| 40 | Sec. 3 | prefix state loaded if and only if the saved token list equals the prefix | - | crates/kestrel-inference/src/llama.rs `prime_cached` | `saved == tokens`, else computed and saved | OK |
| 41 | Sec. 3 | page binds to 127.0.0.1 by default; key for its API | - | apps/cli/src/web.rs `bind`; acceptance README "API 401 without the key" | as stated | OK |
| 42 | Sec. 3 | wifi option exists, plain HTTP, is off | off | KYROS/CHANGELOG.md 2026-10-27 "wifi sharing turned off"; installed unit has no --web-lan | off | OK |
| 43 | Sec. 4 | kinds list; code = write, fix, shorten, test; explaining code is chat | - | kinds_fallback_v2.json `system` | as stated | OK |
| 44 | Sec. 4 | kinds added in three dataset versions (Table 1) | 3 | Table 1; manifests | 3 versions, Table 1 lists 2 | WORDING |
| 45 | Sec. 4 | one e5 pass with "query:" prefix | - | experiments/tier3/embed.py e5 prefix; kind_head.py `input_prefix` | "query: " | OK |
| 46 | Sec. 4 | multinomial logistic regression; C chosen on validation macro-F1 | - | final_kinds_v1/v2.json `c_grid_val_macro_f1`, `c` | argmax C = 10 (v1, 0.7918), 100 (v2, 0.8168) | OK |
| 47 | Sec. 4 | T cross-fitted, 5 folds by template family; final head on train + cal | 5 | `folds`, `temperature_note`, `fit_n` | 5; fit_n 1035 = 920 + 115, 1123 = 1000 + 123 | OK |
| 48 | Sec. 4 | Kinds-v0 calibration split alone 84 to 103 items | 84 to 103 | dev_v0..v4.json `cal_n` | 84, 100, 103, 103, 103 | OK |
| 49 | Sec. 4 | cal-fitted T made validation ECE worse in three of five dev runs | 3 of 5 | dev_v0..v4.json `val_raw.ece` vs `val_calibrated.ece` | worse in 4 of 5 (v1, v2, v3, v4) | MISMATCH |
| 50 | Sec. 4 | Gemma: definitions, two examples per kind and language | 2 | fallback_dev_v2.json `meta.shots_per_kind_and_language`, 28 shots | 2 (7 kinds × 2 languages × 2) | OK |
| 51 | Sec. 4 | one label under a grammar that allows only the labels | - | crates/kestrel-inference/src/tier.rs (label trie) | constrained decoding over a label trie (same effect) | OK |
| 52 | Sec. 4 | Gemma confidence temperature-scaled on cal by BCE, as v1's Tier 4 | - | fallback_dev_v2.json `temperature_note` | "fitted on cal (BCE)", T 1.741 | OK |
| 53 | Sec. 4 | four outcomes; below τ Gemma decides at τg, else head; a skill is asked | - | kinds.rs `decide_fallback` | as stated | OK |
| 54 | Sec. 4 | grid {0.5, ..., 0.9}² | 25 cells | fallback_dev_v1/v2.json `val_grid` keys | 25 cells | OK |
| 55 | Sec. 4 | rule fixed before the run: fewest wrong, fewest asks, lower τ, higher τg | - | `chosen.rule` | same rule | OK |
| 56 | Sec. 4 | Kinds-v1: τ = 0.7, τg = 0.9 | 0.7, 0.9 | my argmin over fallback_dev_v1.json `val_grid` (wrong = skill_not_asked_for + skill_sent_old_way) | min wrong 3 at (0.7,0.9) 14 asks, (0.8,0.9) 16, (0.9,0.9) 18: (0.7, 0.9) | OK |
| 57 | Sec. 4 | Kinds-v2: τ = 0.8, τg = 0.7 (142/152 right on validation) | 0.8, 0.7, 142/152 | same over fallback_dev_v2.json | min wrong 6 at 5 cells; fewest asks 4 at (0.8, 0.7); right 142/152 | OK |
| 58 | Sec. 4 | service uses the Kinds-v2 head and settings | - | kinds.rs `FALLBACK_CONFIG`; kinds_fallback_v2.json `tau`, `tau_g` | kinds_fallback_v2.json, 0.8 / 0.7 | OK |
| 59 | Sec. 5 | chunks = title + one paragraph, "passage:", cached by hash of the note's text | - | apps/cli/src/meaning.rs `chunks`, `fnv`, `scored` | as stated | OK |
| 60 | Sec. 5 | keyword search first; fewer than asked; single closest by meaning ≥ θ; "(by meaning)" | 1 | act.rs; notes_meaning_v0.json `added` 1, `k` 3 | as stated | OK |
| 61 | Sec. 5 | query with a common Hindi word put into English by Gemma | - | meaning.rs `needs_translation` | as stated | OK |
| 62 | Sec. 5 | θ maximises share found minus share of none-queries given a note | - | meaning_dev*.json `grid` | no translation: max 0.389 at 0.85 only; translated: 1.0 at 0.83 only | OK |
| 63 | Sec. 5 | Q&A: N = 2 closest notes; none if the closest is below θ | 2 | notes_qa.rs `N`; meaning.rs `closest`; qa logs `n_notes_given` | 2 | OK |
| 64 | Sec. 5 | answer only from them, in the question's language, or exactly NOT IN NOTES | - | notes_qa.rs `SYSTEM` | as stated | OK |
| 65 | Sec. 5 | grounding check: every number and name in the notes | - | notes_qa.rs header, grounding `check_facts` | as stated | OK |
| 66 | Sec. 5 | reply "The notes do not say this: so the reply is not available for this information" | - | notes_qa.rs `NOT_IN_YOUR_NOTES`; socket.json case 27 | identical text | OK |
| 67 | Sec. 5 | unchecked answer: the closest note instead | - | notes_qa.rs `answer` | as stated | OK |
| 68 | Sec. 6 | short standard-library Python, at most 640 tokens | 640 | datasets/contracts/code_system.txt; code.rs `MAX_TOKENS` | 640 | OK |
| 69 | Sec. 6 | bubblewrap, every namespace unshared, /usr read-only, empty /tmp, fresh work folder, clean env, no home | - | apps/cli/src/sandbox.rs `command` | --unshare-all, --ro-bind /usr, --tmpfs /tmp, --bind work, --clearenv, no /home | OK |
| 70 | Sec. 6 | 512 MiB memory, 10 s CPU, 10 MiB files, 64 processes, wall-clock timeout, python3 -E -S | 512 MiB, 10 s, 10 MiB, 64 | sandbox.rs prlimit args; code.rs `CHECK_TIMEOUT` | --as=536870912, --cpu=10, --fsize=10485760, --nproc=64; 15 s wall; -E -S | OK |
| 71 | Sec. 6 | imported; code waiting for input at top level only compiled | - | code.rs `check` | as stated for the service | OK |
| 72 | Sec. 6 | (implied) the measured loop works this way | - | `git show 70a0a31:experiments/v3/code_eval.py` `self_check` | harness: scripts only compiled, others imported, by type | WORDING |
| 73 | Sec. 6 | syntax error, crash or timeout back to Gemma, at most two times | 2 | code.rs `write` | as stated | OK |
| 74 | Sec. 6 | one line saying what was checked ("I checked it in a safe sandbox: no errors.") and a caveat | - | code.rs `RAN`, `CAVEAT`; socket.json case 29 | identical text | OK |
| 75 | Sec. 6 | Qwen3-4B's own asserts failed on right code and caused 8 retries | 8 | dev_qwen3-4b.json `attempts` | 32 − 24 = 8; 4 from wrong asserts, 4 from asserts fed as script input | WORDING |
| 76 | Sec. 6 | without changing a single outcome | - | dev_qwen3-4b.json `pass1` vs `pass_loop` | equal for all 24 | OK |
| 77 | Table 1 | Kinds-v1: 6 kinds, 920 / 115 / 136 / 256, reviewed | as printed | datasets/kestrel_kinds_v1/splits/manifest.json | 6 labels; 920/115/136/256; "reviewed" | OK |
| 78 | Table 1 | Kinds-v2: 7 kinds (+ code), 1000 / 123 / 152 / 280 (24 fresh), claude reviewed | as printed | kestrel_kinds_v2 manifest | 7 labels; 1000/123/152/280; test fresh 24; "claude-reviewed" | OK |
| 79 | Table 1 | NotesQ-v0: 18 fixture notes, dev 22 (4 none) / test 20 (2 none), reviewed | as printed | kestrel_notesq_v0 manifest; notes/ folder | 18 files (17 + docs/wifi-setup.md); 22 (4), 20 (2); reviewed | OK |
| 80 | Table 1 | NotesQA-v0: dev 26 (6 unans.) / test 25 (6 unans.), reviewed | as printed | kestrel_notesqa_v0 manifest; qa logs | 26 (6), 25 (6); reviewed | OK |
| 81 | Table 1 | CodeTasks-v0: 4 types, dev 24 / test 24 (24 families), claude reviewed | as printed | kestrel_codetasks_v0 manifest | 4 types; 24 / 24; 12 + 12 families; claude-reviewed | OK |
| 82 | Table 1 caption | Kinds-v2 test = Kinds-v1 test + 24 fresh items | 24 | test.jsonl ids | 256 shared + 24 fresh (8 code, 4 app, 4 chat, 4 notes, 4 rewrite) | OK |
| 83 | Table 1 caption | "claude reviewed": author waived, assistant reviewed with a separate agent | - | KYROS/DECISIONS.md ADR-045 | as stated | OK |
| 84 | Sec. 7 | app items from Intents-v0 train, cal and val (test never read) | - | Kinds-v2 `intents:` ids looked up in kestrel_intents_v0 splits | 604 train, 100 cal, 128 val families' items, 0 test | OK |
| 85 | Sec. 7 | Kinds-v0 (5 kinds) reviewed but never tested | 5 | kinds_v0 manifest; logs/v3/kinds | 5 labels, "reviewed"; no fallback_test_v0 log | OK |
| 86 | Sec. 7 | Kinds-v2 added code and more notes, rewrite and poem families | - | kinds_v2 families not in kinds_v1 | 32: 12 code, 6 chat, 6 notes, 5 rewrite, 3 app | WORDING |
| 87 | Sec. 7 | CodeTasks: 24 families of four types, one English and one Hinglish request each, hidden checks; last output line holds the answer as whole words | - | tasks.toml; code_eval.py `answers` | as stated | OK |
| 88 | Sec. 7 | NotesQ queries share no keyword with their note | - | meaning logs `keyword` | [] for every query | OK |
| 89 | Sec. 7 | timings: AC power, boost on, other software idle | - | mmap/README.md conditions | AC, boost on, Kestrel's service idle, load1 1.2–2.6 | UNSUPPORTED |
| 90 | Sec. 7 | proportions with 95% Wilson intervals from the logged counts | - | recomputed every interval | all match (rows 4, 99, 118, 128, 136, 145, 152) | OK |
| 91 | Table 2 | Kinds-v1 head alone: 206/256 (80.5%), 27/76, 179/180, asked 18, old way 32, not asked for 0 | as printed | fallback_test_v1.json `all.head_alone`, `by_gold` (skill = all − app) | 206 (80.47%), 27/76, 179/180, 18, 32, 0 | OK |
| 92 | Table 2 | Kinds-v1 head + Gemma: 220/256 (85.9%), 40/76, 180/180, 11, 25, 0 | as printed | `all.head_gemma`, `by_gold.app` | 220 (85.94%), 40/76, 180/180, 11, 25, 0 | OK |
| 93 | Table 2 | Kinds-v2 head alone: 221/280 (78.9%), 40/96, 181/184, 37, 20, 2 | as printed | fallback_test_v2.json | 221 (78.93%), 40/96, 181/184, 37, 20, 2 | OK |
| 94 | Table 2 | Kinds-v2 head + Gemma: 256/280 (91.4%), 78/96, 178/184, 13, 5, 6 | as printed | fallback_test_v2.json | 256 (91.43%), 78/96, 178/184, 13, 5, 6 | OK |
| 95 | Fig. 2 | v1 head alone 27 / 17 / 32 (n = 76); v1 head + Gemma 40 / 11 / 25 | as printed | `by_gold` summed over skill kinds | 27/17/32/0; 40/11/25/0 | OK |
| 96 | Fig. 2 | v2 head alone 40 / 35 / 20 (+ small bar); v2 head + Gemma 78 / 12 (+ two small bars) (n = 96) | as printed | same | 40/35/20/1; 78/12/5/1 | OK |
| 97 | Fig. 2 caption | The fallback mostly turns questions to the user into right answers | - | asked(fallback) ⊆ asked(head alone) by `decide_fallback`; Table 2 counts | v1: 6 of 17 questions resolved, right +13 (≥ 7 from old way) | MISMATCH |
| 98 | Fig. 2 caption | Kinds-v1: a third of skill requests still sent the old way; Kinds-v2: most gone; 256 of 280 seen | 1/3 | Table 2 | 25/76 = 32.9%; 5/96 | OK |
| 99 | Sec. 8.1 | Kinds-v1 head alone 206/256; with fallback 220/256 (85.9%, CI [81.1%, 89.7%]) | as printed | fallback_test_v1.json | 85.94%; Wilson [81.15, 89.67] | OK |
| 100 | Sec. 8.1 | no skill written that was not asked for (v1) | 0 | `all.head_gemma.skill_not_asked_for` | 0 | OK |
| 101 | Sec. 8.1 | every app request right 180/180; 40/76 skills; notes 1/12, rewrite 4/16; 25 old way | as printed | `by_gold`, `all` | as printed | OK |
| 102 | Sec. 8.1 | "which floor is the dentist on?" looked like app to the head and Gemma | - | fallback_test_v1.json `wrong` notes-14/en/1 | head app 0.515, Gemma app 0.653 | OK |
| 103 | Sec. 8.1 | v2: 78/96 skill; 5 old way; 6 skill not asked for; 13 asked | as printed | fallback_test_v2.json | 78, 5, 6, 13 | OK |
| 104 | Sec. 8.1 | fallback mattered most for chat and translation: 6/20 to 20/20, 8/16 to 16/16 | as printed | `by_gold.chat`, `.translate` | +14 and +8, the two largest gains | OK |
| 105 | Sec. 8.1 | cost app requests a little: 181/184 to 178/184 | as printed | `by_gold.app` | 181, 178 | OK |
| 106 | Sec. 8.1 | Gemma took three jokes and "google exam preparation" as chat | - | `wrong`: intents:chat-07 ×3, intents:oos-web-01 | Gemma chat 0.849, 0.761, 0.763, 0.800 (≥ τg 0.7), head app below 0.8 | OK |
| 107 | Sec. 8.1 | the head, above τ, sent "london mein abhi kitne baje hain?" to notes | - | `wrong` app-14/hi-en/0 | head notes 0.911 ≥ 0.8 | OK |
| 108 | Sec. 8.1 | rewrite weakest, 8/20 right, most others asked | 8/20 | `by_gold.rewrite.head_gemma` | 8 right, 10 asked, 2 old way; lowest share of any kind | OK |
| 109 | Sec. 8.1 | fresh: 23/24 (CI [79.8%, 99.3%]) vs 9/24 head alone, which asked 11 times | as printed | `fresh` | 23/24 Wilson [79.76, 99.26]; 9/24; asked 11 | OK |
| 110 | Sec. 8.1 | English 130/140, Hinglish 126/140 | as printed | `by_lang.*.head_gemma.right` | 130, 126 | OK |
| 111 | Sec. 8.1 | validation accuracy 0.926 (v1), 0.862 (v2) | as printed | final_kinds_v1/v2.json `val_calibrated.accuracy` | 0.92647, 0.86184 | OK |
| 112 | Sec. 8.1 | macro-F1 0.799 and 0.761 | as printed | `val_calibrated.macro_f1` | 0.79858, 0.76121 | OK |
| 113 | Sec. 8.1 | ECE 0.147 to 0.076 and 0.059 to 0.055 | as printed | `val_raw.ece`, `val_calibrated.ece` | 0.14671 to 0.07564; 0.05874 to 0.05469 | OK |
| 114 | Sec. 8.1 | v2 head on test: accuracy 0.900, macro-F1 0.819, ECE 0.072 | as printed | fallback_test_v2.json `head` | 0.9, 0.81852, 0.07196 | OK |
| 115 | Sec. 8.1 | recall lowest for chat (0.50) and code (0.63) | as printed | `head.per_class.*.recall` | chat 0.5, code 0.625 (half up 0.63), next translate 0.75 | OK |
| 116 | Sec. 8.1 | unsure val items (v2): Gemma right 21 of 26, head 10 | 21/26, 10 | fallback_dev_v2.json `val_head_unsure` (at `tau` 0.7); fallback_outputs_v2.jsonl val rows | at 0.7: 26, 21, 10; at chosen τ 0.8: 38, Gemma 30, head 19 | MISMATCH |
| 117 | Sec. 8.1 | Gemma's label call 523 ms median, p95 717 ms (dev) | 523, 717 | fallback_outputs_v2.jsonl `gemma.ms` (275 calls) | median 523.46; p95 (nearest rank) 717.04 | OK |
| 118 | Sec. 8.1 | v1 encoder-alone median 5.6 ms | 5.6 | E1/analysis_intents_v0_test.json `systems.R1.latency_ms.median` | 5.623 | OK |
| 119 | Table 3 | app 179/180, 180/180, 181/184, 178/184 | as printed | `by_gold.app` v1, v2 | same | OK |
| 120 | Table 3 | chat 2/16, 11/16, 6/20, 20/20 | as printed | `by_gold.chat` | same | OK |
| 121 | Table 3 | code –, –, 1/8, 7/8 | as printed | `by_gold.code` (v2 only) | same | OK |
| 122 | Table 3 | email 12/16, 13/16, 12/16, 15/16 | as printed | `by_gold.email` | same | OK |
| 123 | Table 3 | notes 1/12, 1/12, 7/16, 12/16 | as printed | `by_gold.notes` | same | OK |
| 124 | Table 3 | rewrite 3/16, 4/16, 6/20, 8/20 | as printed | `by_gold.rewrite` | same | OK |
| 125 | Table 3 | translate 9/16, 11/16, 8/16, 16/16 | as printed | `by_gold.translate` | same | OK |
| 126 | Table 4 | no translation (dev, θ 0.85): 7/18, 6/9, 1/9, 0/4 | as printed | meaning_dev.json rows | 7/18, 6/9, 1/9, 0/4 | OK |
| 127 | Table 4 | via English (dev, θ 0.83): 18/18, 9/9, 9/9, 0/4 | as printed | meaning_dev_translated.json rows | same | OK |
| 128 | Table 4 | via English (test, θ 0.83): 14/18, 7/9, 7/9, 0/2 | as printed | meaning_test_translated.json rows | same | OK |
| 129 | Table 4 caption | every query's note is missed by the keyword search | - | `keyword` field | [] for all 64 queries | OK |
| 130 | Sec. 8.2 | without translation the right note was closest for 9/9 English, 2/9 Hinglish | 9/9, 2/9 | meaning_dev.json rows | 9/9, 2/9 | OK |
| 131 | Sec. 8.2 | at θ = 0.85 added for 6/9 and 1/9 | 6/9, 1/9 | same | 6/9, 1/9 | OK |
| 132 | Sec. 8.2 | "maa ke liye tohfa" became "gift for mother" | - | meaning_dev_translated.json q-mom/hi-en/0 `asked` | "gift for mother" | OK |
| 133 | Sec. 8.2 | translated: right note closest for 18/18; θ 0.83 added all; no note to the 4 none-queries | 18/18, 0/4 | meaning_dev_translated.json | 18/18; none-query best 0.8146, 0.7927, 0.8104, 0.8230 < 0.83 | OK |
| 134 | Sec. 8.2 | test 14/18 (77.8%, CI [54.8%, 91.0%]) | as printed | meaning_test_translated.json | 77.78%; Wilson [54.79, 91.00] | OK |
| 135 | Sec. 8.2 | 7/9 and 7/9; no wrong note (0/2) | as printed | same | 7/9, 7/9; Goa 0.8131 below θ, so no wrong note added | OK |
| 136 | Sec. 8.2 | closest note right for 16/18 | 16/18 | same | 16/18 | OK |
| 137 | Sec. 8.2 | two queries found their note just below θ | 2 | q-goa en / hi-en | 0.8283, 0.8298 < 0.83 | OK |
| 138 | Sec. 8.2 | "festival shopping" ranked Goa above Diwali in both languages | - | q-diwali en / hi-en | best goa-trip 0.8131, note rank 2, both | OK |
| 139 | Table 5 | Gemma first harness (dev): 6/6, 1/6, 6/6, 6/6, 19/24, 11.1 | as printed | dev_gemma-4-e2b.json rows; sum of attempt ms / 24 | same; 11.14 s | OK |
| 140 | Table 5 | Qwen3-1.7B: 5/6, 1/6, 5/6, 6/6, 17/24, 10.4 | as printed | dev_qwen3-1.7b.json | same; 10.42 s | OK |
| 141 | Table 5 | Qwen3-4B: 6/6, 5/6, 6/6, 6/6, 23/24, 29.0 | as printed | dev_qwen3-4b.json | same; 28.99 s | OK |
| 142 | Table 5 | Gemma final (dev): 6/6, 5/6, 6/6, 6/6, 23/24, 9.1 | as printed | dev_gemma-4-e2b_adr044.json | same; 9.07 s | OK |
| 143 | Table 5 | Gemma final (test): 6/6, 3/6, 6/6, 6/6, 21/24, 8.9 | as printed | test_gemma-4-e2b.json | same; 8.92 s | OK |
| 144 | Table 5 caption; Sec. 8.4 | "mean seconds per request"; "A request took 8.9 s on average" | 11.1 ... 8.9 | attempt ms (generation) vs `wall_s` | generation 11.1/10.4/29.0/9.1/8.9; wall 11.2/10.5/29.1/9.2/9.0 | WORDING |
| 145 | Table 5 caption | in every run pass@1 equalled the result after the loop | - | all five code logs | `pass1` = `pass_loop` for every request | OK |
| 146 | Fig. 3 | dev 6/6, 5/6, 6/6, 6/6; test 6/6, 3/6, 6/6, 6/6 | as printed | dev _adr044 and test logs | same | OK |
| 147 | Sec. 8.3 | 25/25 graded right (CI [86.7%, 100.0%]) | as printed | qa_test.json rows | 25/25; Wilson [86.68, 100.0] | OK |
| 148 | Sec. 8.3 | for every answerable question the right note was among the two given | - | qa_test.json `top` vs `notes` | 19/19 | OK |
| 149 | Sec. 8.3 | an answer took 5.0 s on average | 5.0 | qa_test.json `questions[].s` | 4.947 (rows); logged mean 4.95 | ROUNDING |
| 150 | Sec. 8.3 | at most 6.8 s | 6.8 | same | 6.78 | OK |
| 151 | Sec. 8.3 | on dev the same system declined only 4/6 | 4/6 | qa_dev.json, qa_dev_first.json | 4/6 in both; qa_dev.json has the later Hinglish note | WORDING |
| 152 | Sec. 8.3 | both dev misses: cake price, own words instead of the marker, app would show it | - | qa_dev.json qa-none-cake-price en / hi-en | `seen` "answered", own-words answers | OK |
| 153 | Sec. 8.3 | the test run came before the Hinglish note | - | KYROS/CHANGELOG.md: 177122b (test) before e12cb18 (note) | as stated | OK |
| 154 | Sec. 8.3 | 4/10 answered Hinglish test questions got a Hinglish answer (word list) | 4/10 | qa_test.json hi-en rows with `seen` answered, `is_hinglish` | 4/10 (doctor, goa-flight, goa-stay, tyre) | OK |
| 155 | Sec. 8.3 | with the note, 9/12 on dev | 9/12 | qa_dev.json, same check | 9/12 | OK |
| 156 | Sec. 8.4 | first harness: Qwen3-4B 23/24, Gemma 19/24, Qwen3-1.7B 17/24 | as printed | dev logs | same | OK |
| 157 | Sec. 8.4 | Gemma's misses were scripts (1/6) that printed input prompts or extra lines | - | dev_gemma-4-e2b.json answers re-run on hidden stdin | 3 prompts or extra words; 2 uncaught EOFError (sc-sum-lines en, hi-en) | MISMATCH |
| 158 | Sec. 8.4 | rescored 22/24 Gemma, 22/24 Qwen3-4B; write and fix 16/18 vs 17/18; 11.1 vs 29.0 s | as printed | re-ran tasks.toml checks on the logged answers (code_eval `passes`, `facts_hold`, sandbox); dev_rescored_review.json | 22, 22 (Qwen3-1.7B 18); 16/18, 17/18 | OK |
| 159 | Sec. 8.4 | Gemma's Hinglish explanations Hinglish 3/3, Qwen3-4B 0/3 | as printed | first-harness logs `explain_hi_en_in_hinglish` and rows | 3/3, 0/3 | OK |
| 160 | Sec. 8.4 | final harness: dev 23/24; test 21/24 (87.5%, CI [69.0%, 95.7%]); functions, fixes, explanations 6/6, scripts 3/6 | as printed | _adr044 and test logs | same; Wilson [69.00, 95.66] | OK |
| 161 | Sec. 8.4 | failed scripts: EOF until uncaught error; stopped at first blank line; reversed letters for "words ulte order" | - | test answers re-run on hidden cases; tasks.toml hi-en request | count-lines/en EOFError; count-lines/hi-en prints 1 (iter(input, '')); reverse-words/hi-en "dlrow gib olleh" | OK |
| 162 | Sec. 8.4 | sandbox raised no error in any test answer (0 retries) | 0 | test log | all `error` empty, 24 attempts | OK |
| 163 | Sec. 8.4 | explanations asked in the request's language; English explanations answered in Hinglish (0/3) | 0/3 | 70a0a31 code_eval.py `EXPLAIN_SYSTEM`; test rows ex-*/en `hinglish` | true for all 3 | OK |
| 164 | Sec. 8.4 | fact grading passed a list-comprehension explanation with a wrong output list | - | test ex-list-comp/hi-en/0 (pass) vs tasks.toml code `[x*x for x in range(10) if x % 2 == 0]` | states 0, 4, ..., 324; right is 0, 4, 16, 36, 64 | OK |
| 165 | Sec. 8.5 | 23 dev requests; 14/23 first prompts, 19/23 after | as printed | writer/lang_baseline.json, lang_final.json, rescored with writer_lang checks | 14/23, 19/23 | OK |
| 166 | Sec. 8.5 | into Hindi (Devanagari) 0/4 to 4/4; into Hinglish 3/4 to 1/4, English back unchanged | as printed | same, groups | 0 to 4, 3 to 1; wl-16, 18, 19 echo the English | OK |
| 167 | Table 6 | copied cold 3656 / 111 / 3247 / 8 | as printed | mmap jsonl measured rows, medians (`ttft_first_ms`) | 3655.98 / 110.62 / 3247.05 / 7.63 | OK |
| 168 | Table 6 | copied warm 2596 / 103 / 3247 / 8 | as printed | same | 2596.35 / 103.12 / 3247.05 / 7.66 | OK |
| 169 | Table 6 | mapped cold 2477 / 111 / 1144 / 2702 | as printed | same | 2476.68 / 111.35 / 1143.71 / 2702.20 | OK |
| 170 | Table 6 | mapped warm 1383 / 117 / 1144 / 2702 | as printed | same | 1382.75 / 116.85 / 1143.71 / 2702.33 | OK |
| 171 | Table 6 caption | median of 3 runs each; one session; output byte-identical in all runs | 3 | jsonl | 3 measured per condition; 16 rows, 1 distinct text | OK |
| 172 | Sec. 8.6 | 3247 to 1144 MiB (2103 MiB, 65%) | 2103, 65% | same | 2103.34; 64.78% | ROUNDING |
| 173 | Sec. 8.6 | rest of the file 2702 MiB file-backed, clean | 2702 | same | 2702.2 | OK |
| 174 | Sec. 8.6 | peak-RSS counts those pages; mapped looks larger | - | jsonl `peak_kib` | mapped 3.39 to 3.76 GiB vs copied 3.18 GiB | OK |
| 175 | Sec. 8.6 | loading cold 3656 to 2477 ms; warm 2596 to 1383 ms | as printed | same | same | OK |
| 176 | Sec. 8.6 | first token 103–117 ms in every condition | 103–117 | condition medians | 103.1 to 116.8 | OK |
| 177 | Sec. 8.6 | per-layer embeddings almost half the file read at load; no setter for lazy mode | - | mmap/README.md finding 5 | 1.35 GB, 48%; no setter in llama-cpp-2 0.1.157 | OK |
| 178 | Sec. 8.6, Fig. 4 | v2 ready 19.1 s (median of 3); v3 computing 17.4 s; loading 2.0 s | as printed | worker .tsv | 19.13 (18.01, 19.13, 19.30); 17.36; 2.02 | OK |
| 179 | Sec. 8.6 | 9.5 times faster than v2 | 9.5 | 19.13 / 2.02 | 9.47 | OK |
| 180 | Sec. 8.6 | 1135 MiB anonymous at ready vs 3356 MiB | as printed | worker/README.md table (not in the .tsv) | 1135 / 3356 | OK |
| 181 | Fig. 4 | bars 3247, 1144 (+2702 file-backed); 19.1 s, 17.4 s, 2.0 s | as printed | as rows 167–178 | same | OK |
| 182 | Sec. 8.7 | 34/36 over the socket; list of features covered | 34/36 | socket.json | 34/36; features as listed | OK |
| 183 | Sec. 8.7 | first run 31/35, two failures from wrong checks, rerun fresh | 31/35 | socket_run1.json `summary`; acceptance README | 31/35; 4 failures: 2 wrong checks + the 2 limitations | OK |
| 184 | Sec. 8.7 | page 8/8 headless, 8/8 visible, Copy read back from clipboard | 8/8, 8/8 | page_headless.json, page_headed.json | 8/8, 8/8; "Copied" with the code text | OK |
| 185 | Sec. 8.7 | reminder due 17:31 shown 2 s after its minute began | 2 s | acceptance README | rang 17:31:02 | OK |
| 186 | Sec. 8.7 | 448 Rust, 510 Python, 6 sandbox, 4 model-worker tests | as printed | acceptance README | 448, 510, 6/6, 4/4 | OK |
| 187 | Sec. 8.7 | rent refused: head app 0.62 below τ, Gemma app; Q&A answers it when reached | 0.62 | acceptance README item 1; socket.json case 25; qa_dev.json qa-rent/en/0 | 0.62 (README only); refused; "Your rent is 12000." graded right | OK |
| 188 | Sec. 8.7 | "Which preferences have I set?" found nothing | - | socket.json case 17 | "I don't remember anything about that." | OK |
| 189 | Sec. 8.7 | 17/36 (47.2%) under a second; median 0.3 s | 17/36, 0.3 | socket.json `s` | 17 (47.22%); median 0.3 | OK |
| 190 | Sec. 8.7 | those were task commands, searches, memory and saving a note | - | socket.json | also greeting, task creation, open a document, refusal | WORDING |
| 191 | Sec. 8.7 | translation 2.8–3.2 s, email 8.5–10.8 s, notes answer 9.4 s, function 14.0 s | as printed | socket.json cases 23, 24, 20, 21, 26, 28 | same | OK |
| 192 | Sec. 9 | fallback turns most questions into right decisions | - | Table 2, subset argument | v1 18 to 11, v2 37 to 13 | MISMATCH |
| 193 | Sec. 9 | about half a second | ~0.5 s | row 117 | 523 ms | OK |
| 194 | Sec. 9 | refusing a skill request on the old path is the most common failure | - | Table 2 | v1 25 vs 0; v2 5 vs 6 | MISMATCH |
| 195 | Sec. 9 | jump 40/76 to 78/96 came from data, not from the method | - | Table 2; fresh ids; configs | different denominators; τ/τg and prompt changed too; same 76: 40 to 59 | UNSUPPORTED |
| 196 | Sec. 9 | Gemma matched Qwen3-4B on rescored dev in well under half its time; every test function and fix right | - | rows 158, 160 | 22 = 22; 11.1/29.0 = 38%; 6/6 and 6/6 | OK |
| 197 | Sec. 9 | sandbox only proves code compiles or imports | - | code.rs `check` | as stated | OK |
| 198 | Sec. 9 | cross-fitted temperatures lowered validation ECE for both heads | - | row 113 | both lowered | OK |
| 199 | Sec. 9 | the Kinds-v2 head's test ECE stayed at 0.072 | 0.072 | rows 113, 114 | val 0.055, test 0.072 | WORDING |
| 200 | Sec. 10 | 256 of Kinds-v2's 280 test items are Kinds-v1's | 256 | row 7 | 256 | OK |
| 201 | Sec. 10 | every set small (24 to 280 test items) | 24 to 280 | test sizes | 20 (NotesQ), 24, 25, 256, 280 | MISMATCH |
| 202 | Sec. 10 | two sets reviewed by the assistant and an independent agent | 2 | manifests `status` | Kinds-v2, CodeTasks-v0 claude-reviewed | OK |
| 203 | Sec. 10 | notes are 18 fixture files | 18 | datasets/kestrel_notesq_v0/notes | 18 | OK |
| 204 | Sec. 10 | 21/24 [69.0%, 95.7%]; 25/25 [86.7%, 100.0%] | as printed | Wilson | [69.00, 95.66]; [86.68, 100.00] | OK |
| 205 | Sec. 10 | script grading reads the last output line | - | code_eval.py `answers` | last non-empty line | OK |
| 206 | Sec. 11 | README lists 12 known limitations | 12 | acceptance/README.md | 12 | OK |
| 207 | Sec. 11 | the listed limitations (program to solve X to chat; rewrite asked; jokes to chat, time to notes; Markdown; Hinglish notes answers in English; stdlib only, never run with input; wifi; no conversation memory) | - | acceptance README items 3, 4, 6, 7, 8, 9, 11, 12 | as stated | OK |
| 208 | Sec. 11 | Qwen3-4B decodes 8.2 tok/s | 8.2 | E0 tg64 qwen3-4b-q4_k_m | 8.238, 8.216, 8.203: mean 8.219 | OK |
| 209 | Sec. 12 | code sandbox has no network and no access to the user's files | - | sandbox.rs (no /home bind, --unshare-all) | as stated | OK |
| 210 | Sec. 12 | wifi option exposes notes to anyone who has the link and can read its traffic | - | KYROS/DECISIONS.md ADR-047 | either suffices (key in plain HTTP) | WORDING |
| 211 | Sec. 12 | wifi off by default, off when the paper was written | off | ADR-047 (on only with KESTREL_WEB_LAN); CHANGELOG 2026-10-27; installed unit | off | OK |
| 212 | Sec. 13 | service starts in about 2.0 s | 2.0 | row 178 | 2.02 | OK |
| 213 | Reproducibility | logs at tag v3-frozen; every number generated by paper_assets_v3.py; paper/v3/number_check.md re-derives each | - | git status, git tags, paper/v3 listing, .tex | generator and paper untracked; number_check.md absent; typed numbers in .tex | UNSUPPORTED |
| 214 | Header | Draft, 28 October 2026, tag v3-frozen | - | `git rev-parse v3-frozen^{}` | ad920d5 = HEAD | OK |

## Counts (over the 214 table rows)

- OK: 188
- MISMATCH: 8 (rows 23, 49, 97, 116, 157, 192, 194, 201)
- ROUNDING: 2 (rows 149, 172)
- UNSUPPORTED: 3 (rows 89, 195, 213)
- WORDING: 13 (rows 16, 18, 27, 32, 44, 72, 75, 86, 144, 151, 190, 199, 210)

## Not checked

- Figure 1 as a picture (layout), the bibliography and citations (see `citation_check_v3.md`).
- Numbers that exist only in a README and not in a raw log: the 1132-token prefix, 1135 / 3356 MiB at ready,
  the rent question's head score 0.62, and the 448 / 510 / 6 / 4 test counts (no test-runner output is logged).
- "No component opens a network connection" (Sec. 3) beyond the sandbox and the web listener; no network audit
  of every crate was made.
- The ethics statement that the data "describe no real person".
