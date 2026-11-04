// Says each step of SCENARIO (a JSON list) on the page and writes what came back to RESULTS, one JSON line
// each. It judges nothing: reading the replies is the point. Run by experiments/v4/app_test.py.
//   {"say": "mere tasks btao", "answer": ["yes"]}   a request; a question not listed in "answer" gets No
//   {"view": "tasks"}                               opens a page and records its text
//   {"change": {"internet": true}}                  POST /api/outbound
//   {"theme": "dark"}                               the theme buttons
//   {"shot": "name"}                                OUT/name.jpg
import { appendFileSync, readFileSync } from "node:fs";

export default async ({ js, shot, wait, check }) => {
  const until = async (test, seconds) => {
    for (let i = 0; i < seconds * 2; i++) {
      if (await test()) return true;
      await wait(500);
    }
    return false;
  };
  const free = () => js(`!document.getElementById("send").disabled`);
  const waiting = () => js(`document.querySelector(".question .answer")?.closest(".question").querySelector("p").textContent ?? ""`);
  const newest = () => js(`(document.querySelector(".detail")?.innerText || document.querySelector(".detail")?.textContent || "").trim()`);
  const write = (row) => appendFileSync(process.env.RESULTS, JSON.stringify(row) + "\n");

  const steps = JSON.parse(readFileSync(process.env.SCENARIO, "utf8"));
  for (const [i, step] of steps.entries()) {
    if (step.say !== undefined) {
      const answers = [...(step.answer ?? [])];
      const t0 = Date.now();
      await until(free, 300);
      await js(`(() => { document.querySelector('.nav button[data-view="home"]').click(); document.getElementById("say").value = ${JSON.stringify(step.say)}; document.getElementById("ask").requestSubmit(); return true; })()`);
      await wait(200);
      const questions = [];
      let ended = false;
      for (;;) {
        ended = await until(async () => (await free()) || (await waiting()) !== "", 400);
        const q = await waiting();
        if (q === "") break;
        const a = answers.shift() ?? "no";
        questions.push({ q, a });
        await js(`[...document.querySelectorAll(".question .answer")].find((x) => x.textContent.toLowerCase() === ${JSON.stringify(a)})?.click(); true`);
        await wait(300);
      }
      write({ i, say: step.say, seconds: Math.round((Date.now() - t0) / 100) / 10, timed_out: !ended, questions, reply: await newest() });
      check(`#${i} answered`, ended, step.say);
    } else if (step.view) {
      await js(`document.querySelector('.nav button[data-view="${step.view}"]').click(); true`);
      await wait(2500);
      write({ i, view: step.view, text: await js(`document.getElementById("view-${step.view}").innerText`) });
    } else if (step.change) {
      const r = await js(`api("/api/outbound", ${JSON.stringify(step.change)}).then(async (r) => [r.status, await r.text()])`);
      write({ i, change: step.change, status: r[0], said: r[0] === 200 ? "ok" : r[1] });
    } else if (step.theme) {
      await js(`document.getElementById(${JSON.stringify(step.theme === "dark" ? "dark" : "light")}).click(); true`);
      await wait(300);
    } else if (step.shot) {
      await shot(`${process.env.OUT}/${step.shot}.jpg`);
    }
  }
};
