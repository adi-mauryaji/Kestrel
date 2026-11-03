// v4 D: the morning brief on asking, and from the service's own thread at the time chosen, once, with the
// weather reminder; a stand-in kestrel-net whose forecast says rain. No model needed.
export default async ({ js, shot, wait, check, log }) => {
  const until = async (test, seconds) => {
    for (let i = 0; i < seconds * 2; i++) {
      if (await test()) return true;
      await wait(500);
    }
    return false;
  };
  const free = () => js(`!document.getElementById("send").disabled`);
  const newest = () => js(`document.querySelector(".detail")?.textContent ?? ""`);
  const saying = async (words, want, seconds = 20) => {
    await until(free, 60);
    await js(`(() => { document.getElementById("say").value = ${JSON.stringify(words)}; document.getElementById("ask").requestSubmit(); return true; })()`);
    await wait(150);
    check(`"${words}" → ${want}`, await until(async () => (await free()) && (await newest()).includes(want), seconds), await newest());
  };
  const change = (c) => js(`api("/api/outbound", ${JSON.stringify(c)}).then((r) => r.ok)`);
  const state = () => js(`api("/api/outbound").then((r) => r.json())`);
  const events = () => js(`api("/api/events?after=0").then((r) => r.json())`);
  const titles = async () => {
    const e = await events();
    return (e.events || []).map((x) => x.title);
  };

  await saying("remind me to call Asha today", "call asha");
  await saying("morning brief", "Today: call asha");
  let s = await state();
  check("off: nothing asked for it", s.log.length === 0, s.log);

  await change({ internet: true });
  await saying("my city is Delhi", "A State, India");
  await saying("add feed https://news.example/rss", "news.example/rss");
  await saying("aaj ka brief", "Headline 2");
  const text = await newest();
  log(text);
  check("the weather is in it", text.includes("Delhi today: rain"), text);
  check("so is the reminder", text.includes("take an umbrella"), text);
  check("three headlines, not five", !text.includes("Headline 3"), text);
  const before = (await state()).log.length;

  check("no brief before a time is chosen", (await titles()).every((t) => !t.startsWith("Good morning")), await titles());
  const at = await js(`(() => { const d = new Date(); return String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0"); })()`);
  log(`the brief is set for ${at}`);
  await js(`document.querySelector('.nav button[data-view="settings"]').click(); true`);
  await wait(800);
  await js(`document.getElementById("brief-time").value = ${JSON.stringify(at)}; document.getElementById("brief-form").requestSubmit(); true`);
  check("the page says when", await until(() => js(`document.getElementById("brief-state").textContent.includes(${JSON.stringify(at)})`), 8), null);
  check("the service's thread gives it", await until(async () => (await titles()).some((t) => t === "Good morning, Tester"), 45), await titles());
  check("and the weather reminder", await until(async () => (await titles()).includes("The weather today"), 5), await titles());
  const e = await events();
  const brief = e.events.find((x) => x.title === "Good morning, Tester");
  log(brief.body);
  check("the brief has the day's task", brief.body.includes("call asha"), brief);
  s = await state();
  check("what it asked is in the log", s.log.length === before + 2, s.log);
  await wait(35000);
  const again = (await titles()).filter((t) => t === "Good morning, Tester");
  check("once a day", again.length === 1, again);
  check("no more asked", (await state()).log.length === before + 2, (await state()).log);
  await js(`document.getElementById("brief-off").click(); true`);
  check("it can be turned off", await until(async () => (await state()).brief_at === null, 8), await state());
};
