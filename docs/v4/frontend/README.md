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

## Not designed yet

Tasks and Calendar scroll to their parts of Home. Notes and Settings are plain placeholder cards
(Settings shows what Kestrel can do here, as the old page did). Onboarding (notes folder, internet,
model import, speed test) waits for its design too.

## See it

```
cargo build -p kestrel-cli --features llama
python3 scripts/preview_page.py        # demo data, own folders; your service and memory untouched
```

Checked by driving the page in headless Chrome over the DevTools protocol against a scratch
service (21 checks: tasks, name, counts, fonts, a chip, a typed reminder, Done and its tick, a
delete's y/N, both themes, Settings).
