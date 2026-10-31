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
| First open | name → notes folder → this device → start; shown while no name is saved | `built-welcome-1.jpg` |
| Tasks | add a task (plain words become "add task …"), filter All · Today · Upcoming · No date · Overdue, Done, and ⋯: Rename, Change date, Set time, Remove date, Delete (y/N) | `built-tasks.jpg` |
| Calendar | the month with each day's tasks, the chosen day's list and its hours | `built-calendar.jpg`, `built-calendar-dark.jpg` |
| Notes | the folder's notes with search, one read beside them, Summarize, a question about your notes, New note (y/N) | `built-notes.jpg`, `built-notes-dark.jpg` |
| Settings | your name, Light / Dark / System, the notes folder, the model, what Kestrel can do here | `built-settings.jpg` |

Away from Home, Kestrel's answer (and any y/N question) shows in a card at the bottom right; it also
goes into Recent on Home. The API behind them: `GET`/`POST /api/settings` (the name),
`GET /api/notes`, `POST /api/notes/read`.

## See it

```
cargo build -p kestrel-cli --features llama
python3 scripts/preview_page.py        # demo data, own folders; your service and memory untouched
```

Checked by driving the page in headless Chrome over the DevTools protocol against a scratch
service: Home (21 checks: tasks, name, counts, fonts, a chip, a typed reminder, Done and its tick, a
delete's y/N, both themes) and the other pages (33 checks: the first open, add / filter / rename /
delete, the month and a chosen day, notes list / read / search / new / summarize, name and theme).
