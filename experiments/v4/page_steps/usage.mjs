// Settings' "How Kestrel is used", the pilot's counts (blueprint §4.12), on a fresh service: requests by
// rules, so no model. What can be copied is what is shown, and nothing that was said is in it.
// OUT: where the screenshot goes.
export default async ({ js, shot, wait, check }) => {
  const until = async (test, seconds) => {
    for (let i = 0; i < seconds * 2; i++) {
      if (await test()) return true;
      await wait(500);
    }
    return false;
  };
  const free = () => js(`!document.getElementById("send").disabled`);
  const saying = async (words) => {
    await until(free, 60);
    await js(`(() => { document.getElementById("say").value = ${JSON.stringify(words)}; document.getElementById("ask").requestSubmit(); return true; })()`);
    await wait(150);
    await until(free, 60);
  };
  const settings = async () => {
    await js(`document.querySelector('.nav button[data-view="settings"]').click(); true`);
    await wait(1000);
    // Settings asks in turn, each answer after the last.
    await until(async () => (await shown()) !== "", 30);
  };
  const shown = () => js(`document.getElementById("usage-text").textContent`);
  const line = () => js(`document.getElementById("usage-line").textContent`);
  const said = await line();

  await settings();
  let u = JSON.parse(await shown());
  check("a fresh service has nothing counted", u.days.length === 0 && u.days_used === 0, u);
  check("and nothing to forget", await js(`document.getElementById("usage-forget").hidden`), null);

  await saying("show my tasks");
  await saying("my sister's name is Canary7f3a");
  await settings();
  await until(async () => JSON.parse(await shown()).days.length > 0, 15);
  const text = await shown();
  u = JSON.parse(text);
  const today = u.days[0]?.counts ?? {};
  check("today's requests are counted", today.request === 2 && u.days_used === 1, u);
  check("by the way each took", today.task_command === 1 && today.fact === 1, today);
  check("nothing that was said is there", !/canary|sister/i.test(text), text);
  check("it says what it is, and on which system", u.kestrel && ["linux", "macos", "windows"].includes(u.os), u);
  check("each capability's state is there", Object.keys(u.capabilities).length > 0 && Object.values(u.capabilities).every((s) => ["ready", "slow", "setup", "locked"].includes(s)), u.capabilities);
  check("and no detail of the device", !/ram|budget|gpu/i.test(text), text);
  check("now there is something to forget", await js(`!document.getElementById("usage-forget").hidden`), null);

  await js(`document.getElementById("usage-h").scrollIntoView({ block: "start" }); true`);
  await wait(200);
  await shot(`${process.env.OUT}/usage.jpg`);

  await js(`document.getElementById("usage-copy").click(); true`);
  await wait(300);
  check("Copy copies, or selects it and says so", /Copied|selected/.test(await line()), await line());

  await js(`window.fetchWas = window.fetch; window.fetch = () => Promise.reject(new Error("the line is down")); document.getElementById("usage-forget").click(); true`);
  await wait(300);
  check("a Forget that fails says why", (await line()) === "the line is down", await line());
  await js(`window.fetch = window.fetchWas; document.getElementById("usage-forget").click(); true`);
  await wait(500);
  u = JSON.parse(await shown());
  check("the next Forget empties them", u.days.length === 0, u);
  check("and hides itself", await js(`document.getElementById("usage-forget").hidden`), null);
  check("and the old line is back", (await line()) === said && await js(`!document.getElementById("usage-line").classList.contains("bad")`), await line());

  await saying("show my tasks");
  await settings();
  await until(async () => JSON.parse(await shown()).days.length > 0, 15);
  u = JSON.parse(await shown());
  check("counting goes on after a Forget", u.days[0]?.counts?.request === 1, u.days);
};
