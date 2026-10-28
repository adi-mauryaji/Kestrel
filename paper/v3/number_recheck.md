# Re-check of the corrections · Kestrel v3 paper, report and slides

Date: 2026-10-28. Checked: every correction listed in `number_check.md` ("What changed after the checks": 6 generator
items, paper 1–26 plus the MBPP/HumanEval citation wording, report 1–19, slides F1–F18 = 70 rows), as each passage now
reads in `kestrel_v3.pdf` (built 08:24), `kestrel_v3_report.pdf` (08:24), `slides/kestrel_v3_slides.pdf` (08:24) and
`slides/speaker_notes.md` (`pdftotext -layout`; diagram pages and slides 5, 7–12 rendered and looked at).

How: values derived again from the raw files with Python (exact fractions, rounding half up, Wilson 95% with
z = 1.959964), not from the generator; code read in `apps/cli/src/` (kinds.rs `decide` / `decide_fallback` / `handle`,
assistant.rs `respond`, commands.rs, code.rs, act.rs `WithMeaning`, meaning.rs, summary.rs, service.rs), the harness at
`git show 70a0a31:experiments/v3/code_eval.py` and `0a83608` (first harness); decisions and changelog in the vault, read
only. `experiments/analysis/test_paper_assets_v3.py`: 9 passed. `generated/*` (08:24:01) is older than every PDF.

Result: **70 rows: 58 OK, 12 PROBLEM.** No number is wrong. The 12 problems are wording: 3 matter (P13, R19, P8), 9 are
minor. Every recomputed value requested matched:

- Kinds-v2 validation items with head top probability < τ = 0.8 (`kinds/fallback_outputs_v2.jsonl`, split "val", 152
  rows, none exactly at 0.8): **38**; Gemma's label right **30**, the head's top kind right **19**. (At 0.7: 26 / 21 /
  10 = the log's `val_head_unsure`. Kinds-v1 at its chosen 0.7: 24 / 18 / 15 = its log field.)
- Kinds-v2 new families (`datasets/kestrel_kinds_v2/families.toml`): **32 = 12 code, 6 chat, 6 notes, 5 rewrite, 3 app**;
  the same 32 come out as families in the v2 splits that are not in v1. Every v1 row stays in its split; v2 adds
  80 / 8 / 16 / 24 rows (train / cal / val / test).
- Notes Q&A answer seconds from the per-question rows: test (25) mean 4.9472 → **4.9**, max 6.78 → **6.8**; dev
  `qa_dev.json` (26) mean 5.1385 → **5.1**, max 8.81 → **8.8** (`qa_dev_first.json`: 5.9 / 8.9). The dev values are not
  printed in any document.
- Memory: medians 3247.05 and 1143.71 MiB; 2103.34 / 3247.05 = **64.78% → 64.8%** (2103 / 3247 = 64.77%, also 64.8%).
- Kinds-v0 dev runs with calibrated validation ECE above raw: **4 of 5** (v1, v2, v3, v4; v0 0.0511 → 0.0507 improved);
  calibration sizes 84, 100, 103, 103, 103.
- Questions to the user, all requests (skill requests only): Kinds-v1 test head alone **18 (17)**, head + Gemma
  **11 (11)**; Kinds-v2 test head alone **37 (35)**, head + Gemma **13 (12)**. Summed `by_gold` = `all` in both logs.

## PROBLEMS, with the exact fix

**Matter (claims about how the numbers were made, and one unsupported claim):**

1. **P13 · Paper, Reproducibility.** (a) "this paper and its generator were added in a later commit on the same
   branch" is not true yet: `paper/v3/`, `experiments/analysis/paper_assets_v3.py` and its test are untracked, and HEAD
   = tag `v3-frozen` = ad920d5. (b) "Every result number in this paper is generated ... by paper_assets_v3.py" is still
   false. These result values are typed in `kestrel_v3.tex`: "four of five development runs" and "84 to 103 items"
   (l. 183), "three jokes" (l. 311), "two queries found their note just below θ" (l. 349), the rent head score "0.62"
   (l. 458) and "about half a second" (l. 472). All of them are correct. **Fix:** commit the paper, the generator and its
   test before the PDF goes out (or write "will be added"). Then either make these values `\kv` keys, or write: "Every
   result number in this paper, except a few counts in the prose (four of five runs, 84 to 103 items, three jokes, two
   queries, and the rent score 0.62 from the acceptance README), is generated ...; those and the configuration
   constants are typed and were checked by hand."
2. **R19 · Report, Appendix B.** "Every result number in this report (each one typeset from a generated value)" is
   false for counts typed in the prose. In §6.1 these are "three jokes and a web search", "a time question" and "a loop
   fix" (`report.tex` l. 279); in §6.2 "In two of the misses" (l. 308); in §6.4 "the three English requests" (l. 327)
   and "Three models" (l. 321). All of them are correct. **Fix:** "Every result number in the tables, figures and
   results text is typeset from a generated value ... except a few small counts in the prose (three jokes and a web
   search, two of the misses, the three English explanation requests), which are typed and were checked by hand
   (`number_check_report.md`)." A minor point: "the encoder's latency in experiments/logs/E1/" names a source that no
   number in the report uses (the report has no `e1.*` value), so drop it.
3. **P8 · Paper Sec. 9, "since it usually ends in a refusal".** No log records what the app path answered to the 25
   (Kinds-v1) or 5 (Kinds-v2) requests sent the old way: `fallback_test_v*.json` stores only the routing decision. The
   report dropped "(and refused)" for this reason (report item 6), and ADR-037 says only "end as `unsupported` or in a
   wrong intent". Some of the five, such as "payment pending hai, uska reminder mail bana do" and "make a bulleted list
   from this: eggs bread butter and tea", may well become a task instead. Sec. 4's "(a skill request taken by the app
   path, which usually refuses it)" (l. 191) has the same problem, though it was there before. **Fix:** Sec. 9:
   "...since the app path refuses it or takes it for another action, and nothing tells them that a skill exists."
   Sec. 4: "(a skill request taken by the app path, which refuses it or takes it for another action)".

**Minor (true in substance, but incomplete or slightly off):**

4. **P17 / R12 / F4 · Streaming.** Note summaries are also written by Gemma and are shown only after the grounding
   check: `summary.rs` `ModelText::summarize` returns the whole text, checks it with `grounding::check`, and falls back
   to the first lines otherwise. Slide 10 even plots "Summarize a note 7.9 s" next to "Written answers appear word by
   word". **Fixes:**
   - paper Sec. 3 (l. 148): "...streamed as they are written, except notes answers and note summaries, which are shown
     after their grounding check";
   - report §3 (l. 177): "...except answers from the notes and summaries of them, which appear once they have been
     checked";
   - slide 10 (`deck.template.html` l. 668): "Written answers appear word by word; notes answers and summaries once
     checked."
5. **F10 (residual) · Slide 9 bar labels "copied in" / "from disk".** The subtitle was fixed, but the label "from disk"
   still says that the mapped model is read from disk. That is the idea F10 removed: llama.cpp maps the file with
   `MAP_POPULATE`, and all 2702 MiB are resident as file-backed pages (`mmap/README.md` finding 2). **Fix:** in
   `deck.template.html` l. 632–633, label the bars "copied" and "mapped".
6. **F1 · Notes §8, "As it writes code, it also tries it inside a sealed box".** In `code.rs` `write`, the code is
   streamed first and `check()` runs after the whole code is written, so "as it writes" suggests checking while it
   writes. The summary in `number_check.md` ("code is checked as it is written") has the same slip. **Fix:** notes §8:
   "Once it has written the code (you see it appear as it is written), Kestrel tries it inside a sealed box: ...".
   `number_check.md`: "code is shown as it is written and checked after it, not before it is shown".
7. **R16 · Report §6.2, festival shopping.** The clause leaves out "in both languages", so it accounts for only 3 of the
   4 misses. It also leaves out that the Goa note was below θ too (0.8131 < 0.83), so a reader may think a wrong note
   was added (`notes/meaning_test_translated.json`). **Fix:** "...; for “festival shopping” (in both languages) the
   closest note was the Goa trip note, also below the threshold, so nothing was added."
8. **R7 · Report §2, "A request outside its eight actions".** The sentence just before lists what v2 could do: add,
   list, finish, rename and reschedule tasks, remind, search, open, summarize and preferences. That is more than eight,
   because the v2 task commands (`commands.rs`) are not among the eight intents. **Fix:** "A request outside these,
   such as ..., got *I can't do that* or ended in a wrong action." This also drops the "usually", which ADR-037 does not
   quantify.
9. **P24 · Paper Sec. 8.7, list of sub-second requests.** One of the 17 is "delete task 3" (0.3 s, `socket.json` case
   9), which by the paper's own corrected Fig. 1 is not a task command: delete goes to the app path and asks y/N. Every
   other entry and the count are right: 17/36 = 47.2%, median 0.3 s. **Fix:** "the greeting, creating and deleting a
   task, task commands, keyword and English meaning search, ...".
10. **P12 · Paper Sec. 7, "Timings come from the laptop as found (AC power, CPU boost on, Kestrel's own service
    idle)".** These conditions are recorded only for the memory runs (`mmap/README.md`). `worker/README.md` records AC
    and boost on. The acceptance README says only "AC power. Laptop as found." No code, notes or kind log records
    conditions. The sentence applies the conditions to all timings. **Fix:** "Timings come from the laptop as found,
    not from v1's controlled protocol (the memory runs record AC power, CPU boost on and Kestrel's own service idle),
    and are comparable only within one log."

## Other issues found while checking (not among the corrections)

- **Report Fig. 2 caption** ("when that is unsure too, Kestrel asks the user") has the problem that paper item 15
  fixed: when Gemma is unsure and the head's top kind is app, the request takes the old path without a question
  (`kinds.rs` `decide`; the figure itself is right). **Fix:** "...; when that is unsure too, a request for a skill is
  put to the user as a question."
- **Paper Sec. 8.2** ("two queries found their note just below θ") has the problem that report item 16 fixed: in
  Table 4, "found" means closest *and* ≥ θ. **Fix:** "for two queries the right note was the closest but just below θ
  (0.8283, 0.8298)".
- **Paper Sec. 5** ("when it returns fewer notes than asked, the single closest note ... is added"): the code adds it
  only when the keyword hits are fewer than min(limit, k = 3), and the default limit is 10 (`act.rs` `WithMeaning`,
  `tools.rs` clamp). The report's corrected wording ("fewer than three") is right. **Fix:** "when it returns fewer than
  three notes (or fewer than asked, if fewer were asked)".
- **Paper abstract and Sec. 1** ("nine app intents ... refused everything else"; "outside the nine intents"): one of the
  nine is `unsupported` (`datasets/contracts/intents.txt`). The report now says "nine intents: eight app actions and
  unsupported". **Fix:** "among nine intents (eight app actions and *unsupported*)".
- **Report §6.1** ("only 221/280 were right, because many more requests were asked about"): of the 35 extra right
  decisions with the fallback, 24 come from fewer questions (37 → 13) and 15 from fewer requests sent the old way
  (20 → 5); 4 more skills not asked for (2 → 6) offset them. **Fix:** "...because many more requests were asked about
  or sent the old way."

## Table: one row per correction

Sources are relative to `/home/Adi_mauryaji/Kestrel` (logs under `experiments/logs/v3/`); "vault" =
`/home/Adi_mauryaji/KYROS/KYROS/Projects/kestrel/`.

| # | Document | Passage now | Source | My value | Status |
|---|---|---|---|---|---|
| G1 | paper 8.3; report 6.3 | "took 4.9 s on average (at most 6.8 s)"; "took 4.9 s on average" | notes/qa_test.json `questions[].s`; qa_dev.json | test 4.9472 → 4.9, max 6.78 → 6.8; dev 5.1385 → 5.1, max 8.81 → 8.8 | OK |
| G2 | paper 8.6; report 4.5; slide 9; notes 9 | "(2103 MiB, 64.8%)"; "64.8% less"; "−64.8%" | mmap/20261026T140055Z.jsonl measured rows, medians | 3247.05 − 1143.71 = 2103.34; 64.78% → 64.8% | OK |
| G3 | paper 8.1 | "below τ, Gemma was right 30 times out of 38, the head 19 times" | kinds/fallback_outputs_v2.jsonl val, max head < 0.8 | 38 / 30 / 19 (v1 at 0.7: 24 / 18 / 15 = log) | OK |
| G4 | paper 7 | "Kinds-v2 added 32 families: 12 code, 6 chat ..., 6 notes, 5 rewrite and 3 app" | datasets/kestrel_kinds_v2/families.toml; splits v2 − v1 | 32 = 12 / 6 / 6 / 5 / 3, both ways | OK |
| G5 | paper Table 5; report Table 4 | rows "..., first harness and checks (dev)"; header "gen. s / request" | code/*.json; dev_rescored_review.json `strict_before` | 19, 17, 23 = strict_before; 23; 21; gen s 11.1 / 10.4 / 29.0 / 9.1 / 8.9 | OK |
| G6 | paper 8.7; report 6.5; slide 11 | "17:31", "17:31:02" with a plain colon | acceptance/README.md; numbers.tex `17{:}31` | as logged; renders without spaces | OK |
| P1 | paper 4 | "(84 to 103 items) made validation ECE worse in four of five development runs" | kinds/dev_v0..v4.json `val_raw.ece` / `val_calibrated.ece`, `cal_n` | worse in v1–v4, better in v0; cal 84–103 | OK |
| P2 | paper 8.1 | as G3 | as G3 | 38 / 30 / 19 | OK |
| P3 | paper 8.4 | "all of Gemma's misses were scripts (1/6 right)"; causes not claimed | code/dev_gemma-4-e2b.json | 5 misses, all scripts; script 1/6 | OK |
| P4 | paper 10 | "Every set is small (20 to 280 test items)" | kestrel_notesq_v0 manifest; test logs | 20, 24, 25, 256, 280 | OK |
| P5 | paper 1 (item 6) | "five small English–Hinglish sets with one test run each" | Table 1; test logs; kinds_v0 test ⊂ kinds_v1 test (240/240) | 5 tested once; Kinds-v0 never tested | OK |
| P6 | paper Fig. 2 caption | "turns many questions to the user, and some requests sent the old way, into right answers. On Kinds-v1 a third ... sent the old way" | fallback_test_v1/v2.json `by_gold`; kinds.rs `decide_fallback` (asked ⊆ asked alone) | v1: 6 questions + 7 old-way → right (+13); v2: 23 + 15 (+38); 25/76 = 32.9% | OK (on v1 the old-way share is slightly larger) |
| P7 | paper 9 | "questions to the user fell from 37 to 13 on Kinds-v2, from 18 to 11 on Kinds-v1" | `all.*.asked` and summed `by_gold` | 37 → 13; 18 → 11 | OK |
| P8 | paper 9 | "by far the most common failure (25, against 0 ...); ... close (5 and 6). ... since it usually ends in a refusal" | fallback_test_v*.json; vault DECISIONS ADR-037 | counts right; the refusal is in no log | **PROBLEM** (1, item 3) |
| P9 | paper 8.3 | "4.9 s" | as G1 | 4.9472 → 4.9 | OK |
| P10 | paper 8.6 | "64.8%" | as G2 | 64.78% | OK |
| P11 | paper 9 | "rose from 40/76 ... to 78/96 ..., but new families, a new Gemma prompt and new thresholds came together, and no run separates their effects" | configs/kinds_fallback_v1/v2.json (`system` differs; τ/τg 0.7/0.9 → 0.8/0.7) | as stated | OK |
| P12 | paper 7 | "Timings come from the laptop as found (AC power, CPU boost on, Kestrel's own service idle)" | mmap/README.md; worker/README.md; acceptance/README.md | the conditions are recorded for mmap only | **PROBLEM** (minor, item 10) |
| P13 | paper Reproducibility | "this paper and its generator were added in a later commit ... Every result number in this paper is generated ..." | git status, `v3-frozen` = HEAD = ad920d5; kestrel_v3.tex | not committed; several result values typed | **PROBLEM** (item 1) |
| P14 | paper abstract | "cut ... from 3247 to 1144 MiB, and, with a cached prompt prefix as well, ... ready in 2.0 s instead of ... 19.1 s" | worker/20261026T142304Z.tsv; mmap jsonl | medians v2 19.13, v3 hit 2.02; 3247 / 1144 | OK |
| P15 | paper abstract; 1 (item 1) | "a skill is proposed to the user as a question (a request that leans to the app takes the old path)" | kinds.rs `decide`, `decide_fallback` | Ask only for a skill; app → App | OK |
| P16 | paper Fig. 1 caption | "Saving a note (read from the text as typed) and task commands are checked first" | assistant.rs `respond`: `notes_save::parse(text)`, `commands::parse(&req.text)` | note save on raw text; commands on the normalized text with aliases resolved | OK |
| P17 | paper 3 | "streamed as they are written, except notes answers, which are shown after the grounding check" | notes_qa.rs header; service.rs `write_notes_answer`; summary.rs `ModelText` | summaries are also not streamed | **PROBLEM** (minor, item 4) |
| P18 | paper 4 | "added in three dataset versions, Kinds-v0 to v2; Table 1 lists the two that were tested" | Table 1; kinds_v0 README | as stated | OK |
| P19 | paper 6 | "Qwen3-4B's own checks failed on right code (wrong asserts, or asserts written where a sample input was expected) and caused 8 retries without changing a single outcome" | code/dev_qwen3-4b.json attempts; first harness `0a83608` `self_check` | 32 − 24 = 8: 4 AssertionError (fn-second-largest ×2 languages), 4 ValueError on assert text read as input (sc-celsius/en, sc-even-odd/en); pass1 = pass_loop on all 24 | OK |
| P20 | paper 6 | "The evaluation harness (Section 8.4) checks by request type instead: a function or a fix is imported, a script is only compiled." | `git show 70a0a31:experiments/v3/code_eval.py` `self_check` (= HEAD) | as stated; service (code.rs `check`) imports, compiles only on EOFError | OK |
| P21 | paper 7 | Kinds-v2 families by kind | as G4 | 32: 12 / 6 / 6 / 5 / 3 | OK |
| P22 | paper Table 5 caption; 8.4 | "mean generation seconds per request (model time; wall time is slightly longer)"; "Generation took 8.9 s per request" | code/*.json attempt `ms`, `wall_s` | gen 11.1 / 10.4 / 29.0 / 9.1 / 8.9; wall 11.2 / 10.5 / 29.1 / 9.2 / 9.0 | OK |
| P23 | paper 8.3 | "declined only 4/6 unanswerable questions, both before and after the language note" | notes/qa_dev_first.json (24 q), qa_dev.json (26 q) | 4/6 both; misses qa-none-cake-price en and hi-en in both | OK |
| P24 | paper 8.7 | "17/36 (47.2%) ... under a second: the greeting, creating a task, task commands, ... a refusal (median 0.3 s)" | acceptance/socket.json `s` | 17/36 = 47.2%, median 0.3; includes "delete task 3" (not a task command) | **PROBLEM** (minor, item 9) |
| P25 | paper 9 | "the Kinds-v2 head's test ECE was 0.072 (validation: 0.055)" | kinds/fallback_test_v2.json `head.ece`; final_kinds_v2 | 0.07196 → 0.072; val 0.059 → 0.055 | OK |
| P26 | paper 12 | "who has the link or can read its traffic; it is off by default and was off when this paper was written" | vault DECISIONS ADR-047; CHANGELOG 2026-10-27 "wifi sharing turned off" | either is enough; off | OK |
| P-cit | paper 2 | "HumanEval ... and MBPP ... judge generated Python by running it against unit tests" | citation_check_v3.md | wording as recommended | OK |
| R1 | report 3 | "found 1 note: - 2026-09-08-mom-birthday.md (by meaning)" | acceptance/socket.json case 13 `text` | "found 1 note:\n- 2026-09-08-mom-birthday.md (by meaning)" | OK |
| R2 | report Fig. 2 (= paper Fig. 1) | box "list, done, rename, reschedule, set a time" | commands.rs `Command` (List, Done, Rename, Reschedule, At); delete → `None` (cascade) | as drawn; the diagram matches `respond` and `decide_fallback` | OK |
| R3 | report App. B | "paper/v3/number_check.md records an independent check of every number" | paper/v3 listing | the file exists and points to the three checks | OK |
| R4 | report 4.5 | "fell from 3247 to 1144 MiB (64.8% less)" | as G2 | 64.8% | OK |
| R5 | report 6.3 | "4.9 s on average" | as G1 | 4.9 | OK |
| R6 | report 6.1 | "5 skill requests were sent the old way (the app path)" | fallback_test_v2.json | 5; "(and refused)" gone | OK |
| R7 | report 1; 2 | §1 "nine intents: eight app actions (...) and unsupported, which it refused"; §2 "A request outside its eight actions ... usually got I can't do that, or ended in a wrong action" | datasets/contracts/intents.txt; ADR-037 | §1 right (8 + unsupported); §2 conflicts with the v2 list just before it | **PROBLEM** (minor, item 8) |
| R8 | report 1 | "writing and fixing small Python programs, checked in a sealed sandbox, and explaining code" | kinds_fallback_v2 rule 5; socket case 31 | explaining is chat, not sandboxed | OK |
| R9 | report Table 1 note | "Measured with the evaluation's own explain prompt; in the service, explaining code is a chat answer." | code_eval.py `EXPLAIN_SYSTEM`; kinds.rs | as stated | OK |
| R10 | report 3 | "When it finds fewer than three notes, the note closest in meaning is added if it is close enough" | act.rs `WithMeaning` (room = min(limit, k)); notes_meaning_v0.json k = 3, added = 1; tools.rs default limit 10 | as stated | OK |
| R11 | report 3 | "although the keyword search, which needs every word of the query in a note, found nothing" | kestrel-tools notes.rs `search` (every term must occur) | as stated | OK |
| R12 | report 3 | "appear as they are written, except answers from the notes, which appear once they have been checked" | as P17 | summaries are also held back | **PROBLEM** (minor, item 4) |
| R13 | report 3 | "a request to translate into Hinglish often came back in English (1/4 on development items)" | writer/lang_final.json translate-hinglish | 1/4; the 3 misses came back as the English text | OK |
| R14 | report 4.2 | "Kestrel asks, for example Write this as an email or message draft?; if it is app, the request takes the old path" | kinds.rs `Kind::question`, `decide` | exact text | OK |
| R15 | report 5 | "the memory-mapped load and the saved prompt in the inference crate" | `git diff --stat stage2-frozen 86cb3a7 -- crates/` | llama.rs +150 (prime_cached, mmap), tier.rs +28 | OK |
| R16 | report 6.2 | "In two of the misses the right note was the closest but just below the threshold, so nothing was added; for “festival shopping” the closest note was the Goa trip note ..." | notes/meaning_test_translated.json | goa 0.8283, 0.8298 < 0.83; festival en + hi-en: Goa note 0.8131 < 0.83 | **PROBLEM** (minor, item 7) |
| R17 | report Table 4 | "..., first harness and checks (dev)" | as G5 | as G5 | OK |
| R18 | report App. A | "ADR-044 Qwen3-4B for code (superseded by ADR-046); ..." | vault DECISIONS ADR-044, ADR-046 | ADR-046 supersedes ADR-044 (1) | OK |
| R19 | report App. B | "Every result number in this report (each one typeset from a generated value) ...; E0, E1, PREREG named" | kestrel_v3_report.tex | typed counts in the prose; E1 unused | **PROBLEM** (item 2) |
| F1 | notes 8 | "As it writes code, it also tries it inside a sealed box ... at most twice" | code.rs `write` (stream, then `check`), `RETRIES` = 2 | the check runs after the code is written | **PROBLEM** (minor, item 6) |
| F2 | slide 8 title; Likely questions | "It writes small Python code, and checks it in a sealed box"; "code it writes as code is tried in the sealed box" | code.rs; README limitation 3 | right | OK |
| F3 | slide 10; notes 10 | "Under a second for most of Kestrel's own tools ...; some Hinglish requests take seconds"; "(0.3 seconds for this reminder) ... (4.8 for a Hinglish reminder)" | socket.json | 17 of 22 own-tool requests (no model-written text) < 1 s; over: Hinglish 4.8, 1.7, 2.6, plus 1.1 and 1.4 | OK |
| F4 | slide 10; notes 10 | "Written answers appear word by word; notes answers once checked." | as P17 | the summary on the same chart is held back | **PROBLEM** (minor, item 4) |
| F5 | slide 7; notes 7 | "A small test of the answering step ... Spotting that a question is about your notes is a separate step: 12/16 on its test." | qa_test.json; fallback_test_v2.json `by_gold.notes.head_gemma` | 25/25; 12/16 | OK |
| F6 | slide 8; notes 8 | "21/24 requests passed our checks"; "Explanations were checked for their key points only." | code/test_gemma-4-e2b.json; codetasks test `facts` | 21/24 | OK |
| F7 | notes 1 | "Nothing you type is sent over the internet." | ADR-047; CHANGELOG (sharing off) | true; the tagline was kept on purpose | OK |
| F8 | slide 11; notes 11 | "448 automatic checks of Kestrel's code passed, and 510 of the scripts that test it" | acceptance/README.md; pytest.ini `testpaths = experiments` | 448 cargo, 510 pytest | OK |
| F9 | slide 12; Likely questions | "The main ones; the full list written down at the freeze has 12."; "lists the main things we know go wrong" | acceptance/README.md limitations | 12 | OK |
| F10 | slide 9; notes 9 | subtitle "maps the model file instead of copying it, so the laptop can take most of that memory back"; bars "copied in" / "from disk" | mmap/README.md finding 2; mmap jsonl | subtitle right (2702 of 3846 MiB reclaimable); the "from disk" label is left over | **PROBLEM** (minor, item 5) |
| F11 | notes 6 | "with its cut-off tuned again on the same searches, it found 9/9 ... 14/18 times (7/9 of the Hinglish ones) ... 0/2" | notes/meaning_dev_translated.json (θ 0.85: Hinglish 7/9; 0.83: 9/9); meaning_test_translated.json | as stated | OK |
| F12 | notes 5 | "like treating 'london mein abhi kitne baje hain?' as a question about your notes" | fallback_test_v2.json `wrong[]` | gold app, taken as notes (head 0.911) | OK |
| F13 | notes header; Likely questions | "Every measured number ... filled in from numbers.json; every quoted request and answer was checked word for word" | speaker_notes.template.md, deck.template.html; qa_test.json, socket.json | measured values all `{{key}}`; quotes match | OK |
| F14 | notes 11 | "I'll come to the two requests that failed on the next slide." | notes 12 | both covered in notes 12 | OK |
| F15 | slide 5; notes 5 | "It asks you “Write this as an email or message draft?”"; "When both are unsure but the first look leans to Kestrel's everyday tools, it just uses them." | kinds.rs `Kind::question`, `decide` | exact text; app → old path | OK |
| F16 | slide 5; notes 5 | box "a small model: a few milliseconds"; "measured 5.6 milliseconds in our first version" | E1/analysis_intents_v0_test.json R1 latency median | 5.623 → 5.6 | OK |
| F17 | notes 7 | "This test is small, 25 questions" | qa_test.json | 25 | OK |
| F18 | Likely questions | "In our tests it handled everyday personal tasks well (34/36 ...), but the tests were small" | socket.json summary | 34/36 | OK |

## Spot checks of tables and figures (all OK)

- Paper Table 2 = report Fig. 4 bars = paper Fig. 2 bars: all 20 cells match the `by_gold` sums. Percentages: 80.5, 85.9,
  78.9, 91.4. Wilson intervals: [81.1, 89.7], [87.6, 94.2]; 23/24 [79.8, 99.3].
- Paper Table 3 = report Table 3: all 28 per-kind cells match `by_gold.*.right / n`.
- Code table (paper Table 5, report Table 4): see G5. Paper Fig. 3 bars: dev 6/6, 5/6, 6/6, 6/6; test 6/6, 3/6, 6/6,
  6/6. Wilson for 21/24: [69.0, 95.7].
- Paper Table 4 (notes): 7/18 (6/9, 1/9, 0/4) at θ 0.85; 18/18 (9/9, 9/9, 0/4) at θ 0.83; test 14/18 (7/9, 7/9, 0/2);
  rank-1 11, 18 and 16. Paper Table 6: load 3656 / 2596 / 2477 / 1383 ms; first token 111 / 103 / 111 / 117 ms; anon
  3247 / 3247 / 1144 / 1144; file 8 / 8 / 2702 / 2702.
- Architecture diagram (paper Fig. 1 = report Fig. 2, one file, `figures/arch.tex`):
  - The order in the diagram is the order in `assistant.rs` `respond`: note save on the raw text, then task commands,
    then `kinds.handle`, then the cascade.
  - Task commands = commands.rs `Command`.
  - Head ≥ τ: app or skill. Below τ: Gemma's kind if ≥ τg, else the head's top kind: app → App, skill → Ask (y/N).
    This is `decide_fallback` and `decide` exactly.
  - Notes: N = 2 (`notes_qa::N`). Code: at most 2 retries (`code::RETRIES`).
  - Two steps are not drawn, which is fine for a simplified figure: the alias/correction step (`personal.before`), and
    what happens after a "no" (the app path).
