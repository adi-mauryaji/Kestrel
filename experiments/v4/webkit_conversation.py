#!/usr/bin/env python3
"""A scenario said in real WebKitGTK, the Linux app's own engine, as page_steps/conversation.mjs does in Chrome: the
same steps and the same results file. Run by app_test.py --engine webkit, with the system Python (it has gi).

More steps here: {"say": ..., "answer": ["none"]} leaves a question unanswered until its wait runs out;
{"js": "an expression", "want": value} checks what the page's own code gives; {"touch": "bin"} changes the running
program's file, as an install does.

    /usr/bin/python3 -I webkit_conversation.py URL_FILE SCENARIO RESULTS OUT PROGRAM
"""
import json
import os
import pathlib
import sys
import time

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

url_file, scenario, results, out, program = sys.argv[1:6]
url = pathlib.Path(url_file).read_text().strip()
steps = json.loads(pathlib.Path(scenario).read_text())

FREE = '!document.getElementById("send").disabled'
WAITING = ('(document.querySelector(".question .answer")?.closest(".question").querySelector("p").textContent ?? "")')
NEWEST = '((document.querySelector(".detail")?.innerText || document.querySelector(".detail")?.textContent || "").trim())'

view = WebKit2.WebView()
view.get_settings().set_enable_write_console_messages_to_stdout(True)
window = Gtk.Window(title="Kestrel: WebKit test")
window.set_default_size(1440, 900)
window.add(view)
window.show_all()
failed = [0]


def write(row):
    with open(results, "a") as f:
        f.write(json.dumps(row, ensure_ascii=False) + "\n")


def driver():
    yield ("sleep", 2500)
    for i, step in enumerate(steps):
        if "say" in step:
            answers = list(step.get("answer", []))
            t0 = time.time()
            for _ in range(600):
                if json.loads((yield ("js", f"JSON.stringify({FREE})"))):
                    break
                yield ("sleep", 500)
            yield ("js", '(() => { document.querySelector(\'.nav button[data-view="home"]\').click(); '
                         f'document.getElementById("say").value = {json.dumps(step["say"])}; '
                         'document.getElementById("ask").requestSubmit(); return "said"; })()')
            yield ("sleep", 200)
            questions, ended, deadline = [], False, time.time() + 400
            while time.time() < deadline:
                state = json.loads((yield ("js", f'JSON.stringify({{free: {FREE}, q: {WAITING}}})')))
                if state["q"] and not state["free"] and not any(q["q"] == state["q"] for q in questions):
                    a = answers.pop(0) if answers else "no"
                    questions.append({"q": state["q"], "a": a, "after_s": round(time.time() - t0, 1)})
                    if a != "none":
                        yield ("js", '[...document.querySelectorAll(".question .answer")]'
                                     f'.find((x) => x.textContent.toLowerCase() === {json.dumps(a)})?.click(); "clicked"')
                    yield ("sleep", 300)
                    continue
                if state["free"]:
                    ended = True
                    break
                yield ("sleep", 300)
            reply = json.loads((yield ("js", f"JSON.stringify({NEWEST})")))
            write({"i": i, "say": step["say"], "seconds": round(time.time() - t0, 1), "timed_out": not ended,
                   "questions": questions, "reply": reply})
            print(f"{i} {round(time.time() - t0, 1)}s {step['say'][:50]}", flush=True)
            failed[0] += not ended
        elif "view" in step:
            yield ("js", f'document.querySelector(\'.nav button[data-view="{step["view"]}"]\').click(); "shown"')
            yield ("sleep", 2500)
            text = json.loads((yield ("js", f'JSON.stringify(document.getElementById("view-{step["view"]}").innerText)')))
            write({"i": i, "view": step["view"], "text": text})
        elif "change" in step:
            yield ("js", f'api("/api/outbound", {json.dumps(step["change"])}); "asked"')
            yield ("sleep", 1500)
            write({"i": i, "change": step["change"]})
        elif "theme" in step:
            yield ("js", f'document.getElementById({json.dumps("dark" if step["theme"] == "dark" else "light")}).click(); "set"')
            yield ("sleep", 300)
        elif "shot" in step:
            yield ("shot", step["shot"])
        elif "js" in step:
            got = json.loads((yield ("js", f'JSON.stringify({step["js"]})')))
            good = got == step["want"]
            failed[0] += not good
            write({"i": i, "js": step["js"][:120], "want": step["want"], "got": got, "pass": good})
            print(f"{i} {'PASS' if good else 'FAIL'} {step['js'][:70]}" + ("" if good else f" saw {got!r}"), flush=True)
        elif "touch" in step:
            os.utime(program, (time.time() + 7, time.time() + 7))
            write({"i": i, "touch": program})


flow = driver()


def advance(value=None):
    try:
        kind, arg = flow.send(value)
    except StopIteration:
        Gtk.main_quit()
        return False
    if kind == "sleep":
        GLib.timeout_add(arg, advance)
    elif kind == "js":
        view.evaluate_javascript(arg, -1, None, None, None, finished, None)
    else:
        view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, snapped, arg)
    return False


def finished(_view, res, _data):
    try:
        advance(view.evaluate_javascript_finish(res).to_string())
    except GLib.Error as e:
        print("FAIL the page's script:", e.message, flush=True)
        failed[0] += 1
        Gtk.main_quit()


def snapped(_view, res, name):
    view.get_snapshot_finish(res).write_to_png(f"{out}/{name}.png")
    advance()


view.connect("load-changed", lambda _v, e: advance() if e == WebKit2.LoadEvent.FINISHED else None)
view.load_uri(url)
Gtk.main()
print(f"{failed[0]} failed", flush=True)
sys.exit(1 if failed[0] else 0)
