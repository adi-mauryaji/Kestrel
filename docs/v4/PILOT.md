# Kestrel v4 pilot: the plan

For whoever runs the pilot. The blueprint's last test (§1, §10): do people keep using Kestrel, and would
they miss it? Nothing here has been run yet.

## The test

- **Who:** students and acquaintances, 10 to 20 people, on their own laptops (ADR-051).
- **How long:** two weeks from each person's install.
- **The bar:** 40% or more answer "very disappointed" to "How would you feel if you could no longer use
  Kestrel?" (blueprint §1).
- **Who counts:** everyone who installed Kestrel and used it at least once. Fix this before reading any
  answers. Someone who stopped using it counts, and their answer matters most. Report anyone who gave no
  answer separately, not as a "no".

With so few people this is a signal, not a proof. 8 of 20 is 40%, but its 95% interval runs from about 22%
to 61%. `pilot_summary.py` prints that interval; say it whenever the number is quoted.

## What must be true first

| # | What | Why | Whose |
|---|---|---|---|
| 1 | Kestrel opens and answers on a real Windows PC | Its service and page have only compiled on Windows, never run. Students likely use Windows | the organiser |
| 2 | The `.dmg` opens and answers on the M4 | The same for macOS | the organiser |
| 3 | The real Gmail send and a real ticket PDF work | People will try them; so far they ran only with stand-ins | the organiser |
| 4 | The first open works from nothing, including the real 2.84 GB download | Done once on Linux, from the unpacked rpm, without the window (`experiments/v4/download_test.py`). It never stopped on the way, so a real stop and continue of the whole file has not happened | the organiser, once on macOS and on Windows |
| 5 | A new tag (`v0.1.0-rc3` or later) built its installers | The last ones have no daily counts | the organiser |
| 6 | The installers are on a GitHub Release or a shared drive | A workflow's files need a GitHub login, and they expire | the organiser |
| 7 | The placeholders in `pilot/guide.md` and `pilot/consent.md` are filled in | Name, contact, where to get the installers | the organiser |
| 8 | Ask the mentor whether the college needs an ethics approval | Only if the results go in the paper. Do this before anyone signs | the organiser |

Real SearXNG and Tailscale need not hold it up: few students will set them up, and Kestrel works without them.

Installers, once a tag has built them:

```sh
gh run download RUN_ID --dir installers       # RUN_ID from: gh run list --workflow release-desktop
gh release create v0.1.0-rc3 --prerelease --notes "Pilot build" installers/*/*
```

## The two weeks

| When | What | What to send |
|---|---|---|
| Before | Each person reads the guide and agrees to the consent text | `pilot/guide.md`, `pilot/consent.md` |
| Day 0 | They install it. Be reachable: this is the likeliest place to lose people. Write down each failure as they tell it | |
| Day 2 or 3 | "Did it install? Has it been useful yet?" | One short message |
| Day 7 | "Anything wrong or annoying?" Fix what can be fixed; a new build goes to everyone. Tell them to Quit Kestrel from its icon first: installing over a running app changes nothing until it restarts (the page then says so) | One short message |
| Day 14 | They copy their counts from Settings and fill in the survey | `pilot/survey.md`, as a form |
| After | Run the summary; write up the results, including what failed | |

Give each person a code (P01, P02, ...) and keep the list of who is who apart from the results. Save each
person's counts as `P01.json` in one folder, outside this repository.

**Stop adding people** on a system if two people on it cannot install. Fix that first.

## What is collected, and what is not

From Settings, "How Kestrel is used", only what the person chooses to send:

- Kestrel's version, the system (`linux`, `macos`, `windows`), how many days it was used.
- For each day, counts under these names:

| Name | What one means |
|---|---|
| `request` | Kestrel was asked something (a day with this or `task_added` is a used day) |
| `task_added`, `document_added` | A task was added, by any route; a document was first found in the notes folder |
| `reminder_rang`, `document_reminder_rang`, `brief_rang`, `weather_reminder_rang` | That notification was shown |
| `chat`, `email`, `rewrite`, `translate`, `code`, `notes` | Written by the model as that kind |
| `create_task`, `delete_task`, `search_notes`, `read_document`, `summarize_notes`, `set_preference`, `retrieve_memory`, `chitchat`, `unsupported` | Routed through the tiers as that intent |
| `not_understood` | Nothing could place the request |
| `task_command`, `fact`, `correction`, `documents`, `ticket`, `mail`, `note_saved`, `weather`, `news`, `online_choice`, `brief`, `research`, `research_saved`, `web_search`, `why`, `new_conversation` | Answered by that rule |

- Each capability's state now (`ready`, `slow`, `setup`, `locked`).

**Never:** what anyone said, names, notes, documents, tasks' words, contacts, mail, RAM, CPU or GPU. The test
`each_request_is_counted_by_the_way_it_took_and_nothing_of_what_was_said` holds this.

The counts cannot say which skill helped someone, or which requests went wrong. Only the survey can, so it
asks.

## Reading the results

```sh
python3 experiments/v4/pilot_summary.py FOLDER --very 8 --of 20
```

It prints a line per person and a summary: days used per week, tasks and documents added, reminders,
requests nothing could place, and the "very disappointed" share with its interval. Files it cannot read are
named, and it exits 1 rather than quietly leaving someone out.

Someone who pressed Forget has lost their earlier counts: ask before reading a low number as low use.

Write these up whatever they show: the share with its interval, how many installed and how many stopped, the
failures from day 0, and the requests from the survey. Those requests are the first data for Kinds-v3
(blueprint §4.4), after each is checked for private details.
