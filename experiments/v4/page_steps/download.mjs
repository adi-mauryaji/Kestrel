// v4 C: a model from the internet, on the page, with a stand-in kestrel-net that copies the real file
// (experiments/v4/fake_net.py), so the hash check is real. No model on this device at the start.
export default async ({ js, shot, wait, check, log }) => {
  const until = async (test, seconds) => {
    for (let i = 0; i < seconds * 2; i++) {
      if (await test()) return true;
      await wait(500);
    }
    return false;
  };
  const view = async (name) => {
    await js(`document.querySelector('.nav button[data-view="${name}"]').click(); true`);
    await wait(300);
  };
  const go = () => js(`document.querySelectorAll("#download button")[0].textContent`);
  const hint = () => js(`document.querySelector("#download .hint").textContent`);
  const state = () => js(`api("/api/model/download").then((r) => r.json())`);
  const seen = (n) => js(`getComputedStyle(document.querySelectorAll("#download button")[${n}]).display !== "none"`);
  const logRows = () => js(`[...document.querySelectorAll("#out-rows .out-row")].map((r) => r.innerText)`);

  await view("settings");
  await until(async () => !(await js(`document.getElementById("download").hidden`)), 10);
  check("Download is offered with its size", (await go()).includes("2.6 GB"), await go());
  check("there is nothing to Stop yet", !(await seen(1)), null);
  check("the internet is off: the button waits", await js(`document.querySelectorAll("#download button")[0].disabled`), null);
  check("and says so", (await hint()).includes("Turn the internet on"), await hint());
  const refused = await js(`api("/api/model/download", { id: "gemma-4-E2B-it-Q4_0" }).then(async (r) => [r.status, await r.text()])`);
  check("the service refuses it too", refused[0] === 400 && refused[1].includes("internet is off"), refused);

  await js(`document.getElementById("out-internet").click(); true`);
  check("on: the button is free", await until(() => js(`!document.querySelectorAll("#download button")[0].disabled`), 8), await hint());
  await js(`document.querySelectorAll("#download button")[0].click(); true`);
  check("progress shows", await until(async () => /Downloading… \d+% \(\d/.test(await hint()) && (await state()).done > 0, 15), await hint());
  log(await hint());
  check("while it runs: Stop, and no Download", (await seen(1)) && !(await seen(0)), [await seen(0), await seen(1)]);
  await wait(2500);
  check("it runs while other requests are answered", await js(`(async () => { const t = Date.now(); const r = await api("/api/settings"); return r.ok && Date.now() - t < 800; })()`), null);
  await js(`document.getElementById("model-h").scrollIntoView({ block: "center" }); true`);
  await wait(200);
  await shot(`${process.env.OUT}/built-download.jpg`);
  await js(`document.querySelectorAll("#download button")[1].click(); true`);
  check("Stop stops it", await until(async () => (await state()).state === "stopped", 8), await state());
  const first = await state();
  check("what came is kept and offered", await until(async () => (await go()).startsWith("Continue download") && (await seen(0)) && !(await seen(1)), 8), [await go(), await seen(0), await seen(1)]);
  check("it is more than nothing", first.offers[0].here > 0, first);
  log(await go());
  await wait(1500);
  const after = await state();
  check("stopped means stopped", after.offers[0].here === first.offers[0].here, [first.offers[0].here, after.offers[0].here]);
  await view("settings");
  await until(async () => (await logRows()).length === 1, 8);
  let rows = await logRows();
  check("the stop is in the log, with where it came from", rows.length === 1 && rows[0].includes("download · huggingface.co") && rows[0].includes("stopped"), rows);

  await js(`document.querySelectorAll("#download button")[0].click(); true`);
  check("Continue goes on from there", await until(async () => { const s = await state(); return s.state !== "getting" || s.done > first.offers[0].here; }, 15), await state());
  log("the rest of the file, then its SHA-256 (a debug build: over a minute)");
  check("the hash check shows", await until(async () => (await hint()).includes("SHA-256…"), 120), await hint());
  check("downloaded and checked", await until(async () => (await state()).state === "done", 300), await state());
  check("the page says so", await until(async () => (await hint()).startsWith("Downloaded gemma-4-E2B"), 5), await hint());
  check("nothing is offered now", await until(async () => (await state()).offers.length === 0, 5), await state());
  check("the card moved on: a writer, then the speed test", await until(async () => { await js(`refresh(); true`); await wait(800); return js(`document.getElementById("model-line").textContent.includes("gemma-4-E2B") || !!lastCaps?.device?.writer`); }, 20), await js(`document.getElementById("model-line").textContent`));
  await view("settings");
  await until(async () => (await logRows()).length === 2, 8);
  rows = await logRows();
  check("each try is in the log", rows.length === 2 && rows[0].includes("· done") && rows[1].includes("stopped"), rows);
};
