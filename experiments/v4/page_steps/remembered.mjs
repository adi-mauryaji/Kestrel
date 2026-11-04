// Settings' "What Kestrel remembers", with 2 facts and a conversation of 2 turns kept. OUT: where the
// screenshot goes.
export default async ({ js, shot, wait, check }) => {
  await js(`document.querySelector('.nav button[data-view="settings"]').click(); true`);
  await wait(1200);
  const rows = () => js(`[...document.querySelectorAll("#kept-rows .kept-row span")].map((s) => s.textContent)`);
  let r = await rows();
  check("the kept facts are listed", r.length === 2 && r.includes("my sister's name is Meera"), r);
  check("the conversations can be forgotten", await js(`!document.getElementById("kept-forget").hidden && document.getElementById("kept-forget").textContent.includes("2 turns")`), null);
  await js(`document.getElementById("kept-h").scrollIntoView({ block: "center" }); true`);
  await wait(200);
  await shot(`${process.env.OUT}/remembered.jpg`);
  await js(`document.querySelector("#kept-rows .kept-row button").click(); true`);
  await wait(500);
  r = await rows();
  check("Forget removes one fact", r.length === 1, r);
  await js(`document.getElementById("kept-forget").click(); true`);
  await wait(500);
  check("Forget conversations removes them and hides itself", await js(`document.getElementById("kept-forget").hidden`), null);
  // A Forget that fails says why; the next one that works takes the line back, as out-line does.
  const line = () => js(`document.getElementById("kept-line").textContent`);
  const said = await line();
  await js(`window.fetchWas = window.fetch; window.fetch = () => Promise.reject(new Error("the line is down")); document.querySelector("#kept-rows .kept-row button").click(); true`);
  await wait(300);
  check("a Forget that fails says why", (await line()) === "the line is down", await line());
  await js(`window.fetch = window.fetchWas; document.querySelector("#kept-rows .kept-row button").click(); true`);
  await wait(500);
  r = await rows();
  check("the next Forget works", r.length === 0, r);
  check("and the old error is gone", (await line()) === said && await js(`!document.getElementById("kept-line").classList.contains("bad")`), await line());
};
