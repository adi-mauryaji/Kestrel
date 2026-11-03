// What Home promises follows the internet switch; off, it is the human's design word for word.
export default async ({ js, shot, wait, check }) => {
  const lines = () => js(`[document.getElementById("promise-line").textContent, document.getElementById("local-line").textContent]`);
  await wait(800);
  let said = await lines();
  check("off: the human's words", said[0] === "Runs on this laptop. Nothing you type leaves it." && said[1] === "Your data stays on your device.", said);
  await shot(`${process.env.OUT}/home-off.jpg`);
  await js(`api("/api/outbound", { internet: true }).then(() => refresh()).then(() => true)`);
  await wait(800);
  said = await lines();
  check("on: what is true then", said[0] === "Runs on this laptop. Nothing leaves without your yes." && said[1] === "Your data leaves only with your yes.", said);
  await shot(`${process.env.OUT}/home-on.jpg`);
  await js(`location.reload(); true`);
  await wait(2500);
  said = await lines();
  check("a new load knows", said[0].includes("without your yes"), said);
  await js(`api("/api/outbound", { internet: false }).then(() => refresh()).then(() => true)`);
  await wait(800);
  said = await lines();
  check("off again", said[0].includes("Nothing you type leaves it"), said);
};
