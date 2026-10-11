# Kestrel intent ontology v0

Status: v0, accepted by the human 2026-10-11 (ADR-010). This file is also the **annotation guide**
for the second annotator who labels ≥ 20% of Kestrel-Intents-v0 for Cohen's κ (Ch. 31.2).

Labels live in `datasets/contracts/intents.txt`. That file is the order of `Intent::ALL` in
kestrel-core and is the class order of every classifier head; a core test keeps the two identical.

## Labels

| Label | The user wants Kestrel to… | Tool (Ch. 15) | Example (en / hi-en) |
|---|---|---|---|
| `search_notes` | find notes by topic or keywords | `search_notes(query, limit)` | find my notes about thesis / thesis ke notes dhundo |
| `read_document` | open or show one specific, named document | `read_document(path)` | open todo.md / todo.md kholo |
| `create_task` | add a task or reminder | `create_task(title, due?)` | remind me to call asha tomorrow / kal asha ko call karna yaad dila dena |
| `delete_task` | delete, remove or cancel an existing task or reminder | `delete_task(id)`, destructive: asks first | delete task 7 / task 7 delete karo |
| `summarize_notes` | summarize notes or a document | `fetch_recent_notes` → summarize | summarize my last three notes / last 3 notes ka summary do |
| `set_preference` | remember a standing preference or a personal fact | `update_preference(key, value)` | i prefer short answers / yaad rakhna ki main vegetarian hoon |
| `retrieve_memory` | say what it has stored about the user | `retrieve_memory(terms)` | what do you remember about my sister? / meri sister ke baare mein kya yaad hai? |
| `chitchat` | greet, thank, chat, or answer a question about Kestrel itself | none (Tier 4 reply) | thanks / kya haal hai |
| `unsupported` | do something no Kestrel tool can do | none: say so, list supported actions | what's the weather in pune? / pune mein mausam kaisa hai? |

## Decision rules (apply in order; the first that fits decides)

1. **Two or more actions asked** → *compound*: list every intent in the order asked.
   "find my notes on X and summarize them" → `[search_notes, summarize_notes]`.
2. **A summary, gist, recap or key points** → `summarize_notes`, even when one document is named.
3. **One named document** (file name, path, "the X doc") to open, show or read → `read_document`.
   A topic or keywords instead → `search_notes`.
4. **What Kestrel stored about the user** ("what do you remember", "what did I tell you") →
   `retrieve_memory`. The user's own notes and files → `search_notes`.
5. **Tell Kestrel to keep something about the user**: a standing preference ("from now on",
   "always", "I prefer") or a personal fact ("remember that my sister is Priya") → `set_preference`.
6. **Something to do later** (task, reminder, to-do) → `create_task`. Remove or cancel one →
   `delete_task`.
7. **Needs a capability Kestrel lacks**: internet, email, phone calls, messages, device control,
   shopping, travel, live data, general knowledge, calculations → `unsupported`.
   "call asha" (place a call now) is `unsupported`; "remind me to call asha" is `create_task`.
   "google X" is `unsupported`; "search my notes for X" is `search_notes`.
8. **Social talk, no action**; questions about Kestrel ("what can you do?") → `chitchat`.

Label what the user asks for, not what Kestrel would do well. Ignore spelling, casing and
language mixing: the same request in English or Hinglish gets the same label.

## Not in v0

- **Negated requests** that refuse or cancel ("don't remind me about X"): the label policy is open.
  Idioms that only *contain* a negation word but ask for an action ("don't let me forget to X",
  "mujhe X bhoolne mat dena") are in v0 with their action's label; Tier 1 escalates them on purpose.
- **Mark done / edit / reschedule a task**: no tool exists in Ch. 15.
- **Hindi in Devanagari script**: only Romanized Hinglish (ADR-005).

## Review decisions (human, 2026-10-11)

1. "I prefer X, now summarize…" is compound `[set_preference, summarize_notes]`: "I prefer" is a
   standing preference (rule 5).
2. General-knowledge questions stay `unsupported`: without retrieval, Tier 4 could be confidently wrong.
3. Still open, for v1: is "mark task 7 done" `unsupported` until a tool exists, or should v1 add
   `complete_task`? Not in v0 either way (no templates).
