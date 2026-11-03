# Kestrel's page: the human's design

The human designed Kestrel's page and asked for an exact replica (2026-10-31). Their design is the
spec; build nothing visual without it. A page with no design yet gets none of Claude's own.

| | Light | Dark |
|---|---|---|
| Design (the human's, 2000 px wide copies) | `design-home-light.jpg` | `design-home-dark.jpg` |
| Built (headless Chrome, 1920 px, after a few requests) | `built-home-light.jpg` | `built-home-dark.jpg` |

## Home (built)

`apps/cli/web/page.html`, served by `kestrel serve` and shown in the desktop app's window:

- side bar: Kestrel, Home · Tasks · Calendar · Notes · Settings, the motto card, Local First
- top bar: the promise, the date, sun and moon (light and dark; the choice is kept in the browser,
  else the system's)
- hero: "Good morning, NAME" (`kestrel settings name NAME`), the date, the day's line, the hills
  with the sun (light) or the moon (dark), the bird, and "Today: N scheduled · N unscheduled"
- the day strip: midnight to midnight, the kestrel over now, today's timed tasks as dots and cards
- Talk to Kestrel: the request box and the suggestions
- Recent Tasks / Suggestions: each request with Kestrel's answer under it; a tick once the one task
  it names is done; the arrow opens the whole answer (written text, code, a y/N question). Before
  any request: the suggestions
- Tasks: Overdue · Today · Coming up · No date, a circle and Done on each

Fonts (OFL, in `apps/cli/web/fonts` with their licences, served by the service, nothing fetched
from elsewhere): Source Serif 4 (headings), Caveat (the day's line), Inter (text).

## The other pages (Claude's design in Home's language)

The human, 2026-10-31: Claude designs the rest from Home's look (fonts, colours, cards, icons). A
photo from the human for any page still wins.

| Page | What it does | Screenshot |
|---|---|---|
| First start (the app's own page, `apps/desktop/ui`) | before Kestrel can run: where your notes are (Documents/Notes offered; a full path; the folder is made if missing), then Kestrel starts and the window goes to its page, where the first open follows. If a saved folder no longer works: why Kestrel stopped (the last line of its log), Start Kestrel or another folder | `built-first-start.jpg`, `built-first-start-dark.jpg`, `built-not-running.jpg` |
| First open | name → notes folder → this device → start; shown while no name is saved. With no model on the device, "this device" offers the import (below); with one, the speed test | `built-welcome-1.jpg`, `built-import-progress.jpg`, `built-speed-step3.jpg` |
| Tasks | add a task (plain words become "add task …"), filter All · Today · Upcoming · No date · Overdue, Done, and ⋯: Rename, Change date, Set time, Remove date, Delete (y/N) | `built-tasks.jpg` |
| Documents (inside Notes, so Home's side bar stays as designed) | the ID cards, policies and bills found in the folder, their expiry dates, days left; Check documents asks y/N before a date is kept; a click opens the file | `built-documents.jpg` |
| What Kestrel remembers (Settings) | the facts it was told to keep, each with Forget; Forget conversations; shown on the wifi, forgotten only on this device | — |
| Model download (Settings, first open) | with the internet on and a model missing: Download (2.6 GB), then "Downloading… N%", Stop, Continue from what came; its SHA-256 is checked before it is kept; each try in "What left this device"; turning the internet off stops it; refused from the wifi | `built-download.jpg` |
| Web search (an answer on Home) | "search the web for …": a short answer from the results, then Sources with their addresses, and that it may be wrong. If a page tried to give orders, it says those lines were left out. With no good answer, the results themselves | `built-web-search.jpg` |
| Internet (Settings) | the internet switch (off at install; also on the first open's step 3), the Gmail address and app password (emptied from the page at once; into the OS keyring through `kestrel-net`, in no file), contacts with Add and Remove, "What left this device" newest first with recipient and subject, not the text. your place for the weather, the news feeds you chose, the SearXNG web search goes through. Read on the wifi, changed only on this device. Asking "send it to Ravi" shows the exact mail in the y/N question. Off, Home says the human's words; on, "Nothing leaves without your yes" | `built-internet.jpg`, `built-mail-question.jpg` |
| Calendar | the month with each day's tasks, the chosen day's list and its hours | `built-calendar.jpg`, `built-calendar-dark.jpg` |
| Notes | the folder's notes with search, one read beside them, Summarize, a question about your notes, New note (y/N) | `built-notes.jpg`, `built-notes-dark.jpg` |
| Settings | your name, Light / Dark / System, the notes folder, the model and its import from a file, what Kestrel can do here and its speed test | `built-settings.jpg`, `built-settings-model.jpg`, `built-settings-speed.jpg` |
| Model import (Settings, first open) | a .gguf file's full path (`~` = home); the service copies it in on its own thread and checks its SHA-256 against the catalog, the page shows "Importing… N% (x of y GB)" each second; refused from the wifi | — |
| Speed test (Settings, first open) | Measure speed: `kestrel capabilities --measure` in its own process (the writer unloaded first), "Measuring… N s", then the speeds and the cards drawn again; one at a time, refused from the wifi | — |

Away from Home, Kestrel's answer (and any y/N question) shows in a card at the bottom right; it also
goes into Recent on Home. The API behind them: `GET`/`POST /api/settings` (the name),
`GET /api/notes`, `POST /api/notes/read`, `GET`/`POST /api/model/import`, `GET`/`POST /api/speed`,
`GET`/`POST /api/outbound`, `GET`/`POST /api/model/download`.

## See it

```
python3 scripts/preview_page.py        # demo data, own folders; your service and memory untouched
python3 scripts/preview_page.py --welcome   # the same, with the first-open steps
```

Checked by driving the page in headless Chrome over the DevTools protocol against a scratch
service: Home (21 checks: tasks, name, counts, fonts, a chip, a typed reminder, Done and its tick, a
delete's y/N, both themes) and the other pages (33 checks: the first open, add / filter / rename /
delete, the month and a chosen day, notes list / read / search / new / summarize, name and theme).

The first start was checked in the real app (debug build, WebKitGTK) with scratch folders, driven
through WebKit's remote inspector (`WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:PORT`): a relative path
and a folder that cannot be made are refused on the page; a new folder is made, saved, Kestrel
starts and the same window shows its page with the first open (about 1 s); with the saved folder
deleted, the page says why and Start Kestrel makes it again; from Kestrel's page (127.0.0.1) both
app commands are refused by the app's capability (local pages only).

The model import was checked in headless Chrome against a service run from the installed rpm's
files (no model there, as on a fresh install), own data folder on disk: 13 checks (step 3 offers
it; a non-.gguf, a relative path and a non-catalog .gguf refused; Gemma 4 E2B imported with
progress in 80 s on a debug build, whose SHA-256 is slow (6.8 s on the release build), the page's
other requests answered meanwhile in under 500 ms; then a writer, Settings' line, and a writing request answered by the imported model).
The speed test, same set-up with Gemma in the data folder and no speeds yet: 17 checks (step 3
follows the model going and coming back; the loaded writer unloaded for the test, which runs in
its own process, 12 s here; a second test refused; other requests answered meanwhile; 3 → 9 of 9
ready, on step 3's list and in Settings).
Sending mail, same kind of set-up with real Gemma and a stand-in `kestrel-net` (`experiments/v4/fake_net.py`,
which checks each approval token the way the real one does), then the real one with no password in the
keyring, so no mail could leave: `experiments/v4/page_steps/outbound.mjs` (asked with the internet off, with
no account, with no draft; a no; a yes; a stranger; each try in the log; the password in no file).
The download, same set-up with no Gemma among Kestrel's files, and a stand-in `kestrel-net` copying the real
file at 200 MB/s, so the hash check is real: `experiments/v4/page_steps/download.mjs`, 21 checks (off: the
button waits and the service refuses; progress; other requests answered meanwhile; Stop, Continue, the file
the same bytes as its source; each try in the log). The real `kestrel-net` was tried on the real address only
for its first 60 MiB, stopped twice: redirect, Range and append right, the bytes the same as the local Gemma's.
The whole 2.84 GB has not come down through it.
