// v4 C's exit check on the page: nothing leaves without a yes, and every request is in the log. A real
// Gemma draft, a stand-in kestrel-net (experiments/v4/fake_net.py). OUT: where the screenshots go.
// SWAP: a file; when the steps create it, a script outside puts the real kestrel-net in place.
import { writeFileSync, existsSync } from "node:fs";

export default async ({ js, shot, wait, check, log }) => {
  const text = () => js(`document.body.innerText`);
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
  // The page drops a request while another runs, and keeps answered questions: so each check waits for
  // the page to be free, and looks at the newest request and the question still waiting.
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
  const saying = async (words, want, seconds = 20) => {
    await say(words);
    check(`"${words}" → ${want}`, await ended(want, seconds), await newest());
  };
  const logRows = () => js(`[...document.querySelectorAll("#out-rows .out-row")].map((r) => r.innerText)`);
  const logHas = async (n) => {
    await view("settings");
    await until(async () => (await logRows()).length === n, 10);
    return logRows();
  };

  await view("settings");
  await until(() => js(`document.getElementById("out-state").textContent === "Off"`), 8);
  check("the internet is off at first", await js(`!document.getElementById("out-internet").checked`), null);
  check("nothing has left", await js(`!document.getElementById("out-empty").hidden`), null);
  check("the mail card names its two steps", await until(async () => (await text()).includes("add your Gmail address and app password"), 8), null);

  await view("home");
  await saying("add contact Ravi ravi@example.com", "Ravi's address is ravi@example.com");
  await saying("send it to Ravi", "no draft ready");

  log("asking Gemma for a draft (the first request loads it)");
  await say("write a short mail to Ravi saying I am on leave on Friday");
  await until(async () => (await waiting()) !== "" || (await free()), 300);
  if ((await waiting()) !== "") await answer("Yes");
  check("Gemma wrote a draft", await ended("Subject:", 300), await newest());
  await shot(`${process.env.OUT}/draft.jpg`);

  await saying("send it to Ravi", "internet is off");
  await view("settings");
  await js(`document.getElementById("out-internet").click(); true`);
  check("the switch turns the internet on", await until(() => js(`document.getElementById("out-state").textContent === "On"`), 8), null);
  await view("home");
  await saying("send it to Ravi", "Mail is not set up");

  await view("settings");
  const account = (from, password) => js(`(() => { document.getElementById("mail-from").value = ${JSON.stringify(from)}; document.getElementById("mail-password").value = ${JSON.stringify(password)}; document.getElementById("mail-form").requestSubmit(); return true; })()`);
  await account("not an address", "page canary 1234");
  check("a bad address is refused", await until(() => js(`document.getElementById("out-line").textContent.includes("not one mail address")`), 8), null);
  check("the password field is emptied at once", (await js(`document.getElementById("mail-password").value`)) === "", null);
  await account("me@example.com", "page canary 1234");
  check("the account is shown", await until(() => js(`document.getElementById("mail-state").textContent.includes("me@example.com")`), 8), null);
  check("the refusal is gone after a success", await js(`!document.getElementById("out-line").textContent.includes("not one mail address")`), null);
  check("its password is not", !(await text()).includes("page canary") && (await js(`document.documentElement.outerHTML.includes("page canary")`)) === false, null);
  check("the mail card is ready", await until(async () => { await js(`refresh(); true`); await wait(600); return !(await text()).includes("add your Gmail address and app password"); }, 12), null);
  await view("home");

  await say("send it to Ravi");
  check("a yes is asked", await until(async () => (await waiting()).startsWith("Send this mail?"), 15), await newest());
  const shown = await waiting();
  log(shown);
  for (const part of ["From: me@example.com", "To: ravi@example.com", "Subject:"]) check(`the question shows ${part}`, shown.includes(part), shown);
  check("the person's name filled the blank for it", !shown.includes("[Your Name]"), shown);
  await js(`document.querySelector(".question").scrollIntoView({ block: "center" }); true`);
  await wait(200);
  await shot(`${process.env.OUT}/built-mail-question.jpg`);
  await answer("No");
  check("No: Not sent", await ended("Not sent.", 8), await newest());
  let rows = await logHas(1);
  check("the no is in the log", rows.length === 1 && rows[0].includes("you said no") && rows[0].includes("ravi@example.com"), rows);
  await view("home");

  await say("ravi ko bhej do");
  check("asked again", await until(async () => (await waiting()).startsWith("Send this mail?"), 15), await newest());
  await answer("Yes");
  check("Yes: Sent", await ended("Sent to Ravi (ravi@example.com).", 15), await newest());
  rows = await logHas(2);
  check("the send is in the log, newest first", rows.length === 2 && rows[0].includes("· sent") && rows[1].includes("you said no"), rows);
  check("the body is not in the log", !rows.join(" ").includes("Thanks"), rows);
  await js(`document.getElementById("out-h").scrollIntoView({ block: "start" }); true`);
  await wait(200);
  await shot(`${process.env.OUT}/built-internet-mail.jpg`);
  await view("home");

  await saying("send it to Asha", "not in your contacts");

  // The real kestrel-net from here: its own token check, then no password in the keyring, so no mail.
  writeFileSync(process.env.SWAP, "now\n");
  await until(async () => existsSync(`${process.env.SWAP}.done`), 20);
  await say("send it to Ravi");
  check("asked for the real one too", await until(async () => (await waiting()).startsWith("Send this mail?"), 15), await newest());
  await answer("Yes");
  check("the real kestrel-net took the token and found no password", await ended("no mail password in the keyring", 25), await newest());
  rows = await logHas(3);
  check("the failure is in the log", rows.length === 3 && rows[0].includes("no mail password"), rows);
  await js(`document.getElementById("out-internet").click(); true`);
  await until(() => js(`document.getElementById("out-state").textContent === "Off"`), 8);
  await view("home");
  await saying("bhej do", "internet is off");
  rows = await logHas(3);
  check("off again: nothing asked, nothing new in the log", rows.length === 3, rows);
};
