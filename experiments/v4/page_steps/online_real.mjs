// v4 D with the real kestrel-net: real Open-Meteo, a real feed. No personal data: a place chosen for the test.
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
  const saying = async (words, want, seconds = 40) => {
    await until(free, 60);
    await js(`(() => { document.getElementById("say").value = ${JSON.stringify(words)}; document.getElementById("ask").requestSubmit(); return true; })()`);
    await wait(150);
    check(`"${words}" → ${want}`, await until(async () => (await free()) && (await newest()).includes(want), seconds), await newest());
    log(await newest());
  };
  await js(`api("/api/outbound", { internet: true }).then(() => true)`);
  await saying("my city is Delhi", "National Capital Territory of Delhi");
  await saying("aaj ka mausam", "(Open-Meteo)");
  await saying("kal ka mausam", "Delhi tomorrow:");
  await saying("add feed https://feeds.bbci.co.uk/news/world/rss.xml", "bbci.co.uk");
  await saying("news", "(from feeds.bbci.co.uk)");
};
