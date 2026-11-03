// A request broken on its way (a wifi change does this even to 127.0.0.1) is not "Kestrel is not answering".
export default async ({ js, wait, check }) => {
  const newest = () => js(`document.querySelector(".detail")?.textContent ?? ""`);
  const say = (w) => js(`(() => { document.getElementById("say").value = ${JSON.stringify(w)}; document.getElementById("ask").requestSubmit(); return true; })()`);
  await js(`(() => { window.realFetch = window.fetch; window.fetch = (u, o) => String(u).includes("/api/say") ? Promise.reject(new TypeError("Failed to fetch")) : window.realFetch(u, o); return true; })()`);
  await say("show my tasks");
  await wait(1500);
  const text = await newest();
  check("it says the connection broke and Kestrel runs", text.includes("connection broke") && text.includes("Kestrel is running"), text);
  check("it does not send them to start it", !text.includes("Start it with"), text);
  await js(`(() => { window.fetch = (u, o) => Promise.reject(new TypeError("Failed to fetch")); return true; })()`);
  await say("show my tasks");
  await wait(1500);
  const down = await newest();
  check("with the service truly gone it says that", down.includes("Start it with: kestrel open"), down);
  await js(`(() => { window.fetch = window.realFetch; return true; })()`);
};
