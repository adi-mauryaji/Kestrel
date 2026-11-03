// Drives Kestrel's page in headless Chrome over the DevTools protocol (Node 24: its own fetch and
// WebSocket, no package), for v4's checks against a scratch service.
//   google-chrome --headless=new --remote-debugging-port=9223 --user-data-dir=DIR about:blank &
//   node experiments/v4/page_check.mjs 9223 FILE_WITH_THE_PAGE_ADDRESS STEPS.mjs
// STEPS default-exports async ({ js, shot, wait, check }) => {}. Exit 0 when every check passed.
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const [port, urlFile, stepsFile] = process.argv.slice(2);
const url = readFileSync(urlFile, "utf8").trim();
const tab = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
let id = 0;
const pending = new Map();
const errors = [];
let ws;
let reopened = 0;
const listen = () => new Promise((ok, fail) => {
  ws = new WebSocket(tab.webSocketDebuggerUrl);
  ws.onopen = ok;
  ws.onerror = fail;
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.method === "Inspector.targetCrashed") console.log("FAIL the page crashed");
    if (msg.id && pending.has(msg.id)) { pending.get(msg.id).ok(msg); pending.delete(msg.id); }
    if (msg.method === "Runtime.exceptionThrown") errors.push(msg.params.exceptionDetails.exception?.description || msg.params.exceptionDetails.text);
  };
  // A wifi change closes it with the browser and the service both alive (code 1006; on 2026-11-04 each
  // closing fell on one of this laptop's wifi drops, every 10 to 15 minutes), so it is opened again and
  // what was waiting is asked again. Said each time, and not a check: the run's result stands.
  ws.onclose = async (e) => {
    console.log(`(the browser connection closed, code ${e.code}; opening it again, ${pending.size} asked again)`);
    reopened++;
    try {
      await listen();
    } catch {
      console.log("FAIL the browser is gone");
      process.exit(1);
    }
    for (const [i, p] of pending) ws.send(JSON.stringify({ id: i, method: p.method, params: p.params }));
  };
});
await listen();
const send = (method, params = {}) => new Promise((ok) => {
  const i = ++id;
  pending.set(i, { ok, method, params });
  ws.send(JSON.stringify({ id: i, method, params }));
});
await send("Runtime.enable");
await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
await send("Page.navigate", { url });
const wait = (ms) => new Promise((ok) => setTimeout(ok, ms));
await wait(2500);
const js = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || "eval failed");
  return r.result?.result?.value;
};
const shot = async (file) => {
  const r = await send("Page.captureScreenshot", { format: "jpeg", quality: 85 });
  writeFileSync(file, Buffer.from(r.result.data, "base64"));
};
let passed = 0, failed = 0;
const check = (name, ok, seen) => {
  if (ok) passed++; else failed++;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${ok ? "" : ` — saw: ${JSON.stringify(seen)}`}`);
};
try {
  await (await import(resolve(stepsFile))).default({ js, shot, wait, check, log: console.log });
} catch (e) {
  failed++;
  console.log("FAIL (threw)", e.message);
}
console.log(`${passed} passed, ${failed} failed; script errors on the page: ${errors.length ? errors.join(" | ") : "none"}`);
if (reopened) console.log(`(the browser connection was opened again ${reopened} time${reopened === 1 ? "" : "s"})`);
ws.onclose = null;
ws.close();
process.exit(failed ? 1 : 0);
