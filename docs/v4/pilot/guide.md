# Trying Kestrel for two weeks

Thank you for trying this. Kestrel is an assistant that runs on your own laptop. It keeps your tasks,
reminds you, and finds things in your notes and documents. It understands English and Hinglish. What you
type stays on your laptop unless you say yes to sending something.

This is an early test, so things will break. Please tell us when they do; that is the most useful thing you
can do. Keep your own copies of anything important.

**Questions or problems:** [organiser's name and contact]
**Get the installer here:** [link]

## What you need

- Windows 10 or 11 (64-bit), a Mac with an M chip, or Linux (Ubuntu or Fedora style)
- 8 GB of memory or more
- About 5 GB of free disk space

## Install it

The installer is not signed yet, so your laptop will warn you. That is expected for this test.

**Windows:** Run the `.exe`. If you see "Windows protected your PC", press **More info**, then **Run anyway**.

**Mac:** Open the `.dmg` and drag Kestrel to Applications. Open it once. When the Mac refuses, go to **System
Settings → Privacy & Security** and press **Open Anyway**.

**Linux:** Install the file for your system: `sudo apt install ./Kestrel*.deb` or
`sudo dnf install ./Kestrel*.rpm`.

## The first open

1. **Your name.** It stays on your laptop.
2. **Your notes.** Kestrel shows the folder it reads your notes from. Put notes and documents you want it to
   know about in there. It reads text files and PDFs.
3. **This device.** Kestrel shows what it can do on your laptop. To get all of it, it needs its model, a file
   of about 2.8 GB: turn the internet on there and press **Download**. It only needs to happen once, and the
   download can continue if it stops. Then press **Measure speed** (about a minute) and **Continue**.

## Keep it running

Reminders only work while Kestrel is running. Closing the window leaves it running as a small icon near the
clock.

Click that icon and tick **Start at login**, so that it is there each time you turn on your laptop.

## Things to try

Type these in the box on Home, in your own words:

- `kal shaam 6 baje bill bharna yaad dila dena`
- `show my tasks`
- `my sister's name is Meera`, and later ask about it
- `mera PAN kahan hai`, once a PAN document is in your notes folder
- `draft a mail to my teacher asking for leave on Friday`
- A question about something in your notes
- `why this answer`, after any answer

**The internet** is off until you turn it on in Settings. Weather, news and web search need it, and a web
search sends your words to the search service you chose. A mail leaves only after you say yes to the exact
text. Everything that goes out is listed in Settings.

## For the two weeks

Use it when it would actually help, and leave it when it would not. We are not testing you, and "I stopped
using it" is a useful answer.

We may send you a short message after a few days and after a week.

## At the end

1. Open **Settings → How Kestrel is used**. It shows how many times each thing was used each day, and never
   what you said. Read it, press **Copy**, and send it to us. Sending it is up to you.
2. Fill in the short survey we send you.

## Stopping or removing it

You can stop at any time, and you do not have to say why.

1. If you set up Gmail, press **Forget** next to it in Settings, which removes the app password.
2. Quit from the icon near the clock, then uninstall:
   - **Windows:** Settings → Apps → Kestrel → Uninstall
   - **Mac:** Drag Kestrel from Applications to the Trash
   - **Linux:** `sudo apt remove kestrel` or `sudo dnf remove kestrel`
3. Uninstalling leaves your tasks and the model behind. To remove those too, delete these folders:
   - **Windows:** `%LOCALAPPDATA%\kestrel`
   - **Mac:** `~/Library/Application Support/kestrel` and `~/Library/Caches/kestrel`
   - **Linux:** `~/.local/share/kestrel` and `~/.cache/kestrel`

Your notes folder is yours, and Kestrel leaves it as it is.
