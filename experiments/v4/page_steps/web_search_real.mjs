// v4 D as real as it goes here: the real kestrel-net, a real public page, real Gemma, and for the provider
// SEARX: a SearXNG on this device, or the stand-in experiments/v4/fake_searxng.py.
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
  check("the provider is taken", await change({ search_provider: process.env.SEARX }), null);
  await js(`(() => { document.getElementById("say").value = "search the web for what is a mango"; document.getElementById("ask").requestSubmit(); return true; })()`);
  await wait(200);
  check("an answer comes", await until(async () => (await free()) && (await newest()).length > 0, 400), await newest());
  const text = await newest();
  log(text);
  check("with its sources", text.includes("Sources:"), text);
  check("they have addresses", /https:\/\//.test(text), text);
  const s = await js(`api("/api/outbound").then((r) => r.json())`);
  log(JSON.stringify(s.log.map((r) => [r.kind, r.host, r.what, r.result])));
  check("the search is in the log with its words", s.log.some((r) => r.kind === "search" && r.what === "what is a mango" && r.result === "done"), s.log);
  check("the page read is in the log", s.log.some((r) => r.kind === "fetch" && r.host === "en.wikipedia.org" && r.result === "done"), s.log);
};
