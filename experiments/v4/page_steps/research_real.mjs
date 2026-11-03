// v4.1 research as real as it goes here: the real kestrel-net, real public pages, real Gemma; SEARX is a
// SearXNG or the stand-in experiments/v4/fake_searxng.py. How long it takes is the point.
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
  const change = (c) => js(`api("/api/outbound", ${JSON.stringify(c)}).then((r) => r.ok)`);
  await change({ internet: true });
  await change({ search_provider: process.env.SEARX });
  const t0 = Date.now();
  await js(`(() => { document.getElementById("say").value = "research what is a mango"; document.getElementById("ask").requestSubmit(); return true; })()`);
  await wait(200);
  check("an answer comes", await until(async () => (await free()) && (await newest()).length > 0, 450), await newest());
  log(`${Math.round((Date.now() - t0) / 1000)} s, the first request also loading the model`);
  const text = await newest();
  log(text);
  check("with sources", text.includes("Sources:") && text.includes("wikipedia.org"), text);
  const s = await js(`api("/api/outbound").then((r) => r.json())`);
  log(JSON.stringify(s.log.map((r) => [r.kind, r.host, r.what, r.result])));
  check("all in the log", s.log.filter((r) => r.kind === "fetch").length === 2, s.log);
};
