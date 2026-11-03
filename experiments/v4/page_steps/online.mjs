// v4 D: the weather and the news on the page, with a stand-in kestrel-net. Nothing is asked with the
// internet off; each request is in the log with its exact query. No model is needed for these.
export default async ({ js, shot, wait, check }) => {
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
  const free = () => js(`!document.getElementById("send").disabled`);
  const newest = () => js(`document.querySelector(".detail")?.textContent ?? ""`);
  const saying = async (words, want, seconds = 20) => {
    await until(free, 60);
    await js(`(() => { document.getElementById("say").value = ${JSON.stringify(words)}; document.getElementById("ask").requestSubmit(); return true; })()`);
    await wait(150);
    check(`"${words}" → ${want}`, await until(async () => (await free()) && (await newest()).includes(want), seconds), await newest());
  };
  const logRows = () => js(`[...document.querySelectorAll("#out-rows .out-row")].map((r) => r.innerText)`);
  const logHas = async (n) => {
    await view("settings");
    await until(async () => (await logRows()).length === n, 10);
    return logRows();
  };
  const fill = (form, field, value) => js(`(() => { document.getElementById(${JSON.stringify(field)}).value = ${JSON.stringify(value)}; document.getElementById(${JSON.stringify(form)}).requestSubmit(); return true; })()`);
  const line = () => js(`document.getElementById("out-line").textContent`);

  await view("home");
  await saying("aaj ka mausam", "internet is off");
  await saying("news", "internet is off");
  let rows = await logHas(0);
  check("off: nothing in the log", rows.length === 0, rows);
  await fill("place-form", "place-name", "Delhi");
  check("off: a place is refused, it would use the internet", await until(async () => (await line()).includes("turn it on first"), 8), await line());
  await fill("feed-form", "feed-url", "http://plain.example/rss");
  check("a feed over plain http is refused", await until(async () => (await line()).includes("https address"), 8), await line());
  await fill("search-form", "search-url", "http://search.example.org");
  check("a search provider over plain http is refused", await until(async () => (await line()).includes("on this device"), 8), await line());
  await js(`document.getElementById("search-url").value = ""; true`);
  await fill("search-form", "search-url", "https://searx.example.org/");
  check("a search provider is kept", await until(() => js(`document.getElementById("search-state").textContent.includes("https://searx.example.org.")`), 8), null);
  check("and can be removed", await js(`getComputedStyle(document.getElementById("search-forget")).display !== "none"`), null);

  await js(`document.getElementById("out-internet").click(); true`);
  await until(() => js(`document.getElementById("out-state").textContent === "On"`), 8);
  await view("home");
  await saying("aaj ka mausam", "my city is");
  await saying("news", "add feed");
  await saying("my city is Nowhere", "could not find");
  await saying("my city is Delhi", "A State, India");
  await saying("aaj ka mausam", "Delhi today: rain, 18–28°C, rain 80%. Now 20°C. (Open-Meteo)");
  await saying("kal barish hogi kya", "Delhi tomorrow: clear, 19–30°C, rain 0%. (Open-Meteo)");
  await saying("add feed https://news.example/rss", "news.example/rss");
  await saying("add feed https://broken.example/rss", "broken.example/rss");
  await saying("aaj ki khabar", "Headline 4");
  const news = await newest();
  check("five headlines from a feed, with its source", !news.includes("Headline 5") && news.includes("(from news.example)"), news);
  check("a broken feed says so", news.includes("broken.example") && news.includes("500"), news);
  await saying("write a poem about the weather", "");

  rows = await logHas(6);
  check("each request is in the log", rows.length === 6, rows);
  check("the exact query is there", rows.some((r) => r.includes("weather · api.open-meteo.com · Delhi (28.65, 77.23)")) && rows.some((r) => r.includes("rss · news.example · https://news.example/rss")), rows);
  check("the failure is there", rows.some((r) => r.includes("failed: the feed answered 500")), rows);
  check("the place is shown", (await js(`document.getElementById("place-state").textContent`)).includes("Delhi (A State, India)"), null);
  check("the cards moved on", await until(async () => { await js(`refresh(); true`); await wait(600); return !(await js(`document.body.innerText`)).includes("add your place (Settings)"); }, 10), null);
  await js(`document.getElementById("out-h").scrollIntoView({ block: "start" }); true`);
  await wait(200);
  await shot(`${process.env.OUT}/built-internet.jpg`);
};
