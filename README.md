# Kestrel (version 3)

A private assistant that runs on your own Linux laptop, in English and Hinglish. It keeps your tasks and reminds you,
finds and answers from your notes, and writes mails, rewrites, translations and small Python code. Everything runs on
your device, offline: the language model too. Nothing you type leaves it.

Every request goes through a cascade: keyword rules first, then a small sentence encoder, and the language model
(Gemma 4 E2B) only when the cheaper tiers are not confident.

**This version is frozen.** It runs on Linux only, and has been used on one laptop (Ryzen 5 5500U, 14 GB, no GPU).

## Set it up

You need Linux with systemd, [Rust](https://rustup.rs) (the repository pins its version), CMake, a C++ compiler,
Python 3.11 or newer, and [bubblewrap](https://github.com/containers/bubblewrap) for checking code. Allow about 6 GB of
disk, and 10 to 20 minutes for the first build.

Download this repository (**Code → Download ZIP**) and unpack it, or clone it. Then, inside its folder:

```sh
# The sentence encoder and its trained heads (121 MiB), too big for git
curl -fLO https://github.com/adi-mauryaji/Kestrel/releases/download/models-v1/kestrel-ci-assets-v1.tar.gz
echo "938cfffb9c04073a13a4eb725776fd6fc815720c43cb0620e3ac307390a8c1cf  kestrel-ci-assets-v1.tar.gz" | sha256sum -c
tar -xzf kestrel-ci-assets-v1.tar.gz && rm kestrel-ci-assets-v1.tar.gz

# The language model (2.84 GB); its SHA-256 is checked
python3 scripts/fetch_models.py --only gemma-4-e2b-it-q4_0

# Builds it, and runs it from every login on; the language model loads only when needed
scripts/install_service.sh ~/notes
scripts/install_reminders.sh                  # what is due, every morning at 08:00 and after login
```

Replace `~/notes` with your notes folder. Run the two scripts again after an update; `--remove` takes either away, and
your notes and tasks stay.

## Use it

Open it in the browser (`kestrel open`, or Kestrel in the applications menu), or use a terminal:

```sh
kestrel chat                                  # a conversation; "quit" ends it
kestrel say "mere tasks btao"                 # one request
kestrel stop                                  # frees its memory; open, chat or say start it again
```

| You say | It does |
|---|---|
| `remind me to call Asha at 5 pm` | adds a task at 17:00, and a notification pops up then |
| `kal shaam 6 baje bill bharna yaad dila dena` | the same in Hinglish (tomorrow, 18:00); chat spellings like `krna`, `kl`, `h` work too |
| `mere tasks btao` / `what's due today` / `overdue tasks` | lists your tasks by when they are due |
| `task 3 ho gaya` / `task 3 at 9 am` / `move task 3 to friday` / `task 4 ka naam X rakho` | finishes, times, reschedules or renames a task, by its number |
| `delete task 2` | removes the task, after a yes/no |
| `search my notes for budget` / `open project-plan.md` | lists the matching notes; shows a document |
| `summarize my latest note` | a summary, checked against your notes |
| `how much is the Q4 budget in my notes` | an answer from your closest notes, naming the note |
| `explain gravity in one line` / `photosynthesis kya hota hai` | a chat answer from the small model, marked as such |
| `write an email to my teacher asking for leave on Friday` | a draft for you to copy; this version cannot send mail |
| `translate to hindi: good morning` | a translation |
| `write a python function to reverse a string` | code, compiled in a sandbox that has no network |
| `I prefer short answers` | remembers the preference |
| `when I say standup doc, I mean team sync` | learns the nickname (asks first) |

Most requests take a fraction of a second. The first that needs the language model loads it: about 2 s, or about 30 s
the very first time. It unloads after 10 idle minutes. The service needs about 300 MB, and about 1.1 GB more with the
model loaded, plus its 2.7 GB file mapped into memory, which the system can take back when it runs short.

Notes are read only through a sandbox of the folder you give. Your tasks and memories live in one SQLite file,
`~/.local/share/kestrel/memory.db`, managed with `kestrel memory list | tasks | delete ID | export | purge`. The page
answers only on 127.0.0.1, and only with a key kept beside that file.

## Its limits

- **No memory of the conversation:** a follow-up does not see the last answer.
- **It keeps preferences, not facts:** `my sister's name is Meera` is not kept.
- **Some requests are refused or asked about:** a question it reads as a command gets "I can't do that", and rewrites are
  often asked about first (say yes). Nothing here is on the internet: no weather, news or search.
- **Drafts can come out in Hinglish** when you asked in English, and can leave blanks such as `[Date]`.
- **Chat answers show Markdown as typed** (`**bold**`, `*` bullets), and come from a small model, so they can be wrong.
- **Code:** Python's standard library only; checked by compiling it, never run with input.
- **Times:** "6 baje" means the next time the clock shows it; "at 5" alone is not read as a time. Timed reminders pop up
  only while the service runs.

The full list is in [`experiments/logs/v3/acceptance/README.md`](experiments/logs/v3/acceptance/README.md).

## The research

The project began as a study: a pre-registered, measured evaluation of calibrated cascade routing on a laptop. It lives
under `experiments/`: [`PREREG.md`](experiments/PREREG.md), [`RESULTS.md`](experiments/RESULTS.md),
[`ERROR_ANALYSIS.md`](experiments/ERROR_ANALYSIS.md), [`LIMITATIONS.md`](experiments/LIMITATIONS.md) and
[`README_REPRO.md`](experiments/README_REPRO.md). `scripts/demo.sh` shows how each tier decides on every request. The
papers are in [`paper/`](paper).

## Develop

```sh
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
python3.14 -m venv .venv && .venv/bin/pip install -r requirements.lock && .venv/bin/python -m pytest
```

## Licence

The code is under `MIT OR Apache-2.0`, as `Cargo.toml` says. Models have their own licences.
