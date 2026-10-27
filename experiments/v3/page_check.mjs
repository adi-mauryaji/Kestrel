// experiments/v3/page_check.mjs -- v3's acceptance run of the browser page (before the freeze), over the
// Chrome DevTools Protocol, headless or headed; Node 24's own fetch and WebSocket, no package.
//
//   node experiments/v3/page_check.mjs CDP_PORT PAGE_URL OUT.json [SHOT.png]
//
// Against a scratch `kestrel serve` (never the human's): loads the page with its key, then: the task list
// and day strip are drawn; a Try button sends its request; a task made on the page is deleted after the
// page's own y/N question is answered Yes; a code request shows a code box whose Copy button (a real mouse
// click) puts the code on the clipboard; a chat request streams its text and ends with the caveat.
import { writeFileSync } from "node:fs";

const [, , port, url, outPath, shotPath] = process.argv;
const tab = await (await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent(url)}`, { method: "PUT" })).json();
const ws = new WebSocket(tab.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r, { once: true }));
let id = 0;
const call = (method, params = {}) => new Promise((resolve) => {
  const n = ++id;
  const on = (ev) => { const m = JSON.parse(ev.data); if (m.id === n) { ws.removeEventListener("message", on); resolve(m.result ?? { error: m.error }); } };
  ws.addEventListener("message", on);
  ws.send(JSON.stringify({ id: n, method, params }));
});
const js = async (expr) => (await call("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true })).result?.value;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const origin = new URL(url).origin;
await call("Browser.grantPermissions", { origin, permissions: ["clipboardReadWrite", "clipboardSanitizedWrite"] });
await call("Emulation.setDeviceMetricsOverride", { width: 900, height: 1300, deviceScaleFactor: 1, mobile: false });
await sleep(2500);

// The newest turn's reply as [kind, text] items; a question is answered `yes` when it shows.
async function send(text, yes = true, viaTry = false) {
  if (viaTry) await js(`[...document.querySelectorAll("#try button")].find(b => b.textContent === ${JSON.stringify(text)}).click(); true`);
  else await js(`document.getElementById("say").value = ${JSON.stringify(text)}; document.getElementById("ask").requestSubmit(); true`);
  let asked = null;
  for (let i = 0; i < 300; i++) {
    await sleep(500);
    const q = await js(`(() => { const b = document.querySelector("#log li .reply .question button.answer${yes ? ".yes" : ":not(.yes)"}"); if (!b) return null; const t = b.closest(".question").querySelector("p").textContent; b.click(); return t; })()`);
    if (q) asked = q;
    if (i > 2 && await js(`!document.body.classList.contains("thinking")`)) break;
  }
  const items = await js(`[...document.querySelectorAll("#log li:first-child .reply > *")].map(e => [e.className || e.tagName.toLowerCase(), e.textContent])`);
  return { request: text, asked, items };
}

const out = { url: origin, steps: [] };
const step = (name, ok, detail) => { out.steps.push({ name, ok: !!ok, detail }); console.log(`${ok ? "PASS" : "FAIL"} ${name}`); };

const loaded = await js(`({ tasks: document.querySelectorAll("#tasks li").length, day: !!document.getElementById("strip"),
  tries: [...document.querySelectorAll("#try button")].map(b => b.textContent), welcome: document.getElementById("welcome")?.textContent })`);
step("page loads with the day strip, its Try buttons and the skills", loaded.day && loaded.tries.length >= 4 && /Python code/.test(loaded.welcome || ""), loaded);

const listed = await send("mere tasks btao", true, true);
step("a Try button sends its request and the reply lists the tasks", listed.items.some(([, t]) => /#\d/.test(t)), listed);

const made = await send("remind me to water the plants tomorrow at 7 am");
const num = (made.items.map(([, t]) => t).join(" ").match(/created task #(\d+)/) || [])[1];
step("a task made on the page", !!num, made);
await sleep(1000);
const panel = await js(`document.getElementById("tasks").textContent.includes("water the plants")`);
step("the task list shows it", panel, { panel });
const gone = await send(`delete task ${num}`, true);
step("the page's own question, answered Yes, deletes it", gone.asked && gone.items.some(([, t]) => /deleted/i.test(t)), gone);

const code = await send("write a python function that returns the factorial of a number");
const box = code.items.find(([k]) => k === "codebox");
step("a code request shows a code box and the sandbox check", !!box && code.items.some(([, t]) => /safe sandbox/.test(t)), code);
let copied = null;
if (box) {
  const r = await js(`(() => { const b = document.querySelector("#log li:first-child .codebox .copy"); b.scrollIntoView({ block: "center" }); const q = b.getBoundingClientRect(); return { x: q.x + q.width / 2, y: q.y + q.height / 2 }; })()`);
  for (const type of ["mousePressed", "mouseReleased"]) await call("Input.dispatchMouseEvent", { type, x: r.x, y: r.y, button: "left", clickCount: 1 });
  await sleep(800);
  const label = await js(`document.querySelector("#log li:first-child .codebox .copy").textContent`);
  copied = await js(`navigator.clipboard.readText().then(t => t, e => "unreadable: " + e.message)`);
  const shown = await js(`document.querySelector("#log li:first-child .codebox code").textContent`);
  step("Copy puts the code on the clipboard", label === "Copied" && copied === shown, { label, copied: copied?.slice(0, 80) });
}

const chat = await send("how can i focus better while studying?");
step("a chat answer is written and carries the caveat", chat.items.some(([k, t]) => t.length > 80) && chat.items.some(([, t]) => /small model: it may be wrong/.test(t)), chat);

if (shotPath) {
  await js(`window.scrollTo(0, 0); true`);
  const shot = await call("Page.captureScreenshot", { format: "png" });
  writeFileSync(shotPath, Buffer.from(shot.data, "base64"));
}
out.passed = out.steps.filter((s) => s.ok).length;
out.total = out.steps.length;
writeFileSync(outPath, JSON.stringify(out, null, 1) + "\n");
console.log(`${out.passed}/${out.total}`);
ws.close();
process.exit(0);
