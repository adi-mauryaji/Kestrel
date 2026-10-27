# v3 acceptance run before the freeze (2026-10-27)

The human: "ab kuch changes nahi krne hain, ab hr ek feature jo humara goal tha wo test krna hai ... if headless
and withhead testing required hai wo bhi kro, ab hum ise freeze krenge". Every goal feature (ADR-034/035 daily
use, ADR-036's v3 list and what followed) was driven end to end, nothing changed in the product.

**Build:** 86cb3a7, `target/release/kestrel` (byte-identical to the installed `~/.local/share/kestrel/bin/kestrel`).
**Where:** a scratch `kestrel serve`: Kestrel-NotesQ-v0's 18 fixture notes copied, its own memory, cache and runtime
dir, page on 127.0.0.1:7899; never the human's notes or tasks. AC power. Laptop as found.

## Results

| Run | Result | Log |
|---|---|---|
| Socket (`experiments/v3/acceptance.py`): 36 requests as `kestrel chat` sends them, y/N answered | **34 / 36** | `socket.json` |
| Browser page, headless Chrome (`experiments/v3/page_check.mjs`) | **8 / 8** | `page_headless.json`, `.png` |
| Browser page, headed Chrome on the human's screen (same script) | **8 / 8** | `page_headed.json`, `.png` |
| Timed reminder: a task for 17:31 made at 17:29:29 | rang 17:31:02 (`notified_at` set; notification sent); `kestrel remind` lists it | below |
| `cargo test --workspace` | 448 passed, 0 failed (11 ignored: models / bubblewrap) | |
| bubblewrap tests (`--ignored`, sandbox + code loop) | 6 / 6 | |
| model worker tests (`--release --features llama --test worker --ignored`) | 4 / 4 (2 m 36 s) | |
| `pytest` | 510 passed | |
| Installed service (read only) | active; page 200 on 127.0.0.1 and on the wifi (ADR-047), API 401 without the key; greeting names every skill; reminder timer active | |

Socket cases that passed: small talk; tasks (a time, Hinglish with a time, chat spellings, list, done, rename,
reschedule to Friday, delete after its question, list after); notes (keyword search, by meaning in English and
Hinglish, summary, open a document); memory (a preference, recall by its words); chat (explanation, Hinglish poem,
with the caveat); email (Hinglish, English); rewrite (polite, stays English); translate (into Hindi in Devanagari,
into English); notes Q&A (Hinglish answer with its note, "The notes do not say this ..."); code (a function,
checked "no errors"; a fix; explaining code goes to chat); save to notes (no, yes with the file checked, nothing
given); a refusal for live data; a long essay ends with the cut-off note.

Page steps that passed (both runs): the page with its day strip, Try buttons and skills; a Try button; a task
made on the page appears in the task list; the page's own y/N question, answered Yes, deletes it; a code answer
in a code box with the sandbox line; the Copy button (a real mouse click) puts the code on the clipboard; a chat
answer streamed with its caveat.

## Failures and known limitations (not fixed: the human froze the product)

1. **"how much is my rent?" is refused** ("I can't do that") instead of answered from the notes: the kind head
   gives app 0.62 / notes 0.35 (below tau 0.8), and Gemma's fallback also says app. "maa ka birthday kab hai?" is
   answered. Kinds-v2's test: notes 12 / 16 right. Fixing it needs new data (Kinds-v3).
2. **"which preferences have i set?" finds nothing** although one is stored: Stage 2's memory search matches the
   stored words ("what do you remember about answer length?" works).
3. "write a program to solve X" goes to chat (head 0.94): the whole program comes, but no sandbox check.
4. Rewrite requests are often asked about first (Kinds-v2 test: 8 / 20 taken directly, 10 asked).
5. Scripts that read input to its end: Gemma's weak spot (CodeTasks-v0 test: scripts 3 / 6).
6. Jokes are answered by chat; "what time is it in London" goes to notes ("not in the notes").
7. Markdown in chat answers (`**bold**`, `*` bullets) shows as typed (headed screenshot).
8. Notes answers to Hinglish questions are sometimes English (dev 9 / 12 Hinglish).
9. Code: Python's standard library only; checked by import / compile in the sandbox, never run with input.
10. Once, code did not show on the human's page; not reproduced (curl, headless and headed Chrome, live service).
11. The page is shared on the wifi over plain HTTP (ADR-047): on until the service is installed without it.
12. No memory of the conversation: "continue" or follow-ups do not see the last answer.

## The first socket run

`socket_run1.json`: 31 / 35 on a first scratch service. Two failures were wrong checks of this script, not the
product ("move task 2 to friday" answered "now due 2026-10-30", a Friday; "I prefer short answers" answered
"saved preference: answer_length = short"); the checks were corrected, a recall-by-its-words case added, and the
whole run repeated on a fresh scratch service (`socket.json`). The other two failures are limitations 1 and 2.
