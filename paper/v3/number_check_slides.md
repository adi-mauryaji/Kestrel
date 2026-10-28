# Number and claim check: Kestrel v3 slides and speaker notes

**Date:** 2026-10-28
**Checked:** `paper/v3/slides/kestrel_v3_slides.pdf` (14 slides: the text and the charts) and
`paper/v3/slides/speaker_notes.md` (every section, including "Likely questions").
**Against:** the raw logs, the datasets and the product code only:

- `experiments/logs/v3/acceptance/` (`socket.json`, `page_headless.json`, `page_headed.json`, `page_headed.png`, `README.md`)
- `experiments/logs/v3/kinds/` (`fallback_test_v2.json`, `fallback_dev_v2.json`, `fallback_outputs_v2.jsonl`, `fallback_test_v1.json`)
- `experiments/logs/v3/notes/` (`meaning_dev*.json`, `meaning_test_translated.json`, `qa_dev.json`, `qa_test.json`)
- `experiments/logs/v3/code/test_gemma-4-e2b.json`, `experiments/logs/v3/mmap/`, `experiments/logs/v3/worker/`
- `experiments/PREREG.md` and `experiments/logs/E1/analysis_intents_v0_test.json`
- the datasets: `datasets/kestrel_kinds_v{1,2}/splits/`, `datasets/kestrel_notesq_v0/notes/`, `datasets/kestrel_codetasks_v0/splits/test.jsonl`
- the code in `apps/cli/src/` (kinds, code, sandbox, notes_qa, grounding, meaning, summary, service), `experiments/v3/code_eval.py`, `pytest.ini`
- DECISIONS ADR-036..048 (read-only)

**How:**

- Text: `pdftotext -layout`. Every page was rendered with `pdftoppm -r 50` and looked at; slide 10 was also rendered at 110 dpi so the dot positions could be measured.
- Every value was recomputed from the raw files with short Python scripts.
- Every quoted request and answer was matched character for character against the built HTML's text.
- The bar and dot positions in the charts were measured in pixels against their axes.
- Machine facts were checked live (`lscpu`, `/proc/meminfo`, `lspci`), and so was the dependency tree (`cargo tree --offline -e normal -p kestrel-cli`: no network-client crate).
- `experiments/analysis/paper_assets_v3.py` was not read. `numbers.json` was opened only after every value below had been derived, and it explained no mismatch, because none was found.

Conventions: a share is shown as k/n, a percentage is rounded half up, and a median is a plain median.

**Result:** every number on the slides and in the notes matches its raw source. The problems are claims and
wording: one product claim in the notes is contradicted by the code (MISMATCH), and several statements say
more than the evidence shows (WORDING).

| Status | Rows |
|---|---|
| OK | 123 |
| WORDING | 29 |
| MISMATCH | 1 |
| ROUNDING | 0 |
| UNSUPPORTED | 0 |
| **Total** | **153** |

Notes rows that repeat a slide's claim are counted again, because the presenter says them.

---

## Non-OK items and the fix for each (highest impact first)

**F1 · MISMATCH · Notes §8: "Before it shows you code, it tries the code inside a sealed box."**
`apps/cli/src/code.rs` streams the code to the user while Gemma writes it, and runs the sandbox check after
that. Its module doc says so: "The user sees the code as it is written, then what the check found, and a caveat".
Both page runs show the code box first and the sandbox line after it. If the check fails, the user has already
seen the broken code, then sees "(It had an error in the sandbox check: … Fixing it.)".
Fix: "As it writes code, Kestrel also tries it inside a sealed box: no internet, no access to your files, and
limits on time and memory. It tells you what the check found, and if the code breaks there, it tries again, at
most twice."

**F2 · WORDING · Slide 8 title "It writes small Python code, and tries it in a sealed box first", and Likely
questions "code is tried in the sealed box".**
- "First" is wrong for the same reason as F1.
- README limitation 3 (and ADR-048): "write a program to solve X" goes to chat, so the whole program is shown
  with no sandbox check at all.

Fix:
- Slide 8 title: "It writes small Python code, and checks it in a sealed box".
- Likely questions: "code it writes as code is tried in the sealed box".
- Ideally add limitation 3 to slide 12 (see F9).

**F3 · WORDING · Slide 10 "Under a second for the things Kestrel does with its own tools, like tasks and
searches." Notes §10: "Things Kestrel does with its own tools, like a reminder or a search, take 0.3 seconds."**
The same `socket.json` run disagrees:
- The Hinglish reminder took 4.8 s (case 2), and slide 4 shows that number.
- "kl subah 9 baje gym jana yaad dilana" took 1.7 s (case 3).
- The Hinglish search by meaning took 2.6 s (case 12), on this same chart.

Fix:
- Slide 10: "Under a second for most things Kestrel does with its own tools, like a reminder or a keyword
  search. Some Hinglish requests take a few seconds."
- Notes §10: "An English reminder or a keyword search took 0.3 seconds. The Hinglish reminder on slide 4 took
  4.8, and a Hinglish search by meaning 2.6."

**F4 · WORDING · Slide 10 "You start reading early: The page shows the words as they are written." Notes §10:
"… so you start reading long before the end."**
This holds for chat, emails, rewrites, translations and code. It does not hold for "Answer from your notes"
(9.4 s on this chart): `service.rs` `write_notes_answer` says "not streamed, since the caller shows it only once
it is checked". Summaries (7.9 s) are also kept only after the grounding check (`summary.rs`).
Fix:
- Slide 10: "The page shows chat, emails, translations and code as they are written. Answers from your notes
  appear once they are checked."
- Notes §10: the same.

**F5 · WORDING · Slide 7 "25/25 test questions handled right" (and notes §7).**
The 25/25 is right for what it measured (`qa_test.json`: 19/19 + 6/6), but it overstates what a user gets:
- It tests the answering step on questions that were already sent to the notes. End to end, a question must
  first be recognised as a notes question, and that step got 12/16 on its test
  (`fallback_test_v2.json` by_gold.notes.head_gemma.right). This is why "how much is my rent?" was refused in
  the acceptance run (slide 12).
- The test ran at 177122b, before e12cb18 changed the prompt for Hinglish questions. After the change, on the
  practice set (`qa_dev.json`), 4 of 6 questions the notes don't answer were declined, not 6/6.

Fix:
- Slide 7: change "A small test, on sample notes." to "A small test of the answering step, on sample notes.
  Spotting that a question is about your notes is a separate step: 12/16 on its test."
- Notes §7: add "This counts questions once they reach the notes. Recognising a notes question is a separate
  step, right 12 of 16 times on its test, which is why 'how much is my rent?' was refused."

**F6 · WORDING · Slide 8 "Explanations 6/6" and "21/24 requests right" (notes §8: "In all, 21/24 requests
right").**
- The explain items are judged only by key words (`facts` in `codetasks_v0/splits/test.jsonl`).
- The harness made the explanations with its own prompt (`code_eval.py` `EXPLAIN_SYSTEM`). In the app,
  explanations are answered by chat, with a different prompt (kinds.rs; ADR-044 (2)).
- All 3 English requests (ex-*/en/0) were answered in Hinglish, although that prompt asks for English.
- ex-list-comp/hi-en/0 states a wrong output ("0, 4, 16, … 324"; the code gives [0, 4, 16, 36, 64]).

All of these were graded "right".
Fix:
- Slide 8: "21/24 passed our checks", plus a footnote: "Explanations were checked only for their key points."
- Notes §8: "In all, 21 of 24 passed our checks. Explanations were only checked for their key points."

**F7 · WORDING · Slides 1, 2 and 14, notes §1 and §14: "Nothing you type leaves it" / "The question, the notes
and the answer stay on this laptop" / "Your words stay on your laptop" / "Nothing you type is sent anywhere."**
This holds when Kestrel is used on the laptop itself. However:
- ADR-047 shares the page on the wifi over plain HTTP: "anyone on that wifi can read the traffic, key
  included".
- The acceptance README reports the installed service as reachable "on the wifi", and lists this as
  limitation 11. Slide 12 leaves it out.

Fix:
- Keep the headlines.
- Add to slide 12: "Sharing on the wifi is not private yet. When the page is used from another device, what
  you type there crosses the wifi unencrypted."
- Notes §1: "Nothing you type is sent over the internet."

**F8 · WORDING · Slide 11 "448 + 510 automatic checks of the code passed" (and notes §11).**
- 448 is `cargo test --workspace`, which tests Kestrel's own code.
- 510 is `pytest`. Its testpaths is `experiments` (`pytest.ini`), so these tests check the Python scripts that
  build, run and score the tests, not the app.

Fix: "448 automatic checks of Kestrel's code passed, and 510 of the scripts that test it."

**F9 · WORDING · Slide 12 as the list of limitations. Likely questions: "Slide 12 lists what we know goes
wrong."**
README lists 12 limitations, and the slide has 6 of them. These are missing:
- #2 preferences (said only in the notes)
- #3 code via chat, with no sandbox check
- #4 rewrite requests often asked about first (8/20 taken directly)
- #6 jokes go to chat, and "time in London" goes to notes
- #10 code once missing on the page
- #11 the wifi page uses plain HTTP

#3 and #11 matter because other slides claim the opposite (F2, F7).

Fix:
- Add #3 and #11 to slide 12. Alternatively, title it "The main things it can't do well yet".
- Likely questions: "Slide 12 lists the main things we know go wrong; the full list of 12 is in the test
  report."

**F10 · WORDING · Slide 9 subtitle "Version 3 reads the model straight from the disk instead of copying it into
memory" (and notes §9).**
`mmap/README.md` finding 2: llama.cpp maps the file with `MAP_POPULATE` and reads every tensor at load. The
whole file, 2702 MiB, sits in memory as file-backed pages. Peak VmHWM is higher with mapping (3.5–3.8 GiB)
than with copying (3.2 GiB). What fell by 65% is the memory that cannot be given back, as the chart title says.
Fix:
- Subtitle: "Version 3 maps the model file instead of copying it, so the laptop can take most of that memory
  back when it needs it."
- Notes §9: the same.

**F11 · WORDING · Notes §6: "The fix was simple: first turn the Hinglish search into English, then look. After
that it found 9/9."**
The 9/9 also depends on the cut-off being re-chosen on the same 9 searches (θ 0.85 → 0.83). With translation at
the old θ 0.85, it finds 7/9 (`meaning_dev_translated.json`: doctor 0.8496 and kyc 0.8497 fall below). On the
test set, the Hinglish searches found 7/9.
Fix: "After that, with its cut-off re-tuned on the same searches, it found 9/9. On the separate test set it found
the right note 14/18 times (7 of 9 Hinglish searches), and when no note fitted, it added nothing: 0/2 wrong
notes."

**F12 · WORDING · Notes §5: "Only 6 times did it start something nobody asked for, like answering a joke with
chat."**
The count is right: 3 of the 6 were jokes sent to chat (`fallback_test_v2.json` wrong[]). But a joke told by the
chat model is what the user asked for. The error is the choice of helper: the kind prompt's rule 3 sends jokes to
Kestrel's own small talk.
Fix: use a clearer case from the same list: "like treating 'london mein abhi kitne baje hain?' (what time is it
in London?) as a question about your notes."

**F13 · WORDING · Notes header: "Every number here and on the slides is filled in from numbers.json …" Likely
questions: "Every number on these slides is copied automatically from the saved test logs; nothing is typed by
hand."**
These are hand-typed in the templates:
- in `deck.template.html`: the quoted exchanges with their numbers ("#2", "2026-10-28 18:00", "20 September",
  "14th … 18th", the add function)
- in `speaker_notes.template.md`: "6 pm" and "half a second"
- the machine line comes from PREREG, not from a test log

All of them were checked here and match the logs.
Fix:
- Header: "Every measured number here and on the slides is filled in from `numbers.json`. The quoted requests
  and answers were copied from the test logs and checked word for word (number_check_slides.md)."
- Likely questions: "Every measured number is filled in automatically from the saved test logs, and every
  quoted answer was checked word for word against them."

**F14 · WORDING · Notes §11: "The two requests that failed are on the next slide."**
Slide 12 shows only "how much is my rent?". "Which preferences have I set?" is only spoken.
Fix: "I'll come to the two requests that failed on the next slide." Alternatively, add the preferences item to
slide 12.

**F15 · WORDING · Slide 5 flow and notes §5: "If both are unsure, Kestrel simply asks you", with the quoted
question "Did you mean …?".**
- `kinds.rs` `decide`/`decide_fallback`: when both are unsure and the head leans to Kestrel's usual tools
  (`app`), it goes that way without asking. It also goes that way after a "no".
- The real questions are yes/no questions per kind, such as "Rewrite the text in your request?". 10 of the 13
  questions asked on the test were for rewrites.

Fix:
- Slide 5: under "It asks you", show “Rewrite the text in your request?”.
- Notes §5: "If both are unsure, Kestrel asks you, for example 'Rewrite the text in your request?'. If it leans
  towards its everyday tools, it just uses them."

**F16 · WORDING (low) · Slide 5 box "a small model: about 5.6 ms".**
5.6 ms is version 1's intent head, measured under PREREG settings (turbo off, pinned threads). v3's own first
look was never timed, and ADR-036 records "e5 ~10 ms per pass" for v3. The footnote discloses the source, and
the notes are already careful ("a few milliseconds; … 5.6 milliseconds in our first version").
Fix: box "a small model: a few ms". The footnote can keep "5.6 ms in version 1".

**F17 · WORDING (low) · Notes §7: "This test is small, 25/25 questions".**
Fix: "This test is small, 25 questions, so …"

**F18 · WORDING (low) · Likely questions: "It is good at everyday personal tasks".**
This rests on small tests we wrote ourselves.
Fix: "In our tests it handled everyday personal tasks well (34 of 36 in the final run), but the tests were
small."

---

## Full table

Case numbers refer to `socket.json` results[i], counted from 0. "acc" is `experiments/logs/v3/acceptance/`.

| # | Slide / notes | Claim as shown | Value | Source | Independent value | Status |
|---|---|---|---|---|---|---|
| 1 | Slide 1 | "An assistant that lives on your laptop." | — | cargo tree (no network crate); models/MANIFEST.toml (local GGUF) | holds | OK |
| 2 | Slides 1, 2, 14; notes §1, §14 | "Nothing you type leaves it." / "…stay on this laptop" / "Your words stay on your laptop." / "Nothing you type is sent anywhere." | — | ADR-047; acc README limitation 11 and "Installed service … on the wifi" | holds on the laptop; the wifi page sends text over plain HTTP | WORDING (F7) |
| 3 | Slide 1 | "Version 3, October 2026" | Oct 2026 | ADR-048 (frozen 2026-10-27) | Oct 2026 | OK |
| 4 | Slide 2 | "Most assistants send your words to someone else's computer." | — | general knowledge (not in the logs) | plausible; not checkable against the logs | OK |
| 5 | Slide 2 | "No internet needed: Works with the wifi off." | — | cargo tree -e normal -p kestrel-cli (2026-10-28): no reqwest/hyper/ureq/curl/tls/tokio; ADR-035 (the page loads nothing from elsewhere) | no outbound network code; not re-tested with the wifi off for v3 | OK |
| 6 | Slide 2 | "₹0 to use: No subscription and no paid service behind it." | ₹0 | ADR-036 (paid Jev API rejected); MANIFEST licences | ₹0 | OK |
| 7 | Slide 2 | "Open models: Small language models that run on the laptop itself." | — | MANIFEST: gemma-4-e2b-it apache-2.0; multilingual-e5-small MIT | holds | OK |
| 8 | Slide 3 | The 8 example requests | — | cases 1, 12, 26, 20, 22, 19, 28, 34 | all 8 verbatim in socket.json | OK |
| 9 | Slide 3 | "Says so when it can't: what's the weather in delhi today?" | — | case 34: "I can't do that. …" | refusal, passed | OK |
| 10 | Slide 3 | "in English or Hinglish"; the skills list | — | case 0 greeting; page welcome text | holds | OK |
| 11 | Slide 4 | "kal shaam 6 baje bill bharna yaad dila dena" → "created task #2: bill bharna (due 2026-10-28 18:00)" | — | case 2 request, text | identical | OK |
| 12 | Slide 4 | "answered in 4.8 s" | 4.8 | case 2 s | 4.8 | OK |
| 13 | Slide 4 | "maa ka birthday kab hai?" → "Maa ka birthday 20 September hai." "(from 2026-09-08-mom-birthday.md)" | — | case 26 lines | identical | OK |
| 14 | Slide 4 | "answered in 9.4 s" | 9.4 | case 26 s | 9.4 | OK |
| 15 | Slide 4 | "translate to hindi: good morning, see you tomorrow" → "सुप्रभात, कल मिलते हैं" | — | case 23 written | identical (checked in the built HTML) | OK |
| 16 | Slide 4 | "answered in 2.8 s" | 2.8 | case 23 s | 2.8 | OK |
| 17 | Slide 4 | "goa wale hotel ka naam kya hai?" → "The notes do not say this: so the reply is not available for this information" | — | case 27 | identical | OK |
| 18 | Slide 4 | "answered in 3.0 s" | 3.0 | case 27 s | 3.0 | OK |
| 19 | Slide 4 | "Word for word from the last full test before version 3 was frozen, on a copy of Kestrel with sample notes" | — | acc README "Where" and "The first socket run" | socket.json = the repeated full run on a scratch service with the 18 fixture notes | OK |
| 20 | Slide 5 | "a small model: about 5.6 ms" | 5.6 ms | E1 systems.R1.latency_ms.median | 5.623 → 5.6 (v1's head; v3 not timed; ADR-036 "~10 ms per pass") | WORDING (F16) |
| 21 | Slide 5 | "Gemma, a bigger model: about 523 ms" | 523 | fallback_dev_v2 gemma_ms.p50; plain median of 275 cal+val gemma.ms in fallback_outputs_v2.jsonl | 523.46 | OK |
| 22 | Slide 5 | "256/280 decisions right on the test set" | 256/280 | fallback_test_v2 all.head_gemma right, n | 256/280 | OK |
| 23 | Slide 5 | "13 times it asked first" | 13 | all.head_gemma.asked | 13 | OK |
| 24 | Slide 5 | "6 times it started something nobody asked for" | 6 | all.head_gemma.skill_not_asked_for | 6 | OK |
| 25 | Slide 5 | "280 test requests we wrote in English and Hinglish" | 280 | kinds_v2 manifest test.items; by_lang | 280 (en 140, hi-en 140) | OK |
| 26 | Slide 5 | "24 new, the rest used once before" | 24 | manifest test.fresh_items; fallback_test_v2 fresh.n; kinds_v1 test | 24 new; all 256 others = the Kinds-v1 test, run once (fallback_test_v1 n 256) | OK |
| 27 | Slide 5 | "Times: version 1's small model; Gemma on the practice set." | — | E1 R1; fallback_dev_v2 (cal+val) | holds | OK |
| 28 | Slide 5 | Flow: second opinion unsure → "It asks you" | — | kinds.rs decide_fallback / decide | asks only when the head's top kind is not app | WORDING (F15) |
| 29 | Slide 5 | Quoted question "Did you mean …?" | — | kinds.rs Kind::question | real: e.g. "Rewrite the text in your request?" | WORDING (F15) |
| 30 | Slide 6 | "maa ke liye tohfa": "a gift for mom", the note says "gift ideas", never "tohfa" | — | notes/2026-09-08-mom-birthday.md (grep: no "tohfa") | holds | OK |
| 31 | Slide 6 | Card "2026-09-08-mom-birthday.md, found by meaning" with its lines | — | case 12; fixture note | found by meaning (2.6 s); card is an excerpt (omits the tag line and "Dinner at home with mausi and the cousins.") | OK |
| 32 | Slide 6 | Chart "at first" 1/9 Hinglish found, practice set | 1/9 | meaning_dev.json (θ 0.85): best = its note and ≥ θ | 1/9 (q-interview only) | OK |
| 33 | Slide 6 | Chart "now" 9/9 | 9/9 | meaning_dev_translated.json (θ 0.83) | 9/9 | OK |
| 34 | Slide 6 | "Now it first turns a Hinglish search into English, then looks." | — | meaning.rs; ADR-040; notes_meaning_v0.json translate true | holds (queries with a common Hindi word) | OK |
| 35 | Slide 6 | "14/18 searches found the right note on the test set" | 14/18 | meaning_test_translated.json (θ 0.83) | 14/18 (en 7/9, hi-en 7/9) | OK |
| 36 | Slide 6 | "When no note fits, it adds none: 0/2 wrong notes added" | 0/2 | same file, the q-none-guitar en and hi-en queries | best 0.8007 and 0.7957 < 0.83: 0/2 | OK |
| 37 | Slide 7 | "on which date do i fly to goa?" → "Flights are booked for the 14th, and you will be back on the 18th." | — | qa_test.json qa-goa-flight/en/0 (not in socket.json) | identical | OK |
| 38 | Slide 7 | "goa wale hotel ka naam kya hai?" → the not-in-notes reply | — | case 27 | identical | OK |
| 39 | Slide 7 | "Before it shows an answer, Kestrel checks that the numbers and names in it are in your note." | — | notes_qa.rs; grounding.rs check_facts; service.rs write_notes_answer (not streamed) | holds (against the 2 closest notes) | OK |
| 40 | Slide 7 | "25/25 test questions handled right" | 25/25 | qa_test at_theta; grades by id | 25/25 (answering step; before e12cb18) | WORDING (F5) |
| 41 | Slide 7 | "Questions the notes answer: 19/19 right." | 19/19 | qa_test (ids not qa-none, grade right) | 19/19 | OK |
| 42 | Slide 7 | "Questions they don't: 6/6 times it said so." | 6/6 | qa_test (qa-none ids) | 6/6 (practice set after the change: 4/6) | OK |
| 43 | Slide 7 | "A small test, on sample notes." | — | qa_test questions_n 25; fixture notes | holds | OK |
| 44 | Slide 8 | "fix this function: def add(a, b): return a - b" → def add / return a + b, "(I checked it in a safe sandbox: no errors.)" | — | case 29 | identical (fences rendered as a box) | OK |
| 45 | Slide 8 | Title "…tries it in a sealed box first" | — | code.rs (streamed, then checked); acc README limitation 3 | shown before the check; code sent to chat is never checked | WORDING (F2) |
| 46 | Slide 8 | "No internet inside the box" | — | sandbox.rs bwrap --unshare-all | holds | OK |
| 47 | Slide 8 | "It can't see your files" | — | sandbox.rs: /usr read-only, empty /tmp, a fresh work folder, no /home | holds | OK |
| 48 | Slide 8 | "Limits on time and memory" | — | sandbox.rs prlimit --as 512 MiB, --cpu 10 s; code.rs 15 s wall timeout | holds | OK |
| 49 | Slide 8 | Functions 6/6 | 6/6 | test_gemma-4-e2b summary.function pass_loop | 6/6 | OK |
| 50 | Slide 8 | Fixes 6/6 | 6/6 | summary.fix | 6/6 | OK |
| 51 | Slide 8 | Explanations 6/6 | 6/6 | summary.explain; results ex-*; test.jsonl facts; code_eval.py EXPLAIN_SYSTEM | 6/6 by key words; English requests got Hinglish; one wrong output; not the app's prompt | WORDING (F6) |
| 52 | Slide 8 | Scripts 3/6 | 3/6 | summary.script | 3/6 | OK |
| 53 | Slide 8 | "21/24 requests right" | 21/24 | summary.all pass_loop | 21/24 (includes F6's 6) | WORDING (F6) |
| 54 | Slide 8 | Dot rows (6, 6, 6, 3 of 6 filled) | — | as rows 49–52 | match | OK |
| 55 | Slide 8 | "Python's built-in tools only. The box loads the code …; it does not run your program with your own input." | — | sandbox.rs python3 -E -S; code.rs check (import; a `__main__` block does not run; input code only compiled) | holds | OK |
| 56 | Slide 9 | "Ryzen 5 5500U processor, 6 cores, 14.95 GiB of memory" | — | PREREG "Machine:"; lscpu; MemTotal 15675416 kB | 5500U, 6 cores, 14.95 GiB | OK |
| 57 | Slide 9 | "no separate graphics card" | — | lspci (one VGA: integrated Lucienne); TECH_STACK "no GPU" | holds | OK |
| 58 | Slide 9 | "copied in 3247 MiB" | 3247 | mmap .md copy anon; .jsonl anon_kib 3324984 | 3247.1 → 3247 | OK |
| 59 | Slide 9 | "from disk 1144 MiB" | 1144 | map anon; anon_kib 1171164 | 1143.7 → 1144 | OK |
| 60 | Slide 9 | "−65%" | 65% | 1 − 1171164/3324984 | 64.78% → 65% | OK |
| 61 | Slide 9 | "Version 3 reads the model straight from the disk instead of copying it into memory." | — | mmap README findings 1–2 | the whole file is still read into memory (2702 MiB file-backed, reclaimable) | WORDING (F10) |
| 62 | Slide 9 | "version 2 19.1 s" | 19.1 | worker tsv v2 rows | median(19.30, 19.13, 18.01) = 19.13 → 19.1 | OK |
| 63 | Slide 9 | "version 3 2.0 s" | 2.0 | worker tsv hit rows | median(2.02, 2.04, 2.00) = 2.02 → 2.0 | OK |
| 64 | Slide 9 | "saves its warm-up on the disk once, instead of redoing it every time it starts" | — | worker README "Change" (13 MB KV file in the cache) | holds | OK |
| 65 | Slide 9 | Bar lengths | — | pixels at 50 dpi | 207/587 = 0.353 vs 0.352; 61/587 = 0.104 vs 0.105 | OK |
| 66 | Slide 9 | "All of this runs on an ordinary laptop" | — | acc README; PREREG machine | holds | OK |
| 67 | Slide 10 | Make a reminder 0.3 s | 0.3 | case 1 | 0.3 | OK |
| 68 | Slide 10 | Search notes for a word 0.3 s | 0.3 | case 10 | 0.3 | OK |
| 69 | Slide 10 | Find a note by meaning (Hinglish) 2.6 s | 2.6 | case 12 | 2.6 | OK |
| 70 | Slide 10 | Translate into Hindi 2.8 s | 2.8 | case 23 | 2.8 | OK |
| 71 | Slide 10 | Make a line polite 3.6 s | 3.6 | case 22 | 3.6 | OK |
| 72 | Slide 10 | Fix a small function 4.3 s | 4.3 | case 29 | 4.3 | OK |
| 73 | Slide 10 | Summarize a note 7.9 s | 7.9 | case 13 | 7.9 | OK |
| 74 | Slide 10 | Write an email (Hinglish) 8.5 s | 8.5 | case 20 | 8.5 | OK |
| 75 | Slide 10 | Answer from your notes 9.4 s | 9.4 | case 26 | 9.4 | OK |
| 76 | Slide 10 | Write four lines of poetry 10.3 s | 10.3 | case 19 | 10.3 | OK |
| 77 | Slide 10 | Write a new function 14.0 s | 14.0 | case 28 | 14.0 | OK |
| 78 | Slide 10 | Explain how something works 23.2 s | 23.2 | case 18 | 23.2 | OK |
| 79 | Slide 10 | Dot positions on the 0–25 s axis | — | pixels at 110 dpi (32.9 px/s) | every dot within 1 px of its value | OK |
| 80 | Slide 10 | "Under a second for the things Kestrel does with its own tools, like tasks and searches." | < 1 s | cases 2, 3, 12 | Hinglish task 4.8 s, 1.7 s; Hinglish search 2.6 s | WORDING (F3) |
| 81 | Slide 10 | "A few seconds or more when the language model writes, because it writes word by word." | — | cases 18–24, 26, 28, 29 | 2.8–23.2 s | OK |
| 82 | Slide 10 | "You start reading early: The page shows the words as they are written." | — | service.rs write_notes_answer ("not streamed"); summary.rs (kept only if grounded) | not for notes answers or summaries | WORDING (F4) |
| 83 | Slide 10 | "Seconds until the whole answer, one test run each." | — | socket.json s | holds | OK |
| 84 | Slide 11 | "34/36 everyday requests passed end to end" | 34/36 | socket.json summary | 34/36 (36 results, 34 ok) | OK |
| 85 | Slide 11 | "8/8 checks in a hidden browser" | 8/8 | page_headless.json passed/total | 8/8 | OK |
| 86 | Slide 11 | "8/8 on a real screen" | 8/8 | page_headed.json | 8/8 | OK |
| 87 | Slide 11 | "A reminder set for 17:31 popped up at 17:31:02." | 17:31, 17:31:02 | acc README table ("made at 17:29:29 … rang 17:31:02 … notification sent") | 17:31 / 17:31:02 (README only) | OK |
| 88 | Slide 11 | "448 + 510 automatic checks of the code passed." | 448, 510 | acc README; pytest.ini testpaths = experiments | 448 Rust (app); 510 Python test scripts | WORDING (F8) |
| 89 | Slide 11 | Screenshot "The Kestrel page during that test, on a real screen." | — | deck.template.html img = acc/page_headed.png | same image (Tuesday 27 October, 17:29, "pay the electricity bill", Fri 30 Oct 18:00) | OK |
| 90 | Slide 11 | Title "Tested like a real user, then frozen" | — | acc README; ADR-048 | holds | OK |
| 91 | Slide 12 | "how much is my rent? got 'I can't do that', though the rent is in the notes." | — | case 25; 2026-09-02-budget-plan.md "Rent 12000" | holds | OK |
| 92 | Slide 12 | "continue or a follow-up question does not see its last answer." | — | acc README limitation 12 | holds | OK |
| 93 | Slide 12 | "Bold text and bullet points appear with their stars." | — | limitation 7; page_headed.png | holds | OK |
| 94 | Slide 12 | "Scripts were right 3/6 times on the test set." | 3/6 | code test summary.script | 3/6 | OK |
| 95 | Slide 12 | "Notes answers came back in Hinglish 9/12 times on the practice set." | 9/12 | qa_dev.json answer_language.hi-en | 9/12 | OK |
| 96 | Slide 12 | "Every test set was made for this project. Real people's requests come next." | — | dataset READMEs/manifests; ADR-045, ADR-048 | holds | OK |
| 97 | Slide 12 | The slide as the list of what it can't do well | 6 items | acc README (12 limitations) | 6 of 12; #3 and #11 matter elsewhere | WORDING (F9) |
| 98 | Slide 13 | "Version 3 is frozen." | — | ADR-048 (tag v3-frozen) | holds | OK |
| 99 | Slide 13 | "Learn from real mistakes … test again on questions it has never seen." | — | ADR-048 Consequences (Kinds-v3 / CodeTasks-v1 from real requests, a fresh test) | matches the plan | OK |
| 100 | Slide 13 | "Close the gaps: Work through the list on the last slide …" | — | ADR-048 ("known limitations are the next round's input") | matches the plan | OK |
| 101 | Slide 13 | "Version 4 on a bigger machine: Larger models on a Mac or a stronger computer, still offline, still private." | — | ADR-036 (v4 = the Mac or a bigger machine; 8B+ models to v4) | matches the plan | OK |
| 102 | Notes header | "14 slides, about 13 minutes" | 13 min | the section timings | 790 s = 13.2 min | OK |
| 103 | Notes header | "Every number here and on the slides is filled in from numbers.json, … committed test logs" | — | deck.template.html, speaker_notes.template.md | quotes and some numbers are typed by hand (all checked OK) | WORDING (F13) |
| 104 | Notes §1 | "Kestrels hover in one place and watch closely." | — | general knowledge | true (kestrels hover while hunting); not in the logs | OK |
| 105 | Notes §2 | "It works with the wifi switched off. It costs nothing … open language models, small enough …" | — | as rows 5–7 | holds | OK |
| 106 | Notes §3 | "The examples in pale yellow are real requests from our tests." | — | as row 8 | holds | OK |
| 107 | Notes §3 | "like today's weather, it says so instead of making something up" | — | case 34 | holds | OK |
| 108 | Notes §4 | "word-for-word exchanges from the final test …; sample notes, so no real person's notes were used" | — | acc README | holds | OK |
| 109 | Notes §4 | "It made the task for 6 pm the next day, in 4.8 seconds." | 6 pm, 4.8 | case 2 (due 2026-10-28 18:00, run 2026-10-27) | 18:00 next day, 4.8 s | OK |
| 110 | Notes §4 | "It answered in Hinglish and named the note it used." / translation in Hindi script / "it says so, instead of guessing" | — | cases 26, 23, 27 | holds | OK |
| 111 | Notes §5 | "a few milliseconds; the same kind of model measured 5.6 milliseconds in our first version" | 5.6 | E1 R1 median; router_intents_v0_r1.json encoder multilingual-e5-small-q8_0 (same encoder as the kind head, fallback_dev_v2 meta) | 5.623 | OK |
| 112 | Notes §5 | "about half a second (523 ms on our practice set)" | 523 | as row 21 | 523.46 | OK |
| 113 | Notes §5 | "If both are unsure, Kestrel simply asks you: 'Did you mean …?'" | — | kinds.rs | not always; not the real wording | WORDING (F15) |
| 114 | Notes §5 | "256/280 … asked first 13 times … 6 times" | — | as rows 22–24 | 256/280, 13, 6 | OK |
| 115 | Notes §5 | "like answering a joke with chat" | — | fallback_test_v2 wrong[] (3 of the 6 are jokes, gold app); kind prompt rule 3 | count true, example misleading | WORDING (F12) |
| 116 | Notes §5 | "written by us …, most of them had been used once before … Only 24 were brand new." | 24 | as row 26 | holds | OK |
| 117 | Notes §6 | "The note never says 'tohfa'; it says 'gift ideas'. Still, it found the note …" | — | fixture note; case 12 | holds | OK |
| 118 | Notes §6 | "on our practice searches it found 1/9" | 1/9 | as row 32 | 1/9 | OK |
| 119 | Notes §6 | "The fix was simple: first turn the Hinglish search into English … After that it found 9/9." | 9/9 | meaning_dev_translated (θ 0.83; at θ 0.85: 7/9) | 9/9 needs the re-chosen θ too | WORDING (F11) |
| 120 | Notes §6 | "found the right note 14/18 times … 0/2 wrong notes" | 14/18, 0/2 | as rows 35–36 | 14/18, 0/2 | OK |
| 121 | Notes §7 | "'On which date do I fly to Goa?' It found the trip note and answered." | — | qa_test qa-goa-flight/en/0 top = 2026-09-15-goa-trip.md 0.8706 | holds | OK |
| 122 | Notes §7 | "'What is the name of our hotel in Goa?' … says the notes don't have it. It does not invent a hotel." | — | qa_test qa-none-hotel/en/0 (exactly this question): NOT IN NOTES; goa-trip note names no hotel | holds | OK |
| 123 | Notes §7 | "handled 25/25 questions right: 19/19 … 6/6" | — | as rows 40–42 | values right; scope as F5 | WORDING (F5) |
| 124 | Notes §7 | "checks that the numbers and names in it really appear in the note" | — | as row 39 | holds | OK |
| 125 | Notes §7 | "This test is small, 25/25 questions" | 25/25 | qa_test questions_n | 25 questions | WORDING (F17) |
| 126 | Notes §8 | "Kestrel can also write small Python programs, fix a broken function, or explain code." | — | code.rs; kinds.rs (explain = chat) | holds | OK |
| 127 | Notes §8 | "Before it shows you code, it tries the code inside a sealed box" | — | code.rs module doc and write(); page_*.json step order | code shown while written, then checked | MISMATCH (F1) |
| 128 | Notes §8 | "no internet, no access to your files, and limits on time and memory" | — | as rows 46–48 | holds | OK |
| 129 | Notes §8 | "If the code breaks there, Kestrel tries again, at most twice." | 2 | code.rs RETRIES = 2 (syntax error, crash, timeout); test log retries 2 | 2 | OK |
| 130 | Notes §8 | "functions 6/6, fixes 6/6, explanations 6/6, scripts … 3/6. In all, 21/24 requests right." | — | as rows 49–53 | values right; scope as F6 | WORDING (F6) |
| 131 | Notes §8 | "The box catches mistakes in loading the code; it does not run your program with your own input." | — | as row 55 | holds | OK |
| 132 | Notes §9 | "Ryzen 5 5500U … 6 cores, 14.95 GiB … no separate graphics card" | — | as rows 56–57 | holds | OK |
| 133 | Notes §9 | "reads the language model straight from the disk instead of copying it into memory" | — | as row 61 | as F10 | WORDING (F10) |
| 134 | Notes §9 | "fell from 3247 to 1144 MiB, 65% less" | — | as rows 58–60 | 3247, 1144, 65% | OK |
| 135 | Notes §9 | "ready in 2.0 seconds instead of 19.1" | — | as rows 62–63 | 2.0, 19.1 | OK |
| 136 | Notes §10 | "from pressing enter to the complete answer" | — | acceptance.py client time (socket.json s) | holds | OK |
| 137 | Notes §10 | "like a reminder or a search, take 0.3 seconds" | 0.3 | cases 1, 10 vs 2, 3, 12 | 0.3 for the English ones; 4.8 / 1.7 / 2.6 for the Hinglish ones | WORDING (F3) |
| 138 | Notes §10 | "2.8 seconds for a short translation, 8.5 for an email, 23.2 for a long explanation" | — | cases 23, 20, 18 | 2.8, 8.5, 23.2 | OK |
| 139 | Notes §10 | "The page shows the words as they are written, so you start reading long before the end." | — | as row 82 | as F4 | WORDING (F4) |
| 140 | Notes §11 | "we tested every feature the way a user would" | — | acc README opening | holds | OK |
| 141 | Notes §11 | "34/36 …; 8/8 …; 8/8 …; 17:31 … 17:31:02" | — | as rows 84–87 | holds | OK |
| 142 | Notes §11 | "448 plus 510 automatic checks of the code passed" | — | as row 88 | as F8 | WORDING (F8) |
| 143 | Notes §11 | "that is a screenshot from the test" | — | as row 89 | holds | OK |
| 144 | Notes §11 | "The two requests that failed are on the next slide." | 2 | socket.json summary.failed; slide 12 | only the rent one is on slide 12 | WORDING (F14) |
| 145 | Notes §12 | "'how much is my rent?' … one of the two failures. The other: 'which preferences have I set?' … found nothing, although one was saved." | — | cases 25, 17, 15 | holds | OK |
| 146 | Notes §12 | The other spoken limitations (scripts, no conversation memory, Hinglish answers, stars, small tests) | — | acc README 5, 12, 8, 7 | holds | OK |
| 147 | Notes §13 | The next steps | — | as rows 99–101 | holds | OK |
| 148 | Likely Q1 | "It uses much smaller models so it can run on a laptop without the internet … The trade is privacy and zero cost." | — | as rows 5–7 | holds | OK |
| 149 | Likely Q1 | "It is good at everyday personal tasks" | — | small self-written tests (34/36; slide 12) | a judgement on small tests | WORDING (F18) |
| 150 | Likely Q2 | "Every number on these slides is copied automatically from the saved test logs; nothing is typed by hand." | — | as row 103 | as F13 | WORDING (F13) |
| 151 | Likely Q2 | "the tests are small and we wrote them ourselves … a careful first check" | — | dataset manifests; ADR-045 | holds | OK |
| 152 | Likely Q3 | "Chat answers carry a note …; Notes answers are checked against the note before they are shown, and code is tried in the sealed box …" | — | kinds.rs CAVEAT (cases 18, 19, 30, 35); notes_qa.rs; code.rs; README #3 | chat and notes parts hold; code via chat is not checked | WORDING (F2) |
| 153 | Likely Q3 | "Slide 12 lists what we know goes wrong." | — | as row 97 | 6 of 12 | WORDING (F9) |

---

## What could not be checked, or rests on one source only

- **Reminder "popped up at 17:31:02".** The only record is the acceptance README ("rang 17:31:02 (`notified_at`
  set; notification sent)"). There is no separate log or screenshot of the notification.
- **"Works with the wifi off" (v3).** This is supported by the code: no network-client crate in the tree
  (checked 2026-10-28), the sandbox has no network, and the page loads nothing from elsewhere. But no v3 run
  was made with the wifi off or under `unshare -n`. The v1 privacy check (CHANGELOG, Ch. 42) covered research
  paths only.
- **2.0 s vs 19.1 s.** This was measured on 2026-10-26 on the v3 worker of that day, with the model file in the
  page cache. That was before the kinds fallback, notes Q&A and code features were added, and it was not
  re-measured on the frozen build. The first start after an install, with an empty cache, was 17.4 s.
- **v3's first look (the kind head).** Never timed. The 5.6 ms is version 1's (F16).
- **448 and 510 test counts.** Taken from the README and ADR-048. The tests were not re-run, because that is a
  heavy run on this laptop.
- **General statements** ("most assistants send your words to someone else's computer"; kestrels hover) are
  outside the logs. Both are true as general knowledge.
