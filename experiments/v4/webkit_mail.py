#!/usr/bin/env python3
"""The mail question in real WebKitGTK, the engine of the Linux app (page_check.mjs drives Chrome). A page address in
a file; says the requests and reports each second what the page shows when it changes.

    python3 experiments/v4/webkit_mail.py URL_FILE [yes|no] ["the request"]

Exit 0 when the question reached the page and the answer ended the request.
"""
import json
import pathlib
import sys
import time

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

url = pathlib.Path(sys.argv[1]).read_text().strip()
answer = sys.argv[2] if len(sys.argv) > 2 else "no"
request = sys.argv[3] if len(sys.argv) > 3 else "anup ko mail kr do main kl chutti pr hu"

STATE = """JSON.stringify({
  free: !document.getElementById("send").disabled,
  asked: document.querySelector(".question .answer") ? document.querySelector(".question p").textContent.slice(0, 60) : "",
  row: (document.querySelector(".turn-sub, .row-sub, .sub") || {}).textContent || "",
  detail: (document.querySelector(".detail") || {}).textContent || "",
})"""


def say(words: str) -> str:
    return ("(() => { document.querySelector('.nav button[data-view=\"home\"]').click(); "
            f'document.getElementById("say").value = {json.dumps(words)}; '
            'document.getElementById("ask").requestSubmit(); return "said"; })()')


view = WebKit2.WebView()
view.get_settings().set_enable_write_console_messages_to_stdout(True)
window = Gtk.Window(title="Kestrel: WebKit check")
window.set_default_size(1200, 860)
window.add(view)
window.show_all()
result = {"code": 1}
t0 = time.time()


def steps():
    yield ("sleep", 3000)
    yield ("js", say("add contact Anup anup@example.com"))
    yield ("sleep", 1500)
    yield ("js", say(request))
    print(f"said: {request}", flush=True)
    start, last, answered = time.time(), None, False
    while time.time() - start < 200:
        yield ("sleep", 1000)
        state = json.loads((yield ("js", STATE)))
        seen = (bool(state["detail"]), state["asked"][:20], state["free"], "Sent" in state["detail"], "Not sent" in state["detail"])
        if seen != last:
            print(f"{time.time() - start:6.1f}s text={len(state['detail'])} chars, question={state['asked']!r}, "
                  f"send button free={state['free']}", flush=True)
            last = seen
        if state["asked"] and not answered:
            answered = True
            print(f"{time.time() - start:6.1f}s the question reached the page; answering {answer}", flush=True)
            word = answer.capitalize()
            yield ("js", f'[...document.querySelectorAll(".question .answer")].find((x) => x.textContent === "{word}").click(); "clicked"')
        elif answered and state["free"]:
            print(f"{time.time() - start:6.1f}s it ended: {state['detail'][-120:]!r}", flush=True)
            result["code"] = 0
            return
    print("no end within 200 s: the question did not reach the page" if not answered else "no end after the answer", flush=True)


flow = steps()


def advance(value=None):
    try:
        kind, arg = flow.send(value)
    except StopIteration:
        Gtk.main_quit()
        return False
    if kind == "sleep":
        GLib.timeout_add(arg, advance)
    else:
        view.evaluate_javascript(arg, -1, None, None, None, finished, None)
    return False


def finished(_view, res, _data):
    try:
        advance(view.evaluate_javascript_finish(res).to_string())
    except GLib.Error as e:
        print("the page's script failed:", e.message, flush=True)
        Gtk.main_quit()


def loaded(_view, event):
    if event == WebKit2.LoadEvent.FINISHED:
        advance()


view.connect("load-changed", loaded)
view.load_uri(url)
Gtk.main()
sys.exit(result["code"])
