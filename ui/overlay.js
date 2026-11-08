// The chibi on the desktop and her speech bubble. The window ignores the mouse; everything
// here is driven by events from the backend.
"use strict";

const { listen: listenAll } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

// Which companion this window is: its label says it ("overlay" is Kal'tsit's, "overlay-<id>" the
// others'). Every event says whose it is; this window only listens to hers (and to the ones that
// are nobody's, like the language).
const LABEL = (window.__TAURI__.window && window.__TAURI__.window.getCurrentWindow().label) || "overlay";
const ME = LABEL === "overlay" ? "kaltsit" : LABEL.replace(/^overlay-/, "");
const listen = (ev, f) => listenAll(ev, (e) => { if (!e.payload || !e.payload.who || e.payload.who === ME) f(e); });

// The language of the app comes with its state; the chat can change it at any time.
onLang(() => { document.title = t("overlay.title"); });
invoke("state").then((s) => setLang(s.idioma)).catch(() => {});
listen("k:state", (e) => { if (e.payload && e.payload.idioma) setLang(e.payload.idioma); });

// Her body: the one chosen for her (Modelos → Cuerpo): her Spine chibi, downloaded on demand,
// her pixel art (personajes/<id>/) or her 3D model (a VRM on this computer). Changing it reloads
// this window with the other.
const NAMES = { kaltsit: "Kal'tsit", lappland: "Lappland" };
document.querySelector("#bubble .who").textContent = NAMES[ME] || ME;
document.title = NAMES[ME] || ME;
const made = invoke("body_of", { who: ME })
  .catch(() => ({ cuerpo: ME === "kaltsit" ? "spine" : "pixel", spine: "003_kalts" }))
  .then((b) => {
    const c = document.getElementById("chibi");
    const pose = (body) => { if (b.pose && b.pose !== "Relax") body.ready.then(() => body.play(b.pose)); return body; };
    // Her 3D body (VRM): a module of its own, loaded only when it is the one chosen.
    if (b.cuerpo === "vrm") {
      c.classList.add("vrm");
      document.getElementById("bubble").style.bottom = "290px";
      return import("./vrm.js").then((m) => {
        const body = pose(new m.VrmBody(c, ME));
        invoke("state", { who: ME }).then((s) => body.feel(s.hormonas)).catch(() => {});
        listen("k:state", (e) => body.feel(e.payload && e.payload.hormonas));
        return body;
      });
    }
    if (b.cuerpo !== "pixel") return pose(new Chibi(c, b.spine, ME));
    // Pixel for pixel: the canvas takes her size; the bubble sits over her head.
    c.style.width = c.style.height = "auto";
    const body = new SpriteBody(c, ME);
    body.ready.then((x) => { document.getElementById("bubble").style.bottom = `${x.h - 20}px`; });
    return pose(body);
  });
listen("k:cuerpo", () => location.reload());
const bubble = document.getElementById("bubble");
const textEl = document.getElementById("text");
// The bubble shows the end of what she says; the whole text is in the chat.
const BUBBLE_CHARS = 260;

let current = null;
let acc = "";
let hideTimer = null;

function render(text) {
  textEl.textContent = "";
  // No roleplay actions (an unclosed one while streaming is hidden too) nor an echoed context.
  let shown = text.replace(/^\s*\[[^\]]*(\]|$)/, "").replace(/\*[^*\n]+\*/g, "").replace(/\*[^*\n]*$/, "").replace(/[ \t]{2,}/g, " ").trim();
  let clipped = false;
  if (shown.length > BUBBLE_CHARS) {
    shown = "…" + shown.slice(shown.length - BUBBLE_CHARS);
    clipped = true;
  }
  textEl.append(shown);
  if (clipped) {
    const m = document.createElement("div");
    m.className = "more";
    m.textContent = t("overlay.more");
    textEl.append(m);
  }
}

function show() {
  clearTimeout(hideTimer);
  bubble.classList.add("show");
}

// Long enough to read it: about 14 characters a second, never less than 5 s.
function hideLater(text) {
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => bubble.classList.remove("show"), Math.max(5000, (text.length / 14) * 1000 + 2500));
}

made.then((chibi) => chibi.ready.then(() => {
  listen("k:anim", (e) => chibi.play(e.payload.name, e.payload.loop !== false));
  listen("k:face", (e) => chibi.face(!!e.payload.left));
  listen("k:voz", (e) => chibi.speak(e.payload.level, e.payload.v));
  listen("k:start", (e) => {
    current = e.payload.id;
    acc = "";
    bubble.classList.add("thinking");
    render("");
    show();
  });
  listen("k:tok", (e) => {
    if (e.payload.id !== current) return;
    bubble.classList.remove("thinking");
    acc += e.payload.v;
    render(acc);
  });
  listen("k:said", (e) => {
    if (e.payload.id !== current) return;
    bubble.classList.remove("thinking");
    render(e.payload.text);
    show();
    hideLater(e.payload.text);
  });
  listen("k:error", () => {
    bubble.classList.remove("thinking", "show");
  });
}));
