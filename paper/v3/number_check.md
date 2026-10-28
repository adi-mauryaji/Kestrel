# Number and claim check · Kestrel v3 paper, report and slides

Date: 2026-10-28. Every number in the three documents is generated from the committed logs by
`experiments/analysis/paper_assets_v3.py` (the paper and report use `\kv{key}`, the slides and speaker notes
`{{key}}`); `experiments/analysis/test_paper_assets_v3.py` fails if a document asks for a key the script does not
generate, and the script stops if a logged summary disagrees with the same value counted again from the log's rows.

On top of that, three independent agents re-derived every number and checked every factual claim from the raw logs,
datasets and code, without reading the generator first (their full tables are in the files below). A fourth checked
the six new references against primary sources.

| Document | Check file | Items | OK | Not OK |
|---|---|---|---|---|
| Paper, `kestrel_v3.pdf` | `number_check_paper.md` | 214 | 188 | 8 mismatch, 2 rounding, 3 unsupported, 13 wording |
| Report, `kestrel_v3_report.pdf` | `number_check_report.md` | 208 | 188 | 3 mismatch, 2 rounding, 1 unsupported, 14 wording rows (13 distinct) |
| Slides + speaker notes | `number_check_slides.md` | 153 | 123 | 1 mismatch, 29 wording rows (18 fixes) |
| References | `citation_check_v3.md` | 7 | 5 | 1 wording fix (MBPP), 1 claim checked through a secondary source (Wilson 1927) |

No number generated from a per-item log was wrong. The findings were sentences that said more than the logs show,
two values rounded twice or to a different precision, two values taken at the wrong threshold or range, and one count
of runs. Each was fixed as listed below, and the PDFs, slides and notes were rebuilt.

## What changed after the checks

### In the generator (affects all documents)

- `qa.*.s.mean`, `qa.*.s.max`: from the per-question times (test: 4.947 s → 4.9), not by rounding the log's
  already-rounded mean again (4.95 → 5.0).
- `mmap.anon.cut`: one decimal like every other share (64.8%, was 65%).
- `fb*.unsure.*`: the validation items below the *chosen* τ, counted from `fallback_outputs_v*.jsonl` (Kinds-v2 at τ
  0.8: 38 items, Gemma right 30, head 19; the log's `val_head_unsure` is at the draft τ 0.7). For Kinds-v1, whose
  chosen τ is 0.7, the count is checked against the log's own field.
- New keys `kv2.fam.*` (Kinds-v2's 32 new families by kind, from `families.toml`).
- Table of code results: "first harness and checks" rows, "gen. s / request" header.
- Clock times typeset with an ordinary colon (17:31, not 17 : 31).

### Paper (`number_check_paper.md`, items 1–26)

1 "three of five" runs → four of five. 2 unsure items at the chosen τ (generator). 3 Gemma's first-harness misses: all
scripts (causes not claimed). 4 set sizes "20 to 280". 5 "five" sets with a test run. 6, 7 the fallback "turns many
questions, and some requests sent the old way" into right answers, with the question counts (37 → 13, 18 → 11). 8 old
way vs wrong skill given per dataset. 9, 10 roundings (generator). 11 the Kinds-v1 → v2 gain no longer attributed to
data alone. 12 timing conditions: "Kestrel's own service idle". 13 reproducibility: this file's path, result numbers
vs typed constants, paper added after the tag. 14 abstract: mapped weights and the cached prefix together give 19.1 →
2.0 s. 15 a skill is proposed as a question; an app-leaning request takes the old path. 16 only saving a note reads the
text as typed. 17 notes answers are not streamed. 18 Kinds-v0 to v2, two tested. 19 Qwen3-4B's "own checks". 20 the
harness checks by request type. 21 Kinds-v2's families by kind (generated). 22 "generation seconds". 23 unanswerable
4/6 before and after the language note. 24 the full list of sub-second requests. 25 test ECE "was" 0.072. 26 the
wifi page: "has the link or can read its traffic". Also fixed from the citation check: MBPP and HumanEval "run it
against unit tests".

### Report (`number_check_report.md`, items 1–19)

1 the dash in the quoted search answer restored. 2 Figure 2's task-command box: "set a time" instead of "delete"
(delete goes through the cascade and asks y/N; the same diagram is the paper's Figure 1). 3 this file now exists. 4, 5
roundings (generator). 6 "(and refused)" dropped. 7 v1: nine intents = eight actions + unsupported; such requests
"usually" got the refusal. 8 explaining code is not sandbox-checked. 9 explanations measured with the evaluation's own
prompt (footnote). 10 a meaning note is added when keywords find fewer than three. 11 the keyword search, not the
title, is what found nothing. 12 notes answers appear once checked. 13 Hinglish translation caveat
(`writer.final.translate-hinglish`). 14 the real question wording. 15 the inference crate named. 16 "closest but below
the threshold". 17 code table labels. 18 ADR-044's superseded model choice. 19 "every result number", and the E0, E1
and PREREG sources named.

### Slides and speaker notes (`number_check_slides.md`, F1–F18)

F1, F2 code is checked once it is written (it is shown as it is written), not before it is shown; slide 8 title "checks it in a sealed box". F3 "most"
own-tool requests under a second; some Hinglish ones take seconds. F4 notes answers appear once checked. F5 25/25 is
the answering step; recognising a notes question is a separate step (`kt2.hg.kind.notes`). F6 "passed our checks";
explanations checked for key points only. F8 448 checks of Kestrel's code, 510 of the scripts that test it. F9 slide
12 says it lists the main limitations of 12. F10 the model file is mapped, not "read straight from disk". F11 the
Hinglish 9/9 came with a re-tuned cut-off; the test's Hinglish result named. F12 the "nobody asked for" example. F13
"every measured number" is filled in; quotes checked word for word. F14, F17, F18 wording. F15 the real question text;
an app-leaning request is not asked. F16 the first look is "a few milliseconds" (5.6 ms was v1's encoder alone).
F7: speaker notes now say "nothing you type is sent over the internet". Not changed: the claim that the installed
service shares the page on the wifi is out of date: sharing was turned off on 2026-10-27 (vault CHANGELOG), and the
report check found no `--web-lan` in the installed service unit. The product's own tagline ("Nothing you type leaves
it.") stays on the slides.

## Values with no raw log behind them

These come from a log folder's README, written when the measurement was made, and were taken as recorded: the
1132-token prompt prefix and the RssAnon at ready (3356 / 1135 MiB) in `experiments/logs/v3/worker/README.md`; the
rent question's head score 0.62, the automated test counts (448, 510, 6, 4) and the reminder times (17:29:29, 17:31:02)
in `experiments/logs/v3/acceptance/README.md`. The Gemma label time (523 ms) is a development timing on the laptop as
found, which its log labels as not a controlled measurement. Wilson's 1927 paper is closed access; the interval's
attribution was checked through a secondary source quoting it (`citation_check_v3.md`).

## Second pass on the corrections

A fifth agent re-checked all 70 corrections in the final documents (`number_recheck.md`): 58 OK, no number wrong, 12
wording problems, and 5 more outside the list. All were fixed: the paper's and report's statements about which numbers
are generated now name the few small counts typed in the prose; the old way is described as refusing a request "or
taking it for another action" (no log records which); note summaries, like notes answers, appear only after the
grounding check; slide 9's bars read "copied" / "mapped"; speaker notes §8 say the code is tried once written; the
report's festival-shopping miss and its Figure 2 caption; the paper's sub-second list includes deleting a task (not a
task command); its timing conditions are attributed to the memory runs; its retrieval rule is "fewer than three
notes"; v1 is described as nine intents, eight app actions and unsupported; and the report's head-alone shortfall names
both questions and old-way decisions.

