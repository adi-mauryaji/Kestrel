# Moving this laptop from v3 to v4

v3 stays frozen and can come back in a minute (see "Going back"). Time: about 10 minutes, most of it
the model's copy and the speed test.

**What stays:** tasks and memories (the same `~/.local/share/kestrel/memory.db`, same format), notes,
the 08:00 morning reminder (`kestrel-remind.timer`; v3's binary stays where it is).

**What changes:**
- Kestrel runs inside the app, not as a systemd service. App closed = no Kestrel and no timed
  reminders, so step 5 matters.
- The page's address changes at each start. Open it from the app or with `kestrel open`; a bookmark
  of `127.0.0.1:7811` stops working.
- Terminal `kestrel` becomes v4's. `kestrel stop` no longer stops it: use the tray icon's Quit.
- Before a timed research run (E6): tray icon → Quit Kestrel.

A backup was taken on 2026-11-03: `~/.local/share/kestrel/backup/v3-2026-11-03` (memory file, v3's
unit, menu entry and icon). Step 2 adds a copy from the moment of the switch.

## The steps

```sh
# 1. The new app and CLI (needs your password)
sudo dnf reinstall ~/Kestrel/apps/desktop/target/release/bundle/rpm/Kestrel-0.1.0-1.x86_64.rpm

# 2. v3's service off: removes its unit, menu entry and `kestrel` link. Notes and tasks are kept.
~/Kestrel-v3/scripts/install_service.sh --remove
cp ~/.local/share/kestrel/memory.db ~/.local/share/kestrel/backup/v3-2026-11-03/memory-at-switch.db

# 3. Your notes folder. The full path, because this terminal may still remember the old command.
/usr/bin/kestrel settings notes ~/notes
```

4. Open **Kestrel** from the applications menu. The page asks your name. On **This device**, type
   this path and press **Import** (about 10 s; `~/Kestrel/models` stays as it is):

   ```
   ~/Kestrel/models/gemma-4-E2B-it-Q4_0.gguf
   ```

   Then press **Measure speed** (up to 2 minutes the first time) and **Continue**.

5. Tray icon → tick **Start at login**, so Kestrel and its reminders are there after each login.

6. Check in a new terminal: `kestrel say "mere tasks btao"` lists your tasks.

## Going back to v3

```sh
kestrel-desktop --quit
B=~/.local/share/kestrel/backup/v3-2026-11-03
cp "$B/kestrel.service" ~/.config/systemd/user/
cp "$B/kestrel.desktop" ~/.local/share/applications/
cp "$B/kestrel.svg" ~/.local/share/icons/hicolor/scalable/apps/
ln -s ~/.local/share/kestrel/bin/kestrel ~/.local/bin/kestrel
systemctl --user daemon-reload
systemctl --user enable --now kestrel.service
```

Tasks added in v4 stay. Untick **Start at login** first if you do not want the app as well; with v3
running it only opens v3's page.

## What was checked

A rehearsal on 2026-11-03: the new rpm unpacked in a scratch folder, the backup's memory file, a copy
of the notes, and a `systemctl` answering as after step 2. The real data and the v3 service were not
touched (memory file's hash the same after).

- The app started its own service (0.7 s) and showed its page: 3 of 3 tasks, 12 of 12 notes.
- `kestrel say` listed the tasks and added one; v3's binary read that file afterwards, schema
  unchanged: going back keeps the data.
- With nothing running, `open`, `say` and `stop` give the right advice (the rehearsal found these
  wrong; fixed @5652ef7). Quit stopped the app and its service.

**Not checked until you do the switch:** `dnf reinstall` itself, `--remove`, Start at login, and the
"Going back" commands. The import and the speed test were checked separately on a service from the
installed files (`docs/v4/frontend/README.md`), not on this exact run.
