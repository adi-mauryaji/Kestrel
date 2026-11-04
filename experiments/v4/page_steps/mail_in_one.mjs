// One request to write and send a mail, on the page, in the words a person really typed: a real Gemma
// draft, a stand-in kestrel-net (experiments/v4/fake_net.py). Mail leaves only after a yes; a draft asked
// for on its own says how to send it. OUT: where the screenshot goes. FAKE_NET_LOG: the stand-in's log.
import { existsSync, readFileSync } from "node:fs";

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
  const waiting = () => js(`document.querySelector(".question .answer")?.closest(".question").querySelector("p").textContent ?? ""`);
  const say = async (words) => {
    await until(free, 300);
    await js(`(() => { document.getElementById("say").value = ${JSON.stringify(words)}; document.getElementById("ask").requestSubmit(); return true; })()`);
    await wait(150);
  };
  const answer = async (word) => {
    await js(`[...document.querySelectorAll(".question .answer")].find((x) => x.textContent === ${JSON.stringify(word)})?.click(); true`);
    await wait(150);
  };
  const ended = (want, seconds) => until(async () => (await free()) && (await newest()).includes(want), seconds);
  const sent = () => (existsSync(process.env.FAKE_NET_LOG) ? readFileSync(process.env.FAKE_NET_LOG, "utf8") : "").split("\n").filter((l) => l.includes("smtp")).length;

  await say("add contact Anup anup@example.com");
  check("the contact is kept", await ended("Anup's address is anup@example.com", 20), await newest());

  log("Hinglish, the request that got \"I can't do that\" (the first loads Gemma)");
  await say("anup ko mail kr do mai kl chutti pr hu");
  check("it asks before anything leaves", await until(async () => (await waiting()).startsWith("Send this mail?"), 240), await newest());
  let question = await waiting();
  log(question);
  check("the question names Anup's address", question.includes("To: anup@example.com"), question);
  check("nothing has left", sent() === 0, sent());
  await js(`document.querySelector(".question")?.scrollIntoView({ block: "center" }); true`);
  await wait(200);
  await shot(`${process.env.OUT}/mail-question.jpg`);
  await answer("No");
  check("a no sends nothing", await ended("Not sent.", 30), await newest());
  check("and nothing left", sent() === 0, sent());

  log("English, the request that only drafted");
  await say("send an email to anup regarding i day off tommorow");
  check("it asks too", await until(async () => (await waiting()).startsWith("Send this mail?"), 240), await newest());
  question = await waiting();
  log(question);
  await answer("Yes");
  check("a yes sends it", await ended("Sent to Anup", 60), await newest());
  check("through kestrel-net, once", sent() === 1, sent());

  log("A draft on its own");
  await say("write an email to anup about the meeting on monday");
  check("it says how to send it", await ended("To send it, say: send it to Anup", 240), await newest());
  check("and asks nothing", (await waiting()) === "", await waiting());
  check("still one sent", sent() === 1, sent());
  await say("send it to Anup");
  check("then it asks", await until(async () => (await waiting()).startsWith("Send this mail?"), 30), await newest());
  await answer("No");
  await ended("Not sent.", 30);

  await say("boss ko mail kr do mai late hu");
  check("someone not in the contacts is told, with no model time", await ended("boss is not in your contacts", 8), await newest());
  check("still one sent, at the end", sent() === 1, sent());
};
