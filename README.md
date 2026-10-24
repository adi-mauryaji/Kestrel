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

Then, every day:

```
scripts/assistant.sh ~/notes                  # or: KESTREL_NOTES=~/notes scripts/assistant.sh
```

It takes about 30 s to load, then answers each request in milliseconds (rules, encoder) up to about a second
(the language model). Examples:

| You say | It does |
|---|---|
| `search my notes for budget` | lists the matching notes |
| `open project-plan.md` | shows the document |
| `remind me to call Asha tomorrow` | adds a task with tomorrow's date |
| `summarize my last three meeting notes and create a task list` | a summary, plus a task per action item (asks first if there are more than three) |
| `delete task 2` | removes the task, after a y/N |
| `I prefer short answers` / `No, I prefer detailed answers unless I'm in a hurry` | remembers the preference (a correction asks before saving) |
| `when I say standup doc, I mean team sync` | learns the nickname (asks first) |
| `kal ki meeting notes ka summary do` | Hinglish works for every request type |

Notes are read only through a sandbox of the folder you give. Tasks, preferences and memories live in one
SQLite file, `$KESTREL_MEMORY` or `~/.local/share/kestrel/memory.db`, which you manage with
`target/release/kestrel memory list | tasks | delete ID | export | purge`.

**Limits today.** Nine kinds of request (search, open, summarize, add or delete a task, a preference, what it
remembers, small talk; anything else gets "I can't do that"). Summaries pick sentences from your notes rather
than write new ones. Due dates are stored, but nothing pops up when one arrives. A two-part request needs
"and", "then", "aur" or "phir" between its parts ("also" and ", now" are not recognized yet), and "kal" in a
summary request is not read as a date. It runs in a terminal and needs about 3.7 GB of RAM while running.

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
