# Kestrel

An on-device assistant for your notes and tasks, in English and Hinglish, and the research system that
measures it. Every request goes through a cascade: keyword rules first, then a small sentence encoder, and
a local language model (Gemma-4-E2B) only when the cheaper tiers are not confident. Nothing leaves the
machine.

## Use it on your notes

One-time setup (Linux, about 6 GB of models):

```
python3.14 -m venv .venv && .venv/bin/pip install -r requirements.lock
.venv/bin/python scripts/fetch_models.py      # downloads and checks every model's SHA-256
scripts/convert_encoder.sh                    # the multilingual-e5-small encoder -> GGUF
.venv/bin/python experiments/tier3/train.py   # the encoder's trained heads
cargo build --release -p kestrel-cli --features llama
```

Then install it once, and it runs from every login on:

```
scripts/install_service.sh ~/notes            # always on; the language model loads only when needed
scripts/install_reminders.sh                  # what is due, every morning at 08:00 and after login
```

Talk to it in the browser (`kestrel open`, or Kestrel in the applications menu) or in a terminal:

```
kestrel chat                                  # a conversation; "quit" ends it
kestrel say "mere tasks btao"                 # one request
kestrel stop                                  # frees its memory; open, chat or say start it again
```

Most requests take a fraction of a second (rules, encoder). The first one that needs the language model
loads it: about 2 s, except the first time after an install (about 17 s, while it saves its prepared
prompt). It is unloaded again after 10 idle minutes. `kestrel stop` frees all of it until
the next `kestrel open`, `chat` or `say` (about a second to start again) or the next login; reminders with
a time pop up only while it runs. Run the two scripts again after an update; `--remove` takes either
away (notes and tasks stay). Examples:

| You say | It does |
|---|---|
| `remind me to call Asha at 5 pm` | adds a task for today at 17:00, and a notification pops up then |
| `kal shaam 6 baje bill bharna yaad dila dena` | the same in Hinglish (tomorrow, 18:00); chat spellings like `krna`, `kl`, `h` work too |
| `mere tasks btao` / `what's due today` / `overdue tasks` | lists your tasks by when they are due |
| `task 3 ho gaya` / `task 3 at 9 am` / `move task 3 to friday` / `task 4 ka naam X rakho` | finishes, times, reschedules or renames a task |
| `search my notes for budget` | lists the matching notes |
| `open project-plan.md` | shows the document |
| `summarize my meeting notes` | a summary in the model's own words, checked against your notes: one that adds a name or number they lack is set aside for the notes' own first lines |
| `delete task 2` | removes the task, after a yes/no |
| `I prefer short answers` | remembers the preference |
| `when I say standup doc, I mean team sync` | learns the nickname (asks first) |
| `search my notes for budget, also remind me to call Ravi` | two requests at once (joined by "and", "aur", ", also" or ", now") |

Notes are read only through a sandbox of the folder you give. Tasks, preferences and memories live in one
SQLite file, `$KESTREL_MEMORY` or `~/.local/share/kestrel/memory.db`, which you manage with
`kestrel memory list | tasks | delete ID | export | purge`. The page answers only on 127.0.0.1 and only with
a key kept beside that file (`kestrel open` passes it).

Without installing anything, `scripts/assistant.sh ~/notes` runs the same assistant in one terminal (about
30 s to load, 3.7 GB while it runs, summaries picked from your notes' sentences).

**Limits today.** Nine kinds of request (search, open, summarize, add or delete a task, a preference, what it
remembers, small talk; anything else gets "I can't do that"). A time without am or pm ("6 baje") means the
next time the clock shows it; "at 5" alone is not read as a time. Timed reminders pop up only while the
service runs. The service needs about 300 MB, and about 1.1 GB more while the language model is loaded, plus its
2.7 GB file mapped into memory, which the system can take back when it runs short.

## The research

The study (a pre-registered, measured evaluation of calibrated cascade routing on this laptop) lives under
`experiments/`: the pre-registration `PREREG.md`, the results `RESULTS.md`, the failure analysis
`ERROR_ANALYSIS.md`, `LIMITATIONS.md`, and how to reproduce everything, `README_REPRO.md`. The plan it follows
is `KESTREL_COMPLETE_RESEARCH_AND_IMPLEMENTATION_BLUEPRINT.md`. `scripts/demo.sh` shows how each tier decides
on every request.

## Development

```
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
.venv/bin/python -m pytest
```
