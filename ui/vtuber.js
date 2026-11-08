// VTuber mode, like riko_project's: one companion full screen, from the waist up, on a plain
// background (green for a chroma key in OBS, or another), her mouth and face as on the desktop,
// her eyes following the pointer, what she says as subtitles. Esc closes it; B changes the
// background (or none: transparent); S hides the subtitles; F toggles full screen; M moves it
// to the next screen.
import { VrmBody } from "./vrm.js";

const { invoke } = window.__TAURI__.core;
const { listen: listenAll } = window.__TAURI__.event;
const win = window.__TAURI__.window.getCurrentWindow();
const WHO = new URLSearchParams(location.search).get("who") || "kaltsit";
const listen = (ev, f) => listenAll(ev, (e) => { if (!e.payload || !e.payload.who || e.payload.who === WHO) f(e); });
const $ = (id) => document.getElementById(id);

// Green (#00b140, the chroma key colour), blue, black, dark grey, white, and none (her over
// whatever is behind: the desktop, a game).
const BACKGROUNDS = ["#00b140", "#0047bb", "#000000", "#1b2124", "#ffffff", "transparent"];
const store = (k, v) => { try { if (v === undefined) return localStorage.getItem(k); localStorage.setItem(k, v); } catch {} return null; };
let bg = Number(store("vtuber.bg") || 0) % BACKGROUNDS.length;
let subsOn = store("vtuber.subs") !== "0";
const paintBg = () => document.documentElement.style.setProperty("--bg", BACKGROUNDS[bg]);
paintBg();

async function main() {
  const s = await invoke("state", { who: WHO }).catch(() => ({}));
  setLang(s.idioma || "es");
  const me = (s.crew || []).find((c) => c.id === WHO);
  const name = me ? me.name : WHO;
  document.title = `${name} · VTuber`;
  document.querySelector("#subs .who").textContent = name;
  const help = $("help");
  help.innerHTML = t("vtuber.help");
  let helpTimer = setTimeout(() => help.classList.add("hide"), 5000);
  const showHelp = () => {
    help.classList.remove("hide");
    clearTimeout(helpTimer);
    helpTimer = setTimeout(() => help.classList.add("hide"), 3000);
  };

  const body = new VrmBody($("stage"), WHO, { bust: true });
  try {
    await body.ready;
  } catch (e) {
    $("err").textContent = String(e);
    return;
  }
  body.feel(s.hormonas);
  if (s.config && s.config.reposo) body.play("Sleep");
  listen("k:state", (e) => body.feel(e.payload && e.payload.hormonas));
  listen("k:anim", (e) => body.play(e.payload.name, e.payload.loop !== false));
  listen("k:voz", (e) => body.speak(e.payload.level, e.payload.v));

  // Her eyes follow the pointer; the keys show when it goes to the top.
  window.addEventListener("pointermove", (e) => {
    body.lookAt((e.clientX / innerWidth) * 2 - 1, 1 - (e.clientY / innerHeight) * 2);
    if (e.clientY < 60) showHelp();
  });

  // Subtitles: the end of what she is saying, as it streams.
  const subs = $("subs");
  const text = $("subText");
  let current = null;
  let acc = "";
  let hide = null;
  const clean = (t) => t.replace(/^\s*\[[^\]]*(\]|$)/, "").replace(/\*[^*\n]+\*/g, "").replace(/\*[^*\n]*$/, "").replace(/\s{2,}/g, " ").trim();
  const show = (t) => {
    if (!subsOn) return;
    let shown = clean(t);
    if (shown.length > 180) shown = "…" + shown.slice(-180);
    text.textContent = shown;
    subs.classList.toggle("show", !!shown);
    clearTimeout(hide);
  };
  listen("k:start", (e) => { current = e.payload.id; acc = ""; });
  listen("k:tok", (e) => { if (e.payload.id === current) show((acc += e.payload.v)); });
  listen("k:said", (e) => {
    if (e.payload.id !== current) return;
    show(e.payload.text);
    hide = setTimeout(() => subs.classList.remove("show"), Math.max(5000, (e.payload.text.length / 14) * 1000 + 2500));
  });

  window.addEventListener("keydown", async (e) => {
    const k = e.key.toLowerCase();
    if (k === "escape") return win.close();
    if (k === "b") {
      bg = (bg + 1) % BACKGROUNDS.length;
      store("vtuber.bg", String(bg));
      paintBg();
    } else if (k === "s") {
      subsOn = !subsOn;
      store("vtuber.subs", subsOn ? "1" : "0");
      if (!subsOn) subs.classList.remove("show");
    } else if (k === "f") {
      await win.setFullscreen(!(await win.isFullscreen()));
    } else if (k === "m") {
      await invoke("vtuber_screen", {}).catch(() => {});
    } else return;
    showHelp();
  });
}

main().catch((e) => { $("err").textContent = String(e); });
