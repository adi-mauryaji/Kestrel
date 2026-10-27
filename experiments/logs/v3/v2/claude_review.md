# Claude's review of Kestrel-CodeTasks-v0 and Kestrel-Kinds-v2 (2026-10-27)

The human waived the review: "can you review this on your own its very boring" (ADR-045). Claude wrote both
drafts, so this review is not independent of the author. Two passes:

1. A separate agent, given only the files and the criteria (not Claude's conclusions), ran every reference
   solution and the broken fix code, and listed 19 findings (1 must-fix, 11 should-fix, 7 minor).
2. Claude ran every dev failure of the three models (767b138) against its hidden check, to see whether the
   model or the check was wrong.

Both found the same must-fix: scripts were judged by exact output, while no request says "print only the
answer". A model that asks "Enter a whole number:" or prints "4 is even" wrote a script a person can use.

## Applied

| Finding | Change |
|---|---|
| Scripts judged by exact output (all `sc-*`) | Grading, not the requests: a case passes when the output's last non-empty line holds the expected output as whole words (a number may carry a unit, never more digits). Real users would not write "no input prompt". |
| `sc-count-lines`: a spaces-only line counted as blank, against "khaali nahi" | That line dropped from the case. |
| `fn-vowels`, `fn-grade` Hinglish: no "return" | "... ki ginti return kare", "... pe A return kare". |
| `fn-second-largest` Hinglish unclear | "second largest distinct number return kare (duplicate ek hi baar gino), warna None". |
| `fn-palindrome`: empty string never mentioned | That assert dropped. |
| `fix-average`, `fix-find-max` weak (`//`, `m = -1` passed) | `average([1, 2]) == 1.5`, `find_max([-10, -20]) == -10` added. |
| `ex-countdown`: "10" and "2" are in the code itself | Facts now 8, 6, 4 (only in the output). |
| `ex-word-dict`: the dict fact too strict, "count" too weak | Facts: how many times / kitni baar / frequenc / occurrence / count of each / counts each / increments; word. ("frequenc" after Qwen3-1.7B's correct "word frequencies" failed "frequency".) |
| `ex-list-comp`, `ex-read-file`, `ex-sort-key` facts | More ways: x * x, 0, 2, 4, divisible by 2, 2 se divide, multiple of 2; gin; umar / umr. |
| Kinds-v2 `code-11`: "why does this crash ... int('12a')" is mostly explaining | "this crashes with ValueError when the input has letters, fix it: n = int(input())" (+ Hinglish). |
| `chat-v2-06`: a poem in the stories family | "write a bedtime story about a brave little elephant" (+ Hinglish). |
| `rewrite-v2-05`: a title for a topic, not for text | Real text given. |
| `rewrite-v2-04` near-duplicate of a one-line summary | "cut this to under ten words: ..." / "isko 10 words se kam mein likho: ...". |
| `notes-v2-02`: two "what is my X number?" | "when does my driving licence expire?" / "mera driving licence kab expire hota hai?". |
| `code-12`: words inside code ("then", "phir") | Real newlines in the code. |
| Hinglish: "maine ... dekhni thi", "meri eye checkup", "paragraph mein badha do", "hum aakhir ..." | "mujhe ... dekhni thi", "mera eye checkup", "isko expand karke ek paragraph bana do", "finally hum ...". |

Not applied: `notes-v2-01` / `notes-v2-04` share "when is my X?" (minor; different things asked); "ek line
mein likho" (that request was replaced).

## The three models' dev answers, judged again (dev_rescored_review.json)

| Model | Strict checks (767b138) | Reviewed checks | Write / fix code (18) | Explain (6) | Generation s / request |
|---|---|---|---|---|---|
| Gemma-4-E2B | 19 | 22 | 16 | 6 | 11.1 |
| Qwen3-1.7B | 17 | 18 | 13 | 5 | 10.4 |
| Qwen3-4B | 23 | 22 | 17 | 5 | 29.0 |

The answers are to the old wording of three Hinglish requests. Gemma's remaining misses: `sc-sum-lines`
(both crash at the end of input). Qwen3-4B's: `sc-even-odd` Hinglish (a function, no script) and
`ex-countdown` Hinglish (Devanagari, and "odd numbers": wrong).
