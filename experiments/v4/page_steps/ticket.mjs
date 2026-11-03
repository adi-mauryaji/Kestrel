// v4 E on the page: real PDFs (Chrome's print of experiments/v4/tickets/*.html, made up, not real bookings),
// read by the real `kestrel pdf-text`, filed after a yes with a reminder. TICKETS: the folder with the PDFs.
import { existsSync, readdirSync } from "node:fs";

export default async ({ js, wait, check, log }) => {
  const until = async (test, seconds) => {
    for (let i = 0; i < seconds * 2; i++) {
      if (await test()) return true;
      await wait(500);
    }
    return false;
  };
  const free = () => js(`!document.getElementById("send").disabled`);
  const newest = () => js(`document.querySelector(".detail")?.textContent ?? ""`);
  const waiting = () => js(`document.querySelector(".question .answer")?.closest(".question").querySelector("p").textContent ?? ""`);
  const say = async (words) => {
    await until(free, 60);
    await js(`(() => { document.getElementById("say").value = ${JSON.stringify(words)}; document.getElementById("ask").requestSubmit(); return true; })()`);
    await wait(150);
  };
  const answer = async (word) => {
    await js(`[...document.querySelectorAll(".question .answer")].find((x) => x.textContent === ${JSON.stringify(word)})?.click(); true`);
    await wait(150);
  };
  const tasks = () => js(`api("/api/tasks").then((r) => r.json()).then((t) => JSON.stringify(t))`);
  const filed = () => { const d = `${process.env.NOTES}/tickets`; return existsSync(d) ? readdirSync(d) : []; };
  const train = `${process.env.TICKETS}/train.pdf`;

  await say(`file this ticket ${train}`);
  check("a yes is asked", await until(async () => (await waiting()).startsWith("File this ticket?"), 25), await newest());
  const shown = await waiting();
  log(shown);
  for (const part of ["Train 12004", "NEW DELHI (NDLS) → LUCKNOW NR (LKO)", "15 Nov 2026 at 06:10", "PNR 2134567890", "tickets/2026-11-15-train-12004.pdf", "14 Nov 2026 at 21:00"]) {
    check(`it shows ${part}`, shown.includes(part), shown);
  }
  await answer("No");
  check("No: Not filed", await until(async () => (await free()) && (await newest()).includes("Not filed."), 8), await newest());
  check("nothing was filed", filed().length === 0, filed());
  check("nothing was set", !(await tasks()).includes("12004"), await tasks());

  await say(`ye ticket file karo ${train}`);
  await until(async () => (await waiting()).startsWith("File this ticket?"), 25);
  await answer("Yes");
  check("Yes: filed and reminded", await until(async () => (await free()) && (await newest()).includes("remind you 14 Nov 2026 at 21:00"), 10), await newest());
  check("the PDF is in the notes folder", filed().includes("2026-11-15-train-12004.pdf"), filed());
  const t = await tasks();
  check("the reminder is a task with its time", t.includes("Train 12004 NEW DELHI (NDLS) → LUCKNOW NR (LKO) at 06:10 on 15 Nov") && t.includes("21:00"), t);
  check("the PNR stays out of the reminder", !t.includes("2134567890"), t);

  await say(`file this ticket ${process.env.TICKETS}/flight.pdf`);
  check("a flight is read too", await until(async () => (await waiting()).includes("Flight 6E 2341 DEL → BOM"), 25), await waiting());
  log(await waiting());
  await answer("Yes");
  await until(free, 10);
  check("and filed", filed().includes("2026-11-15-flight-6e-2341.pdf"), filed());
  await say(`file this ticket ${process.env.TICKETS}/missing.pdf`);
  check("a missing file is said", await until(async () => (await free()) && (await newest()).includes("no file"), 10), await newest());
  check("no yes for it", (await waiting()) === "", await waiting());
  await js(`document.querySelector('.nav button[data-view="notes"]').click(); true`);
  await wait(1500);
  const notes = await js(`document.body.innerText`);
  check("the Notes page lists it, by what it says", notes.includes("3 notes") && notes.includes("Electronic Reservation Slip"), notes.slice(0, 500));
};
