"use strict";
// The shell's page while Kestrel's service is not running. On a first start the notes folder is
// chosen here (the app's commands first_start and use_notes, src/main.rs); Kestrel then starts and
// this window gives way to its page.
const $ = (id) => document.getElementById(id);
const tauri = window.__TAURI__;

function code(text) {
  const c = document.createElement("code");
  c.textContent = text;
  return c;
}

function notRunning() {
  $("greet").textContent = "Kestrel";
  $("title").textContent = "Kestrel is not running";
}

async function start() {
  if (!tauri) {
    // Opened outside the app (or its commands are missing): the terminal way.
    notRunning();
    $("about").replaceChildren("Choose your notes folder in a terminal: ", code("kestrel settings notes ~/notes"),
      ", then Open Kestrel from the tray icon.");
    return;
  }
  let s = { saved: false, folder: null, log: null, why: null };
  try { s = await tauri.core.invoke("first_start"); } catch (e) { /* offer an empty field */ }
  $("folder").value = s.folder || "";
  if (s.saved) {
    // A folder is saved, yet the service stopped (the folder gone, Kestrel's files missing …).
    notRunning();
    $("about").textContent = (s.why ? `It stopped: ${s.why}.` : "It did not start with this notes folder.")
      + " Start it again, or choose another folder.";
    if (s.log) $("hint").replaceChildren("Its log: ", code(s.log));
    $("use").textContent = "Start Kestrel";
  } else {
    $("hint").textContent = "A full path. Kestrel makes the folder if it is not there yet.";
  }
  $("choose").hidden = false;
  $("folder").focus();
}

$("choose").addEventListener("submit", async (e) => {
  e.preventDefault();
  const use = $("use"), hint = $("hint"), label = use.textContent;
  use.disabled = true;
  use.textContent = "Starting…";
  hint.classList.remove("bad");
  hint.textContent = "Kestrel is starting; the first time takes a few seconds.";
  try {
    await tauri.core.invoke("use_notes", { folder: $("folder").value });
  } catch (err) {
    hint.classList.add("bad");
    hint.textContent = String(err);
    use.disabled = false;
    use.textContent = label;
  }
});

start();
