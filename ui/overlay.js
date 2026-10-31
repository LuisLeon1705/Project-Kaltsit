// The chibi on the desktop and her speech bubble. The window ignores the mouse; everything
// here is driven by events from the backend.
"use strict";

const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

// The language of the app comes with its state; the chat can change it at any time.
onLang(() => { document.title = t("overlay.title"); });
invoke("state").then((s) => setLang(s.idioma)).catch(() => {});
listen("k:state", (e) => { if (e.payload && e.payload.idioma) setLang(e.payload.idioma); });

const chibi = new Chibi(document.getElementById("chibi"));
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

chibi.ready.then(() => {
  listen("k:anim", (e) => chibi.play(e.payload.name, e.payload.loop !== false));
  listen("k:face", (e) => chibi.face(!!e.payload.left));
  listen("k:voz", (e) => chibi.speak(e.payload.level));
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
});
