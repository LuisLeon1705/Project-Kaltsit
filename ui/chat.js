// The dedicated app: the whole conversation, plus what she saw and her switches.
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const state = { user: "", live: new Map(), busy: false, status: "", last: null };

// Name, abbreviation and meaning come from the dictionary (hormone.<key>[.abbr|.what]).
const HORMONES = [
  ["dopamina", "#e3b45a"],
  ["cortisol", "#e0605a"],
  ["serotonina", "#7ab3f0"],
  ["oxitocina", "#e07ab8"],
  ["adrenalina", "#f08a4b"],
];

// She is an assistant, not a roleplay character: actions between asterisks are not shown (old
// messages may still have them; while streaming, an action not closed yet is hidden too), nor a
// copy of her bracketed context that a small model may echo at the start.
function noActions(text) {
  return noActionBlocks(text)
    .replace(/^\s*\[[^\]]*(\]|$)/, "")
    .replace(/\*[^*\n]+\*/g, "")
    .replace(/\*[^*\n]*$/, "")
    .replace(/[ \t]{2,}/g, " ")
    .replace(/ ([.,;:!?…])/g, "$1")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

// Her action blocks ({"action": …}) are for the game, never for the chat: the backend sends the
// finished reply without them, and while it streams a block (even one not closed yet) is hidden here.
function noActionBlocks(text) {
  let out = text.replace(/```(?:json)?\s*([\s\S]*?)```/g, (m, inner) => (/"action"/.test(inner) ? "" : m));
  for (;;) {
    const i = out.search(/\{\s*"action"/);
    if (i < 0) break;
    let depth = 0;
    let inStr = false;
    let esc = false;
    let j = i;
    for (; j < out.length; j++) {
      const ch = out[j];
      if (inStr) {
        if (esc) esc = false;
        else if (ch === "\\") esc = true;
        else if (ch === '"') inStr = false;
      } else if (ch === '"') inStr = true;
      else if (ch === "{") depth++;
      else if (ch === "}" && --depth === 0) break;
    }
    out = out.slice(0, i) + (j < out.length ? out.slice(j + 1) : "");
  }
  return out.replace(/```(?:json)?\s*$/, "").replace(/\{\s*(?:"a?c?t?i?o?n?"?)?\s*$/, "");
}

function renderBody(el, text) {
  el.textContent = noActions(text);
}

function add(role, text, opts = {}) {
  const m = document.createElement("div");
  m.className = `msg ${role}${opts.alone ? " alone" : ""}`;
  const who = document.createElement("div");
  who.className = "who";
  who.textContent = (role === "user" || role === "ghost") ? state.user : role === "assistant" ? "Kal'tsit" : tOr(`who.${role}`, role);
  const body = document.createElement("div");
  body.className = "body";
  renderBody(body, text);
  m.append(who, body);
  $("log").append(m);
  scroll();
  return { el: m, body };
}

function scroll() {
  const l = $("log");
  l.scrollTop = l.scrollHeight;
}

// What she learned from a message: one chip per change in her memory, the kind as an icon and the
// whole memory on hover. The words are in the dictionary (learned.<action>).
const LEARNED = {
  nuevo: "＋", "nueva causa": "＋", causa: "↳", refuerza: "↑", consolidado: "◆", "se cumple": "✓",
  debilita: "↓", rechazado: "✕", sustituido: "⇄",
};
function learnedChips(msg, events) {
  if (!events.length) return;
  const box = el("div", "chips");
  box.append(el("span", "chips-label", t("learned.label")));
  for (const x of events) {
    const icon = LEARNED[x.action] || "·";
    const what = tOr(`learned.${x.action}`, x.action);
    const c = el("span", `chip learned ${x.action === "rechazado" || x.action === "debilita" ? "neg" : ""}`);
    c.append(el("b", "", icon), document.createTextNode(x.label.length > 54 ? `${x.label.slice(0, 53)}…` : x.label));
    c.title = `${what}: ${x.label}`;
    box.append(c);
  }
  msg.el.append(box);
}

// First launch: the overlay that asks for the primary user's name and the language she thinks in.
// Its texts are in the dictionary (fr.*); the whole app follows the selected language while it
// shows, and it starts from the system language.
const firstRun = { shown: false, busy: false, models: null };

function paintFirstRunRecommend() {
  const box = $("frRecommend");
  if (!box) return;
  box.textContent = "";
  const d = firstRun.models;
  if (!d || !d.recomendacion || !d.equipo) return;
  const e = d.equipo;
  const r = d.recomendacion;
  box.append(el("div", "fr-label", t("fr.hw.label")));
  const card = el("div", "net-card fr-rec-card");
  card.append(
    el(
      "div",
      "hint",
      e.cpu_only
        ? t("fr.hw.cpu", { hw: e.nombre, gb: e.presupuesto.toFixed(1) })
        : t("fr.hw.gpu", { hw: e.nombre, gb: e.presupuesto.toFixed(1) }),
    ),
  );
  const head = el("div", "net-row");
  head.append(
    el("b", "grow", catText("name", r.id, r.nombre)),
    el("span", `tag fit-${r.cabe.replace(" ", "")}`, `~${r.gb.toFixed(1)} GB · ${tOr(`fit.${r.cabe}`, r.cabe)}`),
    el("span", "tag brain", t("model.recommended")),
  );
  card.append(head);
  card.append(el("div", r.lento ? "hint warn" : "hint", t(`fr.reason.${r.motivo}`, { gb: r.gb.toFixed(1) })));
  const ids = r.ids || [r.id];
  const activeId = ids.find((id) => mods.progress.has(id));
  const p = activeId ? mods.progress.get(activeId) : null;
  const row = el("div", "net-row");
  if (p) {
    row.append(progressBar(p));
  } else if (r.instalado) {
    row.append(el("span", "grow hint", t("fr.rec.ready")));
  } else {
    row.append(
      el("span", "grow hint", t("fr.rec.downloadSize", { size: GB(r.descarga), engine: r.incluye_motor ? t("fr.rec.withEngine") : "" })),
    );
    const get = el("button", "ghost", t("model.get"));
    get.type = "button";
    get.onclick = () => {
      startDownload(ids);
      paintFirstRunRecommend();
    };
    row.append(get);
  }
  card.append(row);
  if (!r.instalado && !p) card.append(el("div", "hint", t("fr.rec.later")));
  box.append(card);
}

function paintFirstRun() {
  setLang($("frLang").value);
  repaintLanguage();
  paintFirstRunRecommend();
  if (!$("frError").classList.contains("hidden")) $("frError").textContent = t("fr.error");
}

// Everything behind the overlay is inert: no clicks, no focus, no typing into the chat.
function setFirstRunBlocking(on) {
  for (const el of document.body.children) {
    if (el.id !== "firstRun" && el.tagName !== "SCRIPT") el.inert = on;
  }
}

function showFirstRun() {
  if (firstRun.shown) return;
  firstRun.shown = true;
  const system = (navigator.language || "en").toLowerCase();
  $("frLang").value = system.startsWith("es") ? "es" : "en";
  $("frError").classList.add("hidden");
  paintFirstRun();
  setFirstRunBlocking(true);
  $("firstRun").classList.remove("hidden");
  $("frName").focus();
  invoke("models_state", { profile: null })
    .then((d) => {
      firstRun.models = d;
      if (firstRun.shown) paintFirstRunRecommend();
    })
    .catch(() => {});
}

function hideFirstRun() {
  if (!firstRun.shown) return;
  firstRun.shown = false;
  $("firstRun").classList.add("hidden");
  setFirstRunBlocking(false);
}

$("frLang").onchange = paintFirstRun;
$("frForm").onsubmit = async (ev) => {
  ev.preventDefault();
  if (firstRun.busy) return;
  const name = $("frName").value.trim();
  const err = $("frError");
  if (!name || [...name].length > 40) {
    err.textContent = t("fr.error");
    err.classList.remove("hidden");
    return;
  }
  err.classList.add("hidden");
  firstRun.busy = true;
  try {
    const rec = firstRun.models && firstRun.models.recomendacion;
    if (rec && rec.instalado && !rec.en_uso) {
      await invoke("model_use", { id: rec.id, profile: null }).catch(() => {});
    }
    const s = await invoke("first_run", { name, language: $("frLang").value });
    hideFirstRun();
    // The memory was rebuilt: show her again from the start, with her new greeting.
    $("log").textContent = "";
    await boot();
    applyState(s);
  } catch (e) {
    err.textContent = String(e);
    err.classList.remove("hidden");
  } finally {
    firstRun.busy = false;
  }
};

// Applies the active language to everything on screen, without a reload: the static texts by
// their data-i18n attributes, the header from the last state, and the views that are built by
// code from the data they already hold.
function repaintLanguage() {
  applyI18n(document);
  $("langSel").value = getLang();
  $("status").textContent = state.status || t("status.idle");
  if (state.last) applyState(state.last);
  paintMcState();
  if (mem.model) {
    renderMemory();
    if (mem.selected != null) renderMemoryDetail(mem.selected);
  }
  if (moral.data) moralIndicators(moral.data);
  if (net.data) renderNet();
  if (mods.data) renderModels();
  if (plan.data) renderPlan();
  if (acc.data) renderAccessories();
  if (!$("link").classList.contains("hidden") && net.data) showLink(net.data);
  if (!$("ask").classList.contains("hidden") && net.data && net.data.propuesta) showAsk(net.data.propuesta);
}

function applyState(s) {
  state.last = s;
  // The language changed somewhere else (another window or computer): follow it.
  if (!s.primer_arranque && s.idioma && s.idioma !== getLang()) {
    setLang(s.idioma);
    return repaintLanguage();
  }
  if (s.primer_arranque) showFirstRun();
  else hideFirstRun();
  state.user = s.user;
  const reposoEl = $("reposo");
  if (reposoEl) reposoEl.checked = !!s.config.reposo;
  $("vision").checked = s.config.vision;
  $("wander").checked = s.config.wander;
  $("hideChat").checked = !s.chat_en_capturas;
  $("voiceOn").checked = !!(s.voz && s.voz.activa);
  const inReposo = !!s.config.reposo;
  const inMc = !!s.config.minecraft;
  const mcEl = $("minecraft");
  if (mcEl) mcEl.checked = inMc;
  $("mcState").classList.toggle("hidden", !inMc || !mcState.text);
  $("vision").disabled = inReposo || inMc;
  $("wander").disabled = inReposo;
  $("look").disabled = inReposo || inMc;
  document.body.classList.toggle("in-reposo", inReposo);
  const llm = $("llm");
  if (inReposo) {
    llm.textContent = t("llm.rest");
    llm.className = "llm";
    llm.title = t("llm.rest.title");
  } else {
    llm.textContent = s.online ? "● " + s.llm.split(" · ")[0] : t("llm.offline");
    llm.className = `llm ${s.online ? "on" : "off"}`;
    llm.title = s.llm;
  }
  const box = $("hormones");
  box.textContent = "";
  for (const [k, c] of HORMONES) {
    const label = t(`hormone.${k}.abbr`);
    const v = s.hormonas[k] ?? 0;
    const d = document.createElement("div");
    d.className = "meter";
    d.title = `${t(`hormone.${k}`)} ${v.toFixed(2)} · ${t(`hormone.${k}.what`)}`;
    const b = document.createElement("div");
    b.className = "b";
    const i = document.createElement("i");
    i.style.width = `${Math.round(v * 100)}%`;
    i.style.setProperty("--c", c);
    b.append(i);
    d.append(label, b);
    box.append(d);
  }
  if (s.moral) moralIndicators(s.moral);
}

// The language comes from the state before anything is drawn, so nobody sees the wrong one first;
// while the first-run wizard shows, the wizard's selector decides (see showFirstRun).
async function boot() {
  const s = await invoke("state");
  if (!s.primer_arranque) {
    setLang(s.idioma);
    repaintLanguage();
  }
  const h = await invoke("history");
  state.user = h.user;
  add("assistant", h.opening);
  for (const m of h.messages) add(m.role, m.text);
  applyState(await invoke("state"));
}

// The language selector: English / Español, next to the switches.
const langHintKey = "kaltsit.langHintShown";
$("langSel").onchange = async (e) => {
  const code = e.target.value;
  try {
    const s = await invoke("set_config", { language: code });
    setLang(s.idioma);
    state.last = s;
    repaintLanguage();
    let shown = false;
    try { shown = localStorage.getItem(langHintKey) === "1"; } catch {}
    if (!shown) {
      $("langHint").classList.remove("hidden");
      setTimeout(() => $("langHint").classList.add("hidden"), 8000); // a floating note: it goes on its own
      try { localStorage.setItem(langHintKey, "1"); } catch {}
    }
  } catch (err) {
    $("langSel").value = getLang();
    add("error", String(err));
  }
};

listen("k:status", (e) => {
  state.status = e.payload.text || "";
  $("status").textContent = state.status || t("status.idle");
});
listen("k:log", (e) => {
  // Her own messages arrive through k:start / k:said; this echoes the rest.
  const m = add(e.payload.role, e.payload.text);
  if (e.payload.voz) voiceChips(m, e.payload.voz, !!e.payload.sigue);
});

// A message said out loud: one chip per voice in it. An unknown voice can be named here, and
// from then on it is recognised by that name.
function voiceChips(msg, turns, still) {
  // `still`: a piece of a monologue that was still going on when it was sent.
  const box = el("div", `chips${still ? " sigue" : ""}`);
  box.append(el("span", "chips-label", t("heard.label")));
  const seen = new Set();
  for (const x of turns) {
    if (seen.has(x.voz)) continue;
    seen.add(x.voz);
    // The clip of a voice she does not know: ▶ plays it, to hear whose it is.
    const clip = turns.find((y) => y.voz === x.voz && y.clip)?.clip;
    if (clip) {
      const play = el("span", "chip play", "▶");
      play.title = t("heard.play");
      play.onclick = async () => {
        try { new Audio(await invoke("voice_clip", { clip })).play(); } catch (err) { add("error", String(err)); }
      };
      box.append(play);
    }
    const c = el("span", `chip voice${x.conocida ? "" : " unknown"}`);
    // Below 0.65 she is not sure: "Ana?" (probably Ana).
    const probable = x.voz && x.conocida && (x.seguridad || 0) < 0.65;
    c.textContent = x.voz ? `${x.nombre}${probable ? "?" : ""}` : t("heard.someone");
    if (x.voz) c.title = t("heard.sure", { p: Math.round((x.seguridad || 0) * 100) });
    // Named here: an unknown voice by its id, a take she could not tell ("someone") by its clip.
    const nameIt = x.voz && !x.conocida ? (name) => invoke("voice_name", { id: x.voz, name })
      : !x.voz && clip ? async (name) => (await invoke("voice_name_clip", { clip, name })).nombre : null;
    if (nameIt) {
      c.classList.add("unknown");
      c.title = t("heard.whoTitle");
      // Named in place: the chip becomes a small box (no dialogs in the webview).
      c.onclick = () => {
        const input = el("input", "chip-name");
        input.placeholder = t("heard.whoAsk");
        c.replaceWith(input);
        input.focus();
        let closed = false;
        const done = async (save) => {
          if (closed) return;
          closed = true;
          const name = input.value.trim();
          if (save && name) {
            try {
              const now = await nameIt(name);
              c.textContent = now || name;
              c.classList.remove("unknown");
              c.onclick = null;
              c.title = "";
            } catch (err) { add("error", String(err)); }
          }
          input.replaceWith(c);
        };
        input.onkeydown = (e) => { if (e.key === "Enter") done(true); else if (e.key === "Escape") done(false); };
        input.onblur = () => done(true);
      };
    }
    box.append(c);
  }
  msg.el.append(box);
}
listen("k:start", (e) => {
  const m = add("assistant", "…", { alone: !!e.payload.alone });
  state.live.set(e.payload.id, { msg: m, acc: "" });
});
listen("k:tok", (e) => {
  const l = state.live.get(e.payload.id);
  if (!l) return;
  l.acc += e.payload.v;
  renderBody(l.msg.body, l.acc);
  scroll();
});
listen("k:said", (e) => {
  const l = state.live.get(e.payload.id);
  if (!l) return;
  renderBody(l.msg.body, e.payload.text);
  state.live.delete(e.payload.id);
  scroll();
});
listen("k:learned", (e) => {
  const last = [...document.querySelectorAll(".msg.assistant")].pop();
  if (last) learnedChips({ el: last }, e.payload.events);
});
listen("k:error", (e) => add("error", e.payload.text));
listen("k:state", (e) => applyState(e.payload));

// What her body does in Minecraft (the mod reports every change of state).
// The words are in the dictionary (mc.<STATE>).
const mcState = { text: "", raw: null };
function paintMcState() {
  const p = mcState.raw;
  if (!p) return;
  mcState.text = `⛏ ${tOr(`mc.${p.state}`, String(p.state || "").toLowerCase())}`;
  const chip = $("mcState");
  chip.textContent = mcState.text;
  chip.title = p.reason ? t("mc.reason", { reason: p.reason }) : t("title.mc");
}
listen("k:minecraft_state", (e) => {
  const p = e.payload || {};
  mcState.raw = p;
  paintMcState();
  const chip = $("mcState");
  chip.classList.toggle("bad", p.state === "COMBAT" || p.state === "ALERT");
  chip.classList.toggle("hidden", !$("minecraft").checked);
});

// ── tabs: one view at a time; each loads what it shows when it opens ──
const views = { current: "chat", loaders: {} };
function showView(name) {
  views.current = name;
  for (const b of document.querySelectorAll("#tabs button")) {
    const on = b.dataset.view === name;
    b.classList.toggle("on", on);
    b.setAttribute("aria-selected", on ? "true" : "false");
  }
  for (const v of document.querySelectorAll(".view")) v.classList.toggle("hidden", v.dataset.view !== name);
  if (name === "chat") { scroll(); $("input").focus(); }
  const load = views.loaders[name];
  if (load) Promise.resolve(load()).catch((e) => add("error", String(e)));
}
const isOpen = (name) => views.current === name;
for (const b of document.querySelectorAll("#tabs button")) b.onclick = () => showView(b.dataset.view);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && views.current !== "chat" && !document.querySelector(".ask:not(.hidden)")) showView("chat");
});

let typeInflight = false;
let typePending = null;

function pushInput() {
  const text = $("input").value;
  if (typeInflight) {
    typePending = text;
    return;
  }
  typeInflight = true;
  typePending = null;
  invoke("type_chat", { text })
    .catch(() => {})
    .finally(() => {
      typeInflight = false;
      if (typePending !== null) {
        pushInput();
      }
    });
}

$("input").addEventListener("input", pushInput);

$("composer").addEventListener("submit", async (e) => {
  e.preventDefault();
  const t = $("input").value.trim();
  if (!t) return;
  $("input").value = "";
  typePending = null;
  // Said out loud if what was dictated is still in the box (it may have been touched up).
  const voz = mic.said && t.includes(mic.said.text.slice(0, 24)) ? mic.said.turns : null;
  mic.said = null;
  try { await invoke("send", { text: t, voz }); } catch (err) { add("error", String(err)); }
});
// Dictation, push-to-talk: the microphone is open only while the button (or Ctrl+Space, inside
// this window) is held. What was said goes into the box, after what is already there, for the
// user to review: it is never sent by itself.
// `said`: the last dictation's turns (who spoke, what) and the text it put in the box.
const mic = { down: false, starting: null, busy: false, said: null };
async function micStart() {
  if (mic.down || mic.busy || $("openmic").checked) return;
  mic.down = true;
  $("mic").classList.add("rec");
  $("input").placeholder = t("mic.listening");
  mic.starting = invoke("mic_start");
  try { await mic.starting; } catch (err) { mic.down = false; add("error", String(err)); micIdle(); }
}
function micIdle() {
  $("mic").classList.remove("rec", "busy");
  $("input").placeholder = t("composer.placeholder");
}
async function micStop() {
  if (!mic.down) return;
  mic.down = false;
  $("mic").classList.remove("rec");
  try { await mic.starting; } catch { return; }
  mic.busy = true;
  $("mic").classList.add("busy");
  $("input").placeholder = t("mic.thinking");
  try {
    const turns = (await invoke("mic_stop")) || [];
    const said = turns.map((x) => x.texto).join(" ").trim();
    if (!said) add("error", t("mic.nothing"));
    else {
      const box = $("input");
      box.value = box.value.trim() ? `${box.value.trimEnd()} ${said}` : said;
      mic.said = { turns, text: said };
      pushInput();
    }
  } catch (err) { add("error", String(err)); }
  mic.busy = false;
  micIdle();
  $("input").focus();
}
// "Auto", the open microphone: she listens all the time and each thing said is sent by itself
// when the speaker pauses. Push-to-talk rests while it is on (one microphone at a time).
function paintOpenMic(on, hearing) {
  $("openmic").checked = on;
  $("openmic").closest(".auto").classList.toggle("hearing", on && hearing);
  $("mic").disabled = on;
  if (!mic.down && !mic.busy) $("input").placeholder = on ? t(hearing ? "openmic.hearing" : "openmic.on") : t("composer.placeholder");
}
$("openmic").addEventListener("change", async (e) => {
  const on = e.target.checked;
  try {
    const r = await invoke("open_mic", { on });
    paintOpenMic(!!r.on, false);
  } catch (err) {
    paintOpenMic(false, false);
    add("error", String(err));
  }
});
listen("k:openmic", (e) => paintOpenMic(!!e.payload.on, !!e.payload.oyendo));
$("mic").addEventListener("pointerdown", (e) => { e.preventDefault(); $("mic").setPointerCapture(e.pointerId); micStart(); });
$("mic").addEventListener("pointerup", micStop);
$("mic").addEventListener("pointercancel", micStop);
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.code === "Space" && views.current === "chat") { e.preventDefault(); if (!e.repeat) micStart(); }
});
document.addEventListener("keyup", (e) => { if (e.code === "Space" || e.key === "Control") micStop(); });
window.addEventListener("blur", micStop);

$("input").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    $("composer").requestSubmit();
  }
});
const reposoEl = $("reposo");
if (reposoEl) {
  reposoEl.onchange = async (e) => applyState(await invoke("set_config", { reposo: e.target.checked }));
}
const mcEl = $("minecraft");
if (mcEl) {
  mcEl.onchange = async (e) => applyState(await invoke("set_config", { minecraft: e.target.checked }));
}
$("vision").onchange = async (e) => applyState(await invoke("set_config", { vision: e.target.checked }));
$("wander").onchange = async (e) => applyState(await invoke("set_config", { wander: e.target.checked }));
$("voiceOn").onchange = async (e) => {
  try { await invoke("set_voice", { active: e.target.checked }); } catch (err) { add("error", String(err)); }
  applyState(await invoke("state"));
};
$("hideChat").onchange = async (e) => applyState(await invoke("set_chat_capture", { visible: !e.target.checked }));
$("look").onclick = () => invoke("look_now").catch((e) => add("error", String(e)));

// ── memory panel: single sector graph (§10) ──
// Matched against the sector names of the seed, which are per language (Spanish and English).
const MACRO_PALETTE = [
  ["real", "#58b4e8"],
  ["ficticio", "#c084fc"],
  ["fictional", "#c084fc"],
  ["abstracto", "#f0b44c"],
  ["abstract", "#f0b44c"],
  ["complejo", "#f0b44c"],
  ["emocional", "#eb6f92"],
  ["emotional", "#eb6f92"],
  ["mi mundo", "#4fd1a5"],
  ["my world", "#4fd1a5"],
];
const FALLBACK_MACRO_COLORS = ["#58b4e8", "#c084fc", "#f0b44c", "#eb6f92", "#4fd1a5", "#7fd18b"];
// The verbs of a relation, [from this node, towards this node], come from the dictionary
// (edge.<kind>.out / .in). Unknown kinds show their own name.
const EDGE_KINDS = ["inspirada_en", "causa", "precede", "relacion", "contradice", "menciona", "sustituye"];
const edgeLabels = (kind) => (EDGE_KINDS.includes(kind) ? [t(`edge.${kind}.out`), t(`edge.${kind}.in`)] : [kind, kind]);

const mem = {
  raw: null,
  model: null,
  focus: null, // the sector at the centre (null: the Núcleo)
  layout: null,
  selected: null,
  selectedRefSector: null,
  search: "",
  matches: [],
  matchIdx: 0,
  cam: { x: 0, y: 0, k: 1 },
  items: [],
  pos: new Map(),
  shown: new Map(), // sectorId -> how many loose memories are drawn (paged by LEAF_PAGE)
};
// A sector with more loose memories than this shows the newest ones and a «+N más» node.
const LEAF_PAGE = 24;
const leafTime = (n) => n.u ?? n.born ?? 0;

function macroColorForLabel(label, idx = 0) {
  const low = (label || "").toLowerCase();
  for (const [key, col] of MACRO_PALETTE) {
    if (low.includes(key)) return col;
  }
  return FALLBACK_MACRO_COLORS[idx % FALLBACK_MACRO_COLORS.length];
}

function hexAlpha(hex, a) {
  if (!hex || !hex.startsWith("#") || hex.length < 7) return `rgba(159,210,74,${a.toFixed(3)})`;
  const v = parseInt(hex.slice(1, 7), 16);
  return `rgba(${(v >> 16) & 255},${(v >> 8) & 255},${v & 255},${Math.max(0, Math.min(1, a)).toFixed(3)})`;
}

function normChi(chi) {
  const c = Number.isFinite(chi) ? chi : 0.5;
  return Math.max(0, Math.min(1, c < 0 ? (c + 1) * 0.5 : c));
}

function buildGraphModel(g) {
  const nodes = (g && g.nodes) ? g.nodes.slice() : [];
  const edges = (g && g.edges) ? g.edges.slice() : [];
  const byId = new Map(nodes.map((n) => [n.id, n]));

  // Outgoing "pertenece" edges: f (child) -> t (parent sector)
  const belongsOut = new Map();
  const relationsByNode = new Map();
  for (const n of nodes) {
    belongsOut.set(n.id, []);
    relationsByNode.set(n.id, []);
  }
  for (const e of edges) {
    if (!byId.has(e.f) || !byId.has(e.t)) continue;
    if (e.k === "pertenece") {
      belongsOut.get(e.f).push({ sectorId: e.t, w: e.w ?? 0.5, edge: e });
    } else {
      relationsByNode.get(e.f).push({ otherId: e.t, kind: e.k, w: e.w ?? 0.5, out: true, edge: e });
      if (e.f !== e.t) {
        relationsByNode.get(e.t).push({ otherId: e.f, kind: e.k, w: e.w ?? 0.5, out: false, edge: e });
      }
    }
  }

  // Sort each node's belongsOut by weight w descending so index 0 is the primary sector.
  const primaryParent = new Map();
  const secondaryParents = new Map();
  for (const [id, list] of belongsOut) {
    if (!list.length) continue;
    list.sort((a, b) => (b.w - a.w) || (a.sectorId - b.sectorId));
    primaryParent.set(id, list[0].sectorId);
    if (list.length > 1) secondaryParents.set(id, list.slice(1));
  }

  let sectors = nodes.filter((n) => n.k === "sector");
  let root = sectors.find((n) => !primaryParent.has(n.id) && /n[úu]cleo|core/i.test(n.l))
    || sectors.find((n) => !primaryParent.has(n.id))
    || sectors[0] || null;

  // Fallback for legacy un-sectored memory so the view never breaks:
  if (!root && nodes.length) {
    root = { id: -1, k: "sector", l: t("mem.legacy.root"), t: t("mem.legacy.rootText"), chi: 0.92, core: false, deg: 0 };
    byId.set(root.id, root);
    nodes.unshift(root);
    const modMap = new Map();
    (g.modules || [{ name: "yo", title: t("visor.mod.yo") }, { name: "personas", title: t("visor.mod.personas") }, { name: "cosas", title: t("visor.mod.cosas") }, { name: "saber", title: t("visor.mod.saber") }])
      .forEach((m, i) => {
        const sid = -(i + 2);
        const sn = { id: sid, k: "sector", l: m.title || m.name, t: t("mem.legacy.module", { name: m.title || m.name }), chi: 0.9, core: false, deg: 0 };
        byId.set(sid, sn);
        nodes.push(sn);
        modMap.set(m.name, sid);
        primaryParent.set(sid, root.id);
        belongsOut.set(sid, [{ sectorId: root.id, w: 1 }]);
      });
    for (const n of nodes) {
      if (n.k === "sector") continue;
      const target = n.core ? root.id : (modMap.get(n.m) ?? root.id);
      primaryParent.set(n.id, target);
      belongsOut.set(n.id, [{ sectorId: target, w: 1 }]);
    }
    sectors = nodes.filter((n) => n.k === "sector");
  }

  const childSectors = new Map();
  const directMemories = new Map();
  const refMemories = new Map();
  for (const s of sectors) {
    childSectors.set(s.id, []);
    directMemories.set(s.id, []);
    refMemories.set(s.id, []);
  }
  for (const n of nodes) {
    if (root && n.id === root.id) continue;
    const p = primaryParent.get(n.id);
    if (p != null && childSectors.has(p)) {
      if (n.k === "sector") childSectors.get(p).push(n);
      else directMemories.get(p).push(n);
    }
    const secs = secondaryParents.get(n.id);
    if (secs) {
      for (const sec of secs) {
        if (refMemories.has(sec.sectorId)) {
          refMemories.get(sec.sectorId).push({ node: n, w: sec.w, sectorId: sec.sectorId });
        }
      }
    }
  }

  // Newest first, so a paged sector opens on what she lived most recently.
  for (const list of directMemories.values()) list.sort((a, b) => leafTime(b) - leafTime(a));

  const macros = root ? (childSectors.get(root.id) || []) : [];
  const macroColor = new Map();
  const macroOf = new Map();
  if (root) {
    macroColor.set(root.id, "#9fd24a");
    macroOf.set(root.id, root);
  }
  macros.forEach((m, i) => {
    const c = macroColorForLabel(m.l, i);
    macroColor.set(m.id, c);
    macroOf.set(m.id, m);
  });

  function resolveMacro(id) {
    if (macroOf.has(id)) return macroOf.get(id);
    const seen = new Set([id]);
    let cur = id;
    while (primaryParent.has(cur)) {
      const p = primaryParent.get(cur);
      if (macroOf.has(p) && (!root || p !== root.id)) {
        macroOf.set(id, macroOf.get(p));
        return macroOf.get(p);
      }
      if (seen.has(p)) break;
      seen.add(p);
      cur = p;
    }
    const fallback = root || null;
    macroOf.set(id, fallback);
    return fallback;
  }
  for (const n of nodes) resolveMacro(n.id);

  // Recursive member count per sector
  const memberCount = new Map();
  const directCount = new Map();
  function countRec(sid, visited = new Set()) {
    if (memberCount.has(sid)) return memberCount.get(sid);
    if (visited.has(sid)) return 0;
    visited.add(sid);
    const subs = childSectors.get(sid) || [];
    const leaves = directMemories.get(sid) || [];
    const refs = refMemories.get(sid) || [];
    directCount.set(sid, subs.length + leaves.length + refs.length);
    let total = leaves.length + refs.length;
    for (const sub of subs) total += countRec(sub.id, visited);
    // If a sector only contains subsectors that are currently empty, still count subsectors so size reflects structure.
    if (total === 0 && subs.length > 0) total = subs.length;
    memberCount.set(sid, total);
    return total;
  }
  for (const s of sectors) countRec(s.id);

  return {
    nodes,
    edges,
    byId,
    root,
    macros,
    sectors,
    belongsOut,
    primaryParent,
    secondaryParents,
    childSectors,
    directMemories,
    refMemories,
    relationsByNode,
    macroColor,
    macroOf,
    memberCount,
    directCount,
  };
}

function nodeColor(m, nodeId) {
  const mac = m.macroOf.get(nodeId);
  if (!mac) return "#9fd24a";
  return m.macroColor.get(mac.id) || "#9fd24a";
}

// ── layout: one sector in focus at the centre, what it holds in a ring around it ──
// Drawing every sector at once is unreadable; here the view shows one level (and a preview of
// the next) and you walk the tree by clicking, like folders.
const REL_COLORS = {
  inspirada_en: "#e3b45a",
  causa: "#eb6f92",
  contradice: "#ff6b6b",
  precede: "#7f9cb0",
  relacion: "#9fd24a",
  menciona: "#8a949a",
  sustituye: "#c084fc",
};
// Members of a child sector drawn as dots around it (a preview of the next level).
const PREVIEW_DOTS = 36;
const ANIM_MS = 380;

function focusOf(m) {
  const f = mem.focus;
  return f != null && m.byId.has(f) && m.byId.get(f).k === "sector" ? f : m.root.id;
}

/// Núcleo → … → id, following the primary sectors.
function pathTo(m, id) {
  const out = [];
  const seen = new Set();
  let cur = id;
  while (cur != null && !seen.has(cur)) {
    seen.add(cur);
    out.unshift(cur);
    cur = m.primaryParent.get(cur);
  }
  return out;
}

function childrenOf(m, sid) {
  const subs = m.childSectors.get(sid) || [];
  const all = m.directMemories.get(sid) || [];
  const leaves = all.slice(0, mem.shown.get(sid) || LEAF_PAGE);
  const refs = m.refMemories.get(sid) || [];
  const out = [
    ...subs.map((n) => ({ node: n, kind: "sector" })),
    ...leaves.map((n) => ({ node: n, kind: "leaf" })),
    ...refs.map((r) => ({ node: r.node, kind: "ref" })),
  ];
  const rest = all.length - leaves.length;
  if (rest > 0) out.push({ node: { id: `more:${sid}`, k: "more", l: t("mem.more", { n: rest }), chi: 0.5 }, kind: "more" });
  return out;
}

function computeLayout(m) {
  const focus = focusOf(m);
  const items = [];
  const byNode = new Map();
  const add = (it) => {
    items.push(it);
    if (it.kind !== "ref" && it.kind !== "more" && !byNode.has(it.node.id)) byNode.set(it.node.id, it);
    return it;
  };
  const center = add({ key: `n:${focus}`, node: m.byId.get(focus), kind: "center", x: 0, y: 0, angle: Math.PI / 2, r: focus === m.root.id ? 28 : 24, sectorId: m.primaryParent.get(focus) });
  const kids = childrenOf(m, focus);
  // A sector takes more of the ring than a single memory: it carries its preview around it.
  const weight = (c) => (c.kind === "sector" ? 2.4 : 1);
  const total = kids.reduce((a, c) => a + weight(c), 0) || 1;
  const R = Math.max(150, Math.min(440, (total * 30) / (2 * Math.PI) + 60));
  const edges = [];
  let acc = 0;
  for (const c of kids) {
    const w = weight(c);
    const a = -Math.PI / 2 + ((acc + w / 2) / total) * Math.PI * 2;
    acc += w;
    const isSector = c.kind === "sector";
    const r = isSector ? Math.min(19, 9 + Math.sqrt(m.memberCount.get(c.node.id) || 0) * 1.25) : c.kind === "more" ? 9 : c.node.core ? 7.5 : 5.5;
    const key = c.kind === "ref" ? `ref:${c.node.id}@${focus}` : c.kind === "more" ? c.node.id : `n:${c.node.id}`;
    const it = add({ key, node: c.node, kind: c.kind, x: Math.cos(a) * R, y: Math.sin(a) * R, angle: a, r, sectorId: focus });
    edges.push({ a: center, b: it, tree: true });
    if (isSector) {
      const sub = [...(m.childSectors.get(c.node.id) || []), ...(m.directMemories.get(c.node.id) || [])].slice(0, PREVIEW_DOTS);
      const pr = r + 7;
      sub.forEach((g, i) => {
        const b = a + Math.PI + ((i + 0.5) / sub.length) * Math.PI * 2;
        add({ key: `dot:${g.id}`, node: g, kind: "dot", x: it.x + Math.cos(b) * pr, y: it.y + Math.sin(b) * pr, angle: b, r: g.k === "sector" ? 2.8 : 1.9, sectorId: c.node.id, parent: it });
      });
    }
  }
  // Her relations among what is in view (belongings are the ring itself): the real edges.
  for (const e of m.edges) {
    if (e.k === "pertenece" || e.f === e.t) continue;
    const a = byNode.get(e.f);
    const b = byNode.get(e.t);
    if (a && b && a.kind !== "dot" && b.kind !== "dot") edges.push({ a, b, kind: e.k, w: e.w });
  }
  return { focus, items, edges, R };
}

function applyTransform() {
  const vp = document.getElementById("memViewport");
  if (!vp) return;
  const svg = $("memSvg");
  const w = svg.clientWidth || 560;
  const h = svg.clientHeight || 420;
  vp.setAttribute("transform", `translate(${(w / 2 + mem.cam.x).toFixed(1)},${(h / 2 + mem.cam.y).toFixed(1)}) scale(${mem.cam.k.toFixed(3)})`);
}

/// Frames the ring and its labels, leaving room for the detail panel when it is open.
function fitGraph() {
  const lay = mem.layout;
  if (!lay) return;
  const svg = $("memSvg");
  const w = svg.clientWidth || 560;
  const h = svg.clientHeight || 420;
  const detailOpen = !$("memDetail").classList.contains("hidden");
  const wide = w > 620;
  const availW = detailOpen && wide ? w - 340 : w;
  const availH = detailOpen && !wide ? h * 0.52 : h;
  // The ring plus its longest label (about 6.4 px a character at this size).
  const longest = lay.items.reduce((a, it) => (it.kind === "center" || it.kind === "dot" ? a : Math.max(a, it.r + 14 + Math.min(it.node.l.length + 2, 40) * 6.4)), 40);
  const span = lay.R + longest + 16;
  const k = Math.max(0.3, Math.min(1.5, Math.min(availW, availH) / (2 * span)));
  mem.cam = { x: detailOpen && wide ? -170 : 0, y: detailOpen && !wide ? -Math.round(h * 0.22) : 0, k };
  applyTransform();
}

/// Puts `sid` at the centre.
function setFocus(sid) {
  if (mem.focus === sid) return;
  mem.focus = sid;
  mem.selected = null;
  mem.selectedRefSector = null;
  $("memDetail").classList.add("hidden");
  renderMemory({ fit: true });
}

/// Shows a node: a sector becomes the centre; a memory is selected inside its sector (paging to it
/// if it lies beyond the first page).
function focusNode(nodeId) {
  const m = mem.model;
  if (!m || !m.byId.has(nodeId)) return;
  const n = m.byId.get(nodeId);
  if (n.k === "sector") {
    setFocus(nodeId);
    return;
  }
  const parent = m.primaryParent.get(nodeId) ?? m.root.id;
  const idx = (m.directMemories.get(parent) || []).indexOf(n);
  if (idx >= (mem.shown.get(parent) || LEAF_PAGE)) mem.shown.set(parent, Math.ceil((idx + 1) / LEAF_PAGE) * LEAF_PAGE);
  const moved = focusOf(m) !== parent || $("memDetail").classList.contains("hidden");
  mem.focus = parent;
  mem.selected = nodeId;
  mem.selectedRefSector = null;
  renderMemoryDetail(nodeId);
  renderMemory({ fit: moved });
}

function renderMemoryDetail(nodeId) {
  const box = $("memDetail");
  const m = mem.model;
  if (nodeId == null || !m || !m.byId.has(nodeId)) {
    box.classList.add("hidden");
    box.textContent = "";
    return;
  }
  const n = m.byId.get(nodeId);
  box.textContent = "";
  box.classList.remove("hidden");

  const top = el("div", "mem-detail-top");
  const badges = el("div", "mem-detail-badges");
  const col = nodeColor(m, n.id);
  const kTag = el("span", "tag", n.k);
  kTag.style.borderColor = hexAlpha(col, 0.65);
  kTag.style.color = col;
  badges.append(kTag);
  const mac = m.macroOf.get(n.id);
  if (mac && mac.id !== n.id) badges.append(el("span", "tag", mac.l));
  if (n.core) badges.append(el("span", "tag body", t("mem.coreAxiom")));
  if (n.pcore) badges.append(el("span", "tag warn", t("mem.personalCore")));
  if (n.self) badges.append(el("span", "tag", t("mem.ownSector")));
  const close = el("button", "ghost small", "×");
  close.type = "button";
  close.title = t("mem.closePanel");
  close.onclick = () => {
    mem.selected = null;
    box.classList.add("hidden");
    renderMemory();
  };
  top.append(badges, close);
  box.append(top);
  box.append(el("h3", "", n.l));

  if (n.k === "sector") {
    const total = m.memberCount.get(n.id) || 0;
    const row = el("div", "net-row");
    row.append(el("span", "grow hint", t(total === 1 ? "mem.branch.one" : "mem.branch.other", { n: total })));
    if (n.id !== focusOf(m)) {
      const open = el("button", "ghost small", t("mem.open"));
      open.type = "button";
      open.onclick = () => setFocus(n.id);
      row.append(open);
    }
    box.append(row);
  }
  if (n.t && n.t !== n.l) box.append(el("div", "mem-d-text", n.t));

  const chiVal = Number.isFinite(n.chi) ? n.chi : 0.5;
  const chiRow = el("div", "mem-d-chi");
  chiRow.append(el("span", "", `χ ${chiVal.toFixed(2)}`));
  const bar = el("div", "pbar");
  const fill = el("i");
  fill.style.width = `${Math.round(normChi(chiVal) * 100)}%`;
  fill.style.background = col;
  bar.append(fill);
  chiRow.append(bar, el("span", "", chiVal >= 0.75 ? t("mem.chi.high") : chiVal >= 0.35 ? t("mem.chi.mid") : t("mem.chi.low")));
  box.append(chiRow);

  const meta = el("div", "mem-d-meta");
  const originParts = [n.model || "—", n.host || "—"];
  if (n.o) originParts.push(n.o);
  meta.append(el("div", "", t("mem.origin", { parts: originParts.join(" · ") })));
  const whenParts = [];
  if (n.fecha) whenParts.push(n.fecha);
  if (n.born) whenParts.push(t("mem.savedOn", { date: new Date(n.born * 1000).toLocaleDateString(locale()) }));
  if (n.rec != null) whenParts.push(t("mem.recalled", { n: n.rec }));
  if (whenParts.length) meta.append(el("div", "", t("mem.time", { parts: whenParts.join(" · ") })));
  box.append(meta);

  const belongs = m.belongsOut.get(n.id) || [];
  if (belongs.length) {
    const sec = el("div", "mem-d-sec");
    sec.append(el("h4", "", t("mem.belongs", { n: belongs.length })));
    belongs.forEach((b, idx) => {
      const target = m.byId.get(b.sectorId);
      if (!target) return;
      const btn = el("button", "mem-rel-btn");
      btn.type = "button";
      btn.append(el("small", "", belongs.length > 1 ? (idx === 0 ? t("mem.main") : t("mem.also")) : t("mem.sector")), el("span", "rel-arrow", "→"), el("span", "grow", pathTo(m, target.id).map((id) => m.byId.get(id).l).join(" › ")));
      btn.onclick = () => setFocus(target.id);
      sec.append(btn);
    });
    box.append(sec);
  }

  const rels = m.relationsByNode.get(n.id) || [];
  if (rels.length) {
    const sec = el("div", "mem-d-sec");
    sec.append(el("h4", "", t("mem.relations", { n: rels.length })));
    for (const r of rels) {
      const other = m.byId.get(r.otherId);
      if (!other) continue;
      const btn = el("button", "mem-rel-btn");
      btn.type = "button";
      const pair = edgeLabels(r.kind);
      const verb = el("small", "", r.out ? pair[0] : pair[1]);
      verb.style.color = REL_COLORS[r.kind] || "";
      btn.append(verb, el("span", "rel-arrow", "→"), el("span", "grow", other.l));
      btn.onclick = () => focusNode(other.id);
      sec.append(btn);
    }
    box.append(sec);
  }
}

/// Lights a node and its edges, and dims the rest. `null` lights nothing.
function highlight(id) {
  const svg = $("memSvg");
  for (const x of svg.querySelectorAll(".lit")) x.classList.remove("lit");
  svg.classList.toggle("focusing", id != null);
  if (id == null) return;
  const ids = new Set([String(id)]);
  for (const p of svg.querySelectorAll(".mem-edge")) {
    if (p.dataset.a === String(id) || p.dataset.b === String(id)) {
      p.classList.add("lit");
      ids.add(p.dataset.a);
      ids.add(p.dataset.b);
    }
  }
  for (const g of svg.querySelectorAll(".mem-node")) {
    if (ids.has(g.dataset.nodeId) || g.dataset.parentId === String(id)) g.classList.add("lit");
  }
}

function renderPath(m, focus) {
  const box = $("memPath");
  box.textContent = "";
  pathTo(m, focus).forEach((id, i, all) => {
    if (i) box.append(el("span", "sep", "›"));
    const b = el("button", i === all.length - 1 ? "here" : "", m.byId.get(id).l);
    b.type = "button";
    b.onclick = () => setFocus(id);
    box.append(b);
  });
}

function renderMemory(opts = {}) {
  const m = mem.model;
  const svg = $("memSvg");
  const empty = $("memEmpty");
  if (!m || !m.root) {
    svg.textContent = "";
    empty.textContent = t("mem.empty");
    empty.classList.remove("hidden");
    return;
  }
  empty.classList.add("hidden");
  const focus = focusOf(m);
  const onPath = new Set(pathTo(m, focus));
  renderPath(m, focus);

  const leg = $("memLegend");
  leg.textContent = "";
  for (const mac of m.macros) {
    const pill = el("button", `mem-pill${onPath.has(mac.id) ? " on" : ""}`);
    pill.type = "button";
    pill.style.setProperty("--c", nodeColor(m, mac.id));
    pill.append(el("i"), `${mac.l} (${m.memberCount.get(mac.id) || 0})`);
    pill.title = t("mem.goto", { name: mac.l });
    pill.onclick = () => setFocus(focus === mac.id ? m.root.id : mac.id);
    leg.append(pill);
  }

  const lay = computeLayout(m);
  mem.layout = lay;
  mem.items = lay.items;
  const hosts = [...new Set(m.nodes.map((n) => n.host).filter(Boolean))];
  const models = [...new Set(m.nodes.map((n) => n.model).filter(Boolean))];
  const note = $("memNote");
  note.textContent = t("mem.note", { nodes: m.nodes.length, view: lay.items.filter((i) => i.kind !== "dot").length, hosts: hosts.join(", ") || "—", models: t(models.length === 1 ? "mem.models.one" : "mem.models.other", { n: models.length }) });
  note.title = models.join("\n");

  const NS = "http://www.w3.org/2000/svg";
  const sEl = (tag, cls) => {
    const e = document.createElementNS(NS, tag);
    if (cls) e.setAttribute("class", cls);
    return e;
  };
  svg.textContent = "";
  svg.classList.remove("focusing");
  const vp = sEl("g");
  vp.id = "memViewport";
  svg.append(vp);

  const orbit = sEl("circle", "mem-orbit");
  orbit.setAttribute("r", lay.R.toFixed(1));
  vp.append(orbit);
  if (focus === m.root.id && m.nodes.some((n) => n.pcore)) {
    const ring = sEl("circle", "pcore-ring-guide");
    ring.setAttribute("r", "40");
    vp.append(ring);
  }

  const q = mem.search.toLowerCase();
  const matchSet = q ? new Set(mem.matches.map((n) => n.id)) : null;

  // Edges first, so the nodes sit on top.
  const edgeEls = [];
  const edgeGroup = sEl("g");
  for (const e of lay.edges) {
    const p = sEl("path", `mem-edge ${e.tree ? "tree" : "rel"}`);
    const col = e.tree ? nodeColor(m, e.b.kind === "more" ? focus : e.b.node.id) : REL_COLORS[e.kind] || "#8a949a";
    p.style.stroke = hexAlpha(col, e.tree ? 0.42 : e.kind === "menciona" ? 0.4 : 0.7);
    if (!e.tree) p.style.strokeWidth = String(1 + Math.min(1.4, (e.w || 0.5) * 1.2));
    p.dataset.a = String(e.a.node.id);
    p.dataset.b = String(e.b.node.id);
    if (!e.tree) {
      const t = sEl("title");
      t.textContent = `${e.a.node.l} — ${edgeLabels(e.kind)[0]} → ${e.b.node.l}`;
      p.append(t);
    }
    edgeGroup.append(p);
    edgeEls.push({ e, p });
  }
  vp.append(edgeGroup);

  const nodeEls = [];
  const nodeGroup = sEl("g");
  for (const it of lay.items) {
    const n = it.node;
    const isSector = n.k === "sector";
    const col = it.kind === "more" ? nodeColor(m, focus) : nodeColor(m, n.id);
    const cNorm = normChi(n.chi);
    const alpha = isSector ? 0.5 + 0.5 * cNorm : 0.32 + 0.68 * cNorm;
    const classes = ["mem-node", `node-${it.kind}`];
    if (isSector) classes.push("node-sector");
    if (it.kind === "center") classes.push("node-root");
    if (n.core) classes.push("node-core");
    if (n.pcore) classes.push("node-pcore");
    if (n.self) classes.push("node-self-sector");
    if (it.kind === "ref") classes.push("node-ref");
    if (mem.selected === n.id && it.kind !== "dot") classes.push("selected");
    if (matchSet && !matchSet.has(n.id) && it.kind !== "center") classes.push("dim");
    const g = sEl("g", classes.join(" "));
    g.dataset.nodeId = String(n.id);
    if (it.parent) g.dataset.parentId = String(it.parent.node.id);

    const hit = sEl("circle", "hit");
    hit.setAttribute("r", String(Math.max(it.kind === "dot" ? 4 : 11, it.r + 5)));
    g.append(hit);
    if (n.core) {
      const ring = sEl("circle", "core-ring");
      ring.setAttribute("r", (it.r + 3.2).toFixed(1));
      g.append(ring);
    }
    if (n.pcore) {
      const ring = sEl("circle", "pcore-ring");
      ring.setAttribute("r", (it.r + 5.2).toFixed(1));
      g.append(ring);
    }
    const body = sEl("circle", "body-circle");
    body.setAttribute("r", it.r.toFixed(1));
    if (it.kind === "more" || it.kind === "ref") {
      body.style.fill = "none";
      body.style.stroke = hexAlpha(col, 0.85);
      body.style.strokeDasharray = it.kind === "more" ? "3 2" : "";
    } else {
      body.style.fill = hexAlpha(col, isSector ? alpha * 0.3 : alpha);
      body.style.stroke = hexAlpha(col, Math.min(1, alpha + 0.25));
      body.style.strokeWidth = isSector ? "2px" : it.kind === "dot" ? "0.6px" : "1.3px";
    }
    g.append(body);

    if (isSector && it.kind !== "dot") {
      const cnt = sEl("text", "node-sub");
      cnt.setAttribute("text-anchor", "middle");
      cnt.setAttribute("dy", "3.5");
      cnt.textContent = String(m.memberCount.get(n.id) || 0);
      g.append(cnt);
    }
    if (it.kind === "dot") {
      const t = sEl("title");
      t.textContent = n.l;
      g.append(t);
    } else {
      const label = sEl("text", "node-label");
      if (it.kind === "center") {
        label.setAttribute("text-anchor", "middle");
        label.setAttribute("y", (it.r + 17).toFixed(1));
        label.textContent = n.l;
      } else {
        // Radial: each label points away from the centre, never on top of its neighbours.
        const flip = Math.cos(it.angle) < 0;
        const deg = (it.angle * 180) / Math.PI + (flip ? 180 : 0);
        const off = it.r + (isSector ? 13 : 6);
        label.setAttribute("transform", `rotate(${deg.toFixed(1)})`);
        label.setAttribute("x", (flip ? -off : off).toFixed(1));
        label.setAttribute("dy", "3.8");
        label.setAttribute("text-anchor", flip ? "end" : "start");
        const max = isSector ? 30 : 38;
        const txt = it.kind === "ref" ? `↗ ${n.l}` : n.l;
        label.textContent = txt.length > max ? `${txt.slice(0, max - 1)}…` : txt;
        if (txt.length > max) {
          const t = sEl("title");
          t.textContent = n.l;
          g.append(t);
        }
      }
      g.append(label);
    }

    g.onmouseenter = () => highlight(n.id);
    g.onmouseleave = () => highlight(mem.selected);
    g.onclick = (ev) => {
      ev.stopPropagation();
      if (it.kind === "more") {
        mem.shown.set(focus, (mem.shown.get(focus) || LEAF_PAGE) + LEAF_PAGE);
        renderMemory({ fit: true });
      } else if (it.kind === "center") {
        if (focus !== m.root.id) setFocus(m.primaryParent.get(focus) ?? m.root.id);
      } else if (isSector) {
        setFocus(n.id);
      } else if (it.kind === "ref" || it.kind === "dot") {
        focusNode(n.id);
      } else {
        const opening = $("memDetail").classList.contains("hidden");
        mem.selected = n.id;
        renderMemoryDetail(n.id);
        renderMemory({ fit: opening }); // the panel takes a side: frame the ring beside it
      }
    };
    nodeGroup.append(g);
    nodeEls.push({ it, g });
  }
  vp.append(nodeGroup);

  // Motion: what was on screen glides to its new place; what is new grows out of the sector you
  // opened (or the centre), so you see where you went.
  const prev = mem.pos;
  const spawn = prev.get(`n:${focus}`) || prev.get(`dot:${focus}`) || { x: 0, y: 0 };
  const from = new Map();
  for (const { it } of nodeEls) {
    const p = prev.get(it.key) || (it.kind === "dot" && it.parent ? prev.get(it.parent.key) : null) || spawn;
    from.set(it.key, p);
  }
  const place = (t) => {
    const at = new Map();
    for (const { it, g } of nodeEls) {
      const f = from.get(it.key);
      const x = f.x + (it.x - f.x) * t;
      const y = f.y + (it.y - f.y) * t;
      at.set(it.key, { x, y });
      g.setAttribute("transform", `translate(${x.toFixed(1)},${y.toFixed(1)})`);
    }
    for (const { e, p } of edgeEls) {
      const a = at.get(e.a.key);
      const b = at.get(e.b.key);
      if (e.tree) {
        p.setAttribute("d", `M ${a.x.toFixed(1)} ${a.y.toFixed(1)} L ${b.x.toFixed(1)} ${b.y.toFixed(1)}`);
      } else {
        // Through the inside of the ring, so relations do not cross the labels.
        const cx = (a.x + b.x) * 0.22;
        const cy = (a.y + b.y) * 0.22;
        p.setAttribute("d", `M ${a.x.toFixed(1)} ${a.y.toFixed(1)} Q ${cx.toFixed(1)} ${cy.toFixed(1)} ${b.x.toFixed(1)} ${b.y.toFixed(1)}`);
      }
    }
    return at;
  };
  const moving = nodeEls.some(({ it }) => {
    const f = from.get(it.key);
    return Math.abs(f.x - it.x) + Math.abs(f.y - it.y) > 0.5;
  });
  const token = (mem.anim = (mem.anim || 0) + 1);
  if (!moving || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    mem.pos = place(1);
  } else {
    const t0 = performance.now();
    const step = (now) => {
      if (token !== mem.anim) return;
      const t = Math.min(1, (now - t0) / ANIM_MS);
      const at = place(1 - Math.pow(1 - t, 3));
      if (t < 1) requestAnimationFrame(step);
      else mem.pos = at;
    };
    place(0);
    mem.pos = new Map(lay.items.map((it) => [it.key, { x: it.x, y: it.y }]));
    requestAnimationFrame(step);
  }

  if (mem.selected != null) highlight(mem.selected);
  if (opts.fit) fitGraph();
  else applyTransform();
}

// Interactive pan & zoom on #memSvg
(() => {
  const svg = $("memSvg");
  let drag = null;
  svg.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || e.target.closest(".mem-node")) return;
    drag = { x: e.clientX, y: e.clientY, camX: mem.cam.x, camY: mem.cam.y, moved: false };
    svg.classList.add("dragging");
    svg.setPointerCapture(e.pointerId);
  });
  svg.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    mem.cam.x = drag.camX + dx;
    mem.cam.y = drag.camY + dy;
    applyTransform();
  });
  const endDrag = (e) => {
    if (!drag) return;
    const wasClick = !drag.moved;
    drag = null;
    svg.classList.remove("dragging");
    if (wasClick && !e.target.closest(".mem-node")) {
      mem.selected = null;
      mem.selectedRefSector = null;
      $("memDetail").classList.add("hidden");
      renderMemory({ keepCam: true });
    }
  };
  svg.addEventListener("pointerup", endDrag);
  svg.addEventListener("pointercancel", endDrag);
  svg.addEventListener("wheel", (e) => {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.14 : 0.88;
    const nextK = Math.max(0.35, Math.min(2.8, mem.cam.k * factor));
    const r = svg.getBoundingClientRect();
    const mx = e.clientX - r.left - r.width / 2;
    const my = e.clientY - r.top - r.height / 2;
    const ratio = nextK / mem.cam.k;
    mem.cam.x = mx - (mx - mem.cam.x) * ratio;
    mem.cam.y = my - (my - mem.cam.y) * ratio;
    mem.cam.k = nextK;
    applyTransform();
  }, { passive: false });
})();

$("memCollapse").onclick = () => {
  mem.shown.clear();
  mem.focus = null;
  mem.selected = null;
  mem.selectedRefSector = null;
  $("memDetail").classList.add("hidden");
  renderMemory({ fit: true });
};
$("memCenter").onclick = () => fitGraph();
// Her memory in 3D, in its own window (santuario, hélice, arboleda, constelación; read only).
$("memViewer").onclick = () => invoke("open_viewer").catch((e) => add("error", String(e)));
$("memZoomIn").onclick = () => {
  mem.cam.k = Math.min(2.8, mem.cam.k * 1.2);
  applyTransform();
};
$("memZoomOut").onclick = () => {
  mem.cam.k = Math.max(0.35, mem.cam.k / 1.2);
  applyTransform();
};

function runMemorySearch(query, step = 0) {
  const m = mem.model;
  mem.search = query.trim();
  const badge = $("memSearchCount");
  if (!m || !mem.search) {
    mem.matches = [];
    mem.matchIdx = 0;
    badge.classList.add("hidden");
    renderMemory({ keepCam: true });
    return;
  }
  const q = mem.search.toLowerCase();
  const labelMatches = m.nodes.filter((n) => n.l && n.l.toLowerCase().includes(q));
  const textMatches = m.nodes.filter((n) => !(n.l && n.l.toLowerCase().includes(q)) && n.t && n.t.toLowerCase().includes(q));
  mem.matches = [...labelMatches, ...textMatches];
  if (!mem.matches.length) {
    badge.textContent = "0";
    badge.classList.remove("hidden");
    renderMemory({ keepCam: true });
    return;
  }
  mem.matchIdx = (mem.matchIdx + step + mem.matches.length) % mem.matches.length;
  const hit = mem.matches[mem.matchIdx];
  badge.textContent = `${mem.matchIdx + 1}/${mem.matches.length}`;
  badge.classList.remove("hidden");
  focusNode(hit.id, { unfold: true, pan: true });
}

$("memSearch").addEventListener("input", (e) => {
  mem.matchIdx = 0;
  runMemorySearch(e.target.value, 0);
});
$("memSearch").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && mem.matches.length) {
    e.preventDefault();
    runMemorySearch(e.target.value, e.shiftKey ? -1 : 1);
  }
});
window.addEventListener("resize", () => {
  if (isOpen("memory")) applyTransform();
});

async function loadMemoryGraph(keepState = false) {
  const g = await invoke("memory_graph");
  mem.raw = g;
  mem.model = buildGraphModel(g);
  if (!keepState) {
    mem.focus = null;
    mem.selected = null;
    mem.selectedRefSector = null;
    $("memDetail").classList.add("hidden");
  }
  renderMemory({ fit: !keepState });
  if (keepState && mem.selected != null) renderMemoryDetail(mem.selected);
}

views.loaders.memory = async () => {
  await loadMemoryGraph(!!mem.model);
};
listen("k:memoria", async () => {
  if (isOpen("memory")) {
    await loadMemoryGraph(true);
  }
});

// ── moral compass: her homeostasis, whether she may act and what she defends ──
const moral = { data: null };
// The keys of the compass axes; name and meaning are in the dictionary (axis.<key>[.what]).
const AXES = ["escepticismo", "urgencia", "afinidad", "tension"];

// The chip in the header and the line over the composer follow every state she sends.
function moralIndicators(m) {
  moral.data = m;
  const chip = $("moralState");
  chip.textContent = m.suspendida ? t("moral.chip.suspended", { omega: m.omega.toFixed(2) }) : t("moral.chip.free");
  chip.classList.toggle("bad", m.suspendida);
  chip.title = m.suspendida ? t("moral.chip.title.suspended") : t("moral.chip.title.free");
  const banner = $("moralBanner");
  banner.classList.toggle("hidden", !m.suspendida);
  if (m.suspendida) {
    banner.textContent = t("moral.banner", { omega: m.omega.toFixed(2), pulses: m.pulsos, required: m.pulsos_requeridos });
  }
  if (isOpen("moral")) renderMoral();
}

function radar(m) {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 320 220");
  svg.setAttribute("class", "radar");
  const cx = 160;
  const cy = 110;
  const R = 70;
  const dirs = [[0, -1], [1, 0], [0, 1], [-1, 0]];
  const pt = (i, v) => [cx + dirs[i][0] * R * v, cy + dirs[i][1] * R * v];
  const poly = (vals, cls) => {
    const p = document.createElementNS(NS, "polygon");
    p.setAttribute("points", vals.map((v, i) => pt(i, Math.max(0.02, Math.min(1, v))).join(",")).join(" "));
    p.setAttribute("class", cls);
    svg.append(p);
  };
  for (const r of [0.25, 0.5, 0.75, 1]) poly([r, r, r, r], "grid");
  for (let i = 0; i < 4; i++) {
    const l = document.createElementNS(NS, "line");
    const [x, y] = pt(i, 1);
    l.setAttribute("x1", cx); l.setAttribute("y1", cy); l.setAttribute("x2", x); l.setAttribute("y2", y);
    l.setAttribute("class", "spoke");
    svg.append(l);
  }
  poly(AXES.map((k) => m.atractor[k]), "attractor");
  poly(AXES.map((k) => m.vector[k]), m.suspendida ? "now bad" : "now");
  // Name and value on two lines, so the side labels fit next to the compass.
  AXES.forEach((k, i) => {
    const label = t(`axis.${k}`);
    const [x, y] = pt(i, i % 2 ? 1.1 : 1.14);
    const text = document.createElementNS(NS, "text");
    text.setAttribute("text-anchor", i === 1 ? "start" : i === 3 ? "end" : "middle");
    const top = i === 0 ? y - 16 : i === 2 ? y + 4 : y - 6;
    [label, m.vector[k].toFixed(2)].forEach((s, n) => {
      const span = document.createElementNS(NS, "tspan");
      span.setAttribute("x", x);
      span.setAttribute("y", top + 11 * (n + 1));
      if (n === 1) span.setAttribute("class", "v");
      span.textContent = s;
      text.append(span);
    });
    svg.append(text);
  });
  return svg;
}

// A 0–1 bar with its thresholds marked; `tone` is "bad", "warn" (still keeping her suspended) or "".
function gauge(value, marks, tone) {
  const g = el("div", "gauge");
  const fill = el("i", tone || "");
  fill.style.width = `${Math.round(Math.max(0, Math.min(1, value)) * 100)}%`;
  g.append(fill);
  // Each tick shows its number; what it means is on hover (long labels would overlap).
  for (const [at, label, cls] of marks) {
    const tick = el("span", `tick ${cls || ""}`);
    tick.style.left = `${at * 100}%`;
    tick.title = label;
    if (cls !== "max") tick.dataset.label = at.toFixed(2);
    g.append(tick);
  }
  return g;
}

function renderMoral() {
  const box = $("moralBody");
  const m = moral.data;
  if (!m) {
    box.replaceChildren(el("div", "loading", t("moral.loading")));
    return;
  }
  const u = m.umbrales;
  box.replaceChildren();

  // 1. Agency: may she act?
  const ag = el("div", `net-card moral-agency ${m.suspendida ? "bad" : ""}`);
  ag.append(el("h4", "", t("moral.agency")));
  const head = el("div", "agency-head");
  head.append(el("span", "agency-icon", m.suspendida ? "⛔" : "⚖"));
  const ht = el("div", "grow");
  ht.append(el("div", "agency-title", m.suspendida ? t("moral.agency.suspended") : t("moral.agency.free")));
  ht.append(el("div", "hint", m.suspendida ? t("moral.agency.hint.suspended") : t("moral.agency.hint.free")));
  head.append(ht);
  ag.append(head);
  if (m.suspendida && m.orden) ag.append(el("div", "agency-order", `«${m.orden}»`));
  const tone = (v, critical) => (v >= critical ? "bad" : m.suspendida && v > u.recuperacion ? "warn" : "");
  const om = el("div", "gauge-row");
  om.append(el("span", "gauge-k", `Ω ${m.omega.toFixed(2)}`), gauge(m.omega, [[u.recuperacion, t("moral.gauge.recovery", { v: u.recuperacion }), "ok"], [u.omega, t("moral.gauge.suspend", { v: u.omega }), "bad"], [u.chi_max * u.chi_max, t("moral.gauge.max", { v: (u.chi_max * u.chi_max).toFixed(2), chi: u.chi_max }), "max"]], tone(m.omega, u.omega)));
  ag.append(om);
  const te = el("div", "gauge-row");
  te.append(el("span", "gauge-k", t("moral.tension", { v: m.vector.tension.toFixed(2) })), gauge(m.vector.tension, [[u.recuperacion, t("moral.gauge.recovery", { v: u.recuperacion }), "ok"], [u.tension, t("moral.gauge.suspend", { v: u.tension }), "bad"]], tone(m.vector.tension, u.tension)));
  ag.append(te);
  ag.append(el("div", "hint", t("moral.marks")));
  ag.append(el("div", "hint", m.suspendida
    ? t("moral.exit", { rec: u.recuperacion, req: m.pulsos_requeridos, n: m.pulsos })
    : t("moral.enter", { omega: u.omega, tension: u.tension })));
  box.append(ag);

  // 2. The compass itself: where she is against who she is by default.
  const cp = el("div", "net-card");
  cp.append(el("h4", "", t("moral.compass")));
  const row = el("div", "compass-row");
  row.append(radar(m));
  const legend = el("div", "compass-legend");
  for (const k of AXES) {
    const d = m.vector[k] - m.atractor[k];
    const line = el("div", "axis");
    line.append(el("span", "axis-k", t(`axis.${k}`)), el("span", `axis-d ${Math.abs(d) < 0.05 ? "" : d > 0 ? "up" : "down"}`, `${d >= 0 ? "+" : ""}${d.toFixed(2)}`));
    line.title = t("moral.axis.title", { what: t(`axis.${k}.what`), now: m.vector[k].toFixed(2), base: m.atractor[k].toFixed(2) });
    legend.append(line);
  }
  legend.append(el("div", "hint", t("moral.compass.hint", { d: m.distancia.toFixed(2) })));
  row.append(legend);
  cp.append(row);
  box.append(cp);

  // 3. What she defends.
  const ax = el("div", "net-card");
  ax.append(el("h4", "", t("moral.defends", { n: m.axiomas.length })));
  if (!m.axiomas.length) ax.append(el("div", "empty", t("moral.coreEmpty")));
  for (const a of m.axiomas) {
    const r = el("div", "axiom");
    const head = el("div", "axiom-t");
    head.append(el("span", "grow", a.label));
    head.append(el("span", "axiom-chi", `χ ${a.chi.toFixed(2)}`));
    r.append(head);
    const bar = el("div", "pbar axiom-bar");
    const fill = el("i");
    fill.style.width = `${Math.round((Math.max(0, a.chi) / u.chi_max) * 100)}%`;
    bar.append(fill);
    r.append(bar);
    if (a.text && a.text !== a.label) r.append(el("div", "hint", a.text));
    ax.append(r);
  }
  ax.append(el("div", "hint", t("moral.doubt", { max: u.chi_max, pct: Math.round((1 - u.chi_max) * 100) })));
  box.append(ax);

  // 3b. What she has become (her personal core): shown only when she has something there.
  const pc = m.nucleo_personal || [];
  if (pc.length) {
    const pcCard = el("div", "net-card");
    pcCard.append(el("h4", "", t("moral.became", { n: pc.length })));
    for (const a of pc) {
      const r = el("div", "axiom");
      const head = el("div", "axiom-t");
      head.append(el("span", "grow", a.label));
      head.append(el("span", "axiom-chi", `χ ${a.chi.toFixed(2)}`));
      r.append(head);
      const bar = el("div", "pbar axiom-bar");
      const fill = el("i");
      fill.style.width = `${Math.round((Math.max(0, a.chi) / u.chi_max) * 100)}%`;
      bar.append(fill);
      r.append(bar);
      if (a.text && a.text !== a.label) r.append(el("div", "hint", a.text));
      pcCard.append(r);
    }
    box.append(pcCard);
  }

  // 4. What the model receives at the very top of its instructions.
  const dv = el("div", "net-card");
  dv.append(el("h4", "", t("moral.directive")));
  dv.append(el("pre", "log-tail directive", m.directiva));
  box.append(dv);

  // 5. Her last suspensions (they live in her memory, module «yo»).
  const hi = el("div", "net-card");
  hi.append(el("h4", "", t("moral.history")));
  if (!m.historial.length) hi.append(el("div", "empty", t("moral.history.empty")));
  for (const e of m.historial) {
    const r = el("div", "net-row");
    r.append(el("span", `tag ${e.entra ? "bad" : "body"}`, e.entra ? t("moral.enters") : t("moral.leaves")));
    const what = el("span", "grow", e.text);
    r.append(what, el("span", "hint", new Date(e.at * 1000).toLocaleString(locale(), { day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit" })));
    hi.append(r);
  }
  box.append(hi);
}

views.loaders.moral = async () => {
  moralIndicators((await invoke("state")).moral);
  renderMoral();
};
$("moralState").onclick = () => showView("moral");

// ── network panel ──
const net = { data: null, confirm: null };
const roleName = (rol) => tOr(`net.role.${rol}`, rol);

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function renderNet() {
  const d = net.data;
  if (!d) return;
  const body = $("netBody");
  body.textContent = "";

  const me = el("div", "net-card");
  me.append(el("h4", "", t("net.thisPC")));
  const r = el("div", "net-row");
  r.append(el("b", "grow", d.yo.nombre), el("span", `tag${d.yo.rol === "cerebro" ? " brain" : ""}`, roleName(d.yo.rol)));
  if (d.yo.cuerpo) r.append(el("span", "tag body", t("net.herePresent")));
  me.append(r);
  const think = el("button", "", d.yo.rol === "cerebro" ? t("net.thinkingHere") : t("net.thinkHere"));
  think.type = "button";
  think.disabled = d.yo.rol === "cerebro" || !d.yo.puede_pensar || !!d.esperando;
  think.title = d.yo.puede_pensar ? t("net.thinkHere.title") : t("net.thinkHere.noModel");
  think.onclick = async () => {
    try { net.data = await invoke("think_here"); renderNet(); } catch (e) { add("error", String(e)); }
  };
  const tr = el("div", "net-row");
  tr.append(think);
  if (d.esperando) tr.append(el("span", "hint", t("net.waiting", { name: d.esperando })));
  else if (!d.yo.puede_pensar) tr.append(el("span", "hint", t("net.noModelHint")));
  else tr.append(el("span", "hint", d.yo.modelo.split(/[\\/]/).pop()));
  me.append(tr);
  body.append(me);

  const peers = el("div", "net-card");
  peers.append(el("h4", "", t("net.peers")));
  if (!d.equipos.length) peers.append(el("div", "hint", t("net.peers.none")));
  for (const p of d.equipos) {
    const row = el("div", "net-row");
    row.append(el("b", "grow", p.name), el("span", "hint", p.addr));
    row.append(el("span", `tag${p.brain ? " brain" : ""}`, p.brain ? t("net.peer.brain") : p.think ? t("net.peer.canThink") : t("net.peer.terminal")));
    if (p.body) row.append(el("span", "tag body", t("net.peer.here")));
    row.append(el("span", "hint", p.idle < 5 ? t("net.peer.inUse") : t("net.peer.idle", { s: Math.round(p.idle) })));
    peers.append(row);
  }
  body.append(peers);

  const nb = el("div", "net-card");
  nb.append(el("h4", "", t("net.neighbors")), el("div", "hint", t("net.neighbors.hint")));
  for (const side of ["izquierda", "derecha"]) {
    const row = el("div", "net-row");
    row.append(el("span", "grow", t(`net.side.${side}`)));
    const sel = el("select");
    const names = ["", ...new Set([...d.equipos.map((p) => p.name), d.vecinos[side]].filter(Boolean))];
    for (const n of names) {
      const o = el("option", "", n || t("net.noneOption"));
      o.value = n;
      o.selected = n === d.vecinos[side];
      sel.append(o);
    }
    sel.onchange = async () => { net.data = await invoke("set_neighbor", { side, name: sel.value }); renderNet(); };
    row.append(sel);
    nb.append(row);
  }
  body.append(nb);

  // Linked computers.
  const linked = el("div", "net-card");
  linked.append(el("h4", "", t("net.paired")), el("div", "hint", t("net.paired.hint")));
  if (!d.vinculados.length) linked.append(el("div", "hint", t("net.paired.none")));
  for (const v of d.vinculados) {
    const r = el("div", "net-row");
    r.append(el("b", "grow", v.nombre), el("span", "tag", v.huella), el("span", `tag${v.en_linea ? " body" : ""}`, v.en_linea ? t("net.online") : t("net.offline")));
    if (v.sesion != null) {
      const s = el("span", "tag", t("net.session", { when: v.sesion < 60 ? t("net.session.new") : t("net.session.ago", { min: Math.round(v.sesion / 60) }) }));
      s.title = t("net.session.title");
      r.append(s);
    }
    const off = el("button", "ghost", net.confirm === v.id ? t("net.sure") : t("net.unlink"));
    off.type = "button";
    off.onclick = async () => {
      if (net.confirm !== v.id) { net.confirm = v.id; return renderNet(); }
      net.confirm = null;
      try { net.data = await invoke("unlink", { id: v.id }); renderNet(); } catch (e) { add("error", String(e)); }
    };
    r.append(off);
    linked.append(r);
  }
  body.append(linked);

  // Linking a new one.
  const pair = el("div", "net-card");
  const vin = d.vincular;
  pair.append(el("h4", "", t("net.pair")));
  if (!vin.abierto) {
    pair.append(el("div", "hint", t("net.pair.hint")));
    const open = el("button", "", t("net.pair"));
    open.type = "button";
    open.onclick = async () => { try { net.data = await invoke("pairing", { open: true }); renderNet(); } catch (e) { add("error", String(e)); } };
    const r = el("div", "net-row");
    r.append(el("span", "grow hint", t("net.pair.this", { fp: d.yo.huella })), open);
    pair.append(r);
  } else {
    pair.append(el("div", "hint", t("net.pair.open", { s: Math.round(vin.quedan) })));
    if (!vin.candidatos.length) pair.append(el("div", "hint", t("net.pair.searching")));
    for (const c of vin.candidatos) {
      const r = el("div", "net-row");
      r.append(el("b", "grow", c.name), el("span", "tag", c.huella), el("span", "hint", c.addr));
      const go = el("button", "", t("net.pair.link"));
      go.type = "button";
      go.onclick = async () => { try { net.data = await invoke("link_request", { id: c.id }); renderNet(); showLink(net.data); } catch (e) { add("error", String(e)); } };
      r.append(go);
      pair.append(r);
    }
    const close = el("button", "ghost", t("net.pair.close"));
    close.type = "button";
    close.onclick = async () => { net.data = await invoke("pairing", { open: false }); renderNet(); };
    const r = el("div", "net-row");
    r.append(el("span", "grow"), close);
    pair.append(r);
  }
  body.append(pair);

  // Networks.
  const nets = el("div", "net-card");
  nets.append(el("h4", "", t("net.networks")), el("div", "hint", t("net.networks.hint")));
  for (const p of d.redes.perfiles) {
    const r = el("div", "net-row");
    const priv = p.tipo !== "Public";
    r.append(el("span", "grow", p.nombre), el("span", `tag${priv ? " body" : ""}`, priv ? t("net.private") : t("net.public")));
    nets.append(r);
  }
  if (d.redes.perfiles.length && d.redes.perfiles.every((p) => p.tipo === "Public") && !d.redes.publicas_permitidas) {
    nets.append(el("div", "hint warn", t("net.allPublic")));
  }
  const pub = el("label", "toggle");
  const cb = el("input");
  cb.type = "checkbox";
  cb.checked = d.redes.publicas_permitidas;
  cb.onchange = async () => { net.data = await invoke("set_public_networks", { allow: cb.checked }); renderNet(); };
  pub.append(cb, t("net.allowPublic"));
  nets.append(pub);
  body.append(nets);
}

// The linking dialog: the same code on both screens, accepted on both.
function showLink(d) {
  const l = d && d.vincular && d.vincular.en_curso;
  const box = $("link");
  if (!l) return box.classList.add("hidden");
  box.classList.remove("hidden");
  $("linkTitle").textContent = t("link.title", { name: l.name });
  $("linkCode").textContent = l.codigo;
  let text, buttons;
  if (l.rechazado) {
    text = t("link.rejected", { name: l.name });
    buttons = "close";
  } else if (l.listo) {
    text = t("link.done", { name: l.name });
    buttons = "close";
  } else if (l.acepte) {
    text = t("link.waiting", { name: l.name });
    buttons = "cancel";
  } else {
    text = t("link.check", { name: l.name });
    buttons = "answer";
  }
  $("linkText").textContent = text;
  $("linkNo").classList.toggle("hidden", buttons === "close");
  $("linkNo").textContent = buttons === "cancel" ? t("link.cancel") : t("link.no");
  $("linkYes").classList.toggle("hidden", buttons !== "answer");
  $("linkDone").classList.toggle("hidden", buttons !== "close");
}

$("linkYes").onclick = async () => { try { net.data = await invoke("link_answer", { accept: true }); showLink(net.data); renderNet(); } catch (e) { add("error", String(e)); } };
$("linkNo").onclick = async () => { try { net.data = await invoke("link_answer", { accept: false }); } catch (e) { add("error", String(e)); } $("link").classList.add("hidden"); };
$("linkDone").onclick = async () => { $("link").classList.add("hidden"); net.data = await invoke("pairing", { open: false }); renderNet(); };

views.loaders.net = async () => {
  if (!net.data) $("netBody").replaceChildren(el("div", "loading", t("net.loading")));
  net.data = await invoke("red");
  renderNet();
};
listen("k:red", (e) => {
  net.data = e.payload;
  if (isOpen("net")) renderNet();
  if (e.payload.propuesta) showAsk(e.payload.propuesta);
  else $("ask").classList.add("hidden");
  // A request from the other computer opens the dialog here; while it is open, it follows.
  const l = e.payload.vincular && e.payload.vincular.en_curso;
  if (l && (!$("link").classList.contains("hidden") || (!l.listo && !l.rechazado))) showLink(e.payload);
});

function showAsk(p) {
  $("askTitle").textContent = t("ask.title", { name: p.nombre });
  $("askText").textContent = t("ask.text", { name: p.nombre });
  $("ask").classList.remove("hidden");
}
listen("k:propuesta", (e) => showAsk(e.payload));
$("askYes").onclick = async () => {
  $("ask").classList.add("hidden");
  try { await invoke("resolve_proposal", { accept: true }); } catch (e) { add("error", String(e)); }
};
$("askNo").onclick = async () => {
  $("ask").classList.add("hidden");
  try { await invoke("resolve_proposal", { accept: false }); } catch (e) { add("error", String(e)); }
};
// ── models panel ──
const mods = { data: null, profile: null, progress: new Map() };
try { mods.profile = localStorage.getItem("kaltsit.perfil"); } catch {}
const GB = (b) => `${(b / 1e9).toFixed(2)} GB`;
const KINDS = ["cerebro", "voz", "oido", "motor"]; // title and hint: models.kind.<kind>[.hint]
// What the catalog says in Spanish (a name, a note) has its English in the dictionary, by id.
const catText = (what, id, fallback) => tOr(`cat.${what}.${id}`, fallback);

async function loadModels() {
  const [m, e] = await Promise.all([invoke("models_state", { profile: mods.profile }), invoke("engine_state").catch(() => null)]);
  mods.data = m;
  mods.engine = e;
  renderModels();
}

const hhmm = (unix) => new Date(unix * 1000).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });

function engineCard(g) {
  const card = el("div", "net-card");
  card.append(el("h4", "", t("engine.title")));
  const st = g.estado;
  const head = el("div", "net-row");
  head.append(el("b", "grow", g.modelo || t("engine.noModel")));
  if (st.propio) head.append(el("span", "tag brain", t("engine.running")));
  else if (st.externo) head.append(el("span", "tag", t("engine.other")));
  else head.append(el("span", "tag", g.cerebro ? t("engine.stopped") : t("engine.notHere")));
  card.append(head);
  const info = [];
  if (st.propio) info.push(t("engine.pid", { pid: st.pid, time: hhmm(st.desde) }));
  if (g.cargado) info.push(t("engine.loaded", { name: g.cargado }));
  info.push(`${g.version || t("engine.versionUnknown")} · ${g.servidor}`);
  card.append(el("div", "hint", info.join(" · ")));

  if (g.vram) {
    const used = g.vram.usada, total = g.vram.total;
    const w = el("div", "progress");
    const bar = el("div", "pbar");
    const fill = el("i");
    fill.style.width = `${Math.min(100, (used / total) * 100).toFixed(1)}%`;
    if (used / total > 0.92) fill.style.background = "var(--amber)";
    bar.append(fill);
    w.append(bar, el("div", "hint", t("engine.vram", { used: used.toFixed(1), total: total.toFixed(1), est: g.estimado.toFixed(1) })));
    card.append(w);
    if (!st.responde && g.estimado > total - used) {
      card.append(el("div", "hint warn", t("engine.tight")));
    }
  }

  const row = el("div", "net-row");
  row.append(el("span", "grow", t("engine.context")));
  const ctx = el("select");
  const sizes = [2048, 4096, 6144, 8192, 12288, 16384];
  if (g.ctx && !sizes.includes(g.ctx)) sizes.push(g.ctx);
  for (const n of sizes.sort((a, b) => a - b)) {
    const o = el("option", "", t("engine.tokens", { n }));
    o.value = n;
    o.selected = n === g.ctx;
    ctx.append(o);
  }
  const gpu = el("select");
  for (const [v, label] of [["no", t("engine.gpuAll")], ["si", t("engine.gpuAuto")]]) {
    const o = el("option", "", label);
    o.value = v;
    o.selected = (v === "si") === g.gpu_auto;
    gpu.append(o);
  }
  const apply = el("button", "ghost", g.cerebro ? t("engine.apply") : t("engine.save"));
  apply.type = "button";
  apply.title = g.cerebro ? t("engine.apply.title") : t("engine.save.title");
  apply.onclick = async () => {
    try { mods.engine = await invoke("engine_set", { ctx: Number(ctx.value), gpuAuto: gpu.value === "si" }); renderModels(); } catch (e) { add("error", String(e)); }
  };
  row.append(ctx, gpu, apply);
  card.append(row);

  const actions = el("div", "net-row");
  actions.append(el("span", "grow hint", g.cerebro ? "" : t("engine.onlyBrain")));
  const restart = el("button", "ghost", t("engine.restart"));
  restart.type = "button";
  restart.disabled = !g.cerebro;
  restart.onclick = async () => {
    try { mods.engine = await invoke("engine_restart"); renderModels(); } catch (e) { add("error", String(e)); }
  };
  actions.append(restart);
  card.append(actions);

  if (g.motores.length) {
    card.append(el("div", "hint", t("engine.found")));
    for (const m of g.motores) {
      const r = el("div", "net-row");
      const using = m.origen === "configurado";
      r.append(el("span", "grow hint", m.ruta), el("span", "tag", tOr(`engine.origin.${m.origen}`, m.origen)));
      if (m.version) r.append(el("span", "tag", m.version));
      if (using) r.append(el("span", "tag body", t("engine.inUse")));
      else {
        const b = el("button", "ghost", t("engine.use"));
        b.type = "button";
        b.onclick = async () => {
          try { mods.engine = await invoke("engine_set", { servidor: m.ruta }); renderModels(); } catch (e) { add("error", String(e)); }
        };
        r.append(b);
      }
      card.append(r);
    }
  } else if (!g.puede_pensar) {
    card.append(el("div", "hint warn", t("engine.none")));
  }
  if (g.log.length) {
    const pre = el("pre", "log-tail", g.log.join("\n"));
    card.append(el("div", "hint", t("engine.lastLog")), pre);
  }
  return card;
}

function voiceControls(v) {
  const box = el("div", "net-row");
  const on = el("label", "toggle");
  const cb = el("input");
  cb.type = "checkbox";
  cb.checked = v.activa;
  cb.onchange = async () => { try { mods.data.voz = await invoke("set_voice", { active: cb.checked }); applyState(await invoke("state")); renderModels(); } catch (e) { add("error", String(e)); } };
  on.append(cb, t("voice.speak"));
  box.append(on);
  const speed = el("select");
  for (const s of [0.8, 0.9, 1.0, 1.1, 1.2, 1.3]) {
    const o = el("option", "", t("voice.speed", { v: s.toFixed(1) }));
    o.value = s;
    o.selected = Math.abs(s - v.velocidad) < 0.01;
    speed.append(o);
  }
  speed.onchange = async () => { try { mods.data.voz = await invoke("set_voice", { speed: Number(speed.value) }); } catch (e) { add("error", String(e)); } };
  box.append(el("span", "grow"), speed);
  const test = el("button", "ghost", t("voice.test"));
  test.type = "button";
  test.disabled = !v.lista;
  test.onclick = async () => { try { mods.data.voz = await invoke("voice_test"); } catch (e) { add("error", String(e)); } };
  box.append(test);
  const wrap = el("div");
  const said = v.idioma === "en" ? t("voice.english") : "";
  wrap.append(box, el("div", v.lista ? "hint" : "hint warn", v.lista ? t("voice.speaksWith", { name: v.nombre, said }) : v.falta || t("voice.none")));
  if (v.error) wrap.append(el("div", "hint warn", t("voice.lastError", { e: v.error })));
  return wrap;
}

function progressBar(p) {
  const w = el("div", "grow progress");
  const bar = el("div", "pbar");
  const fill = el("i");
  fill.style.width = `${p.total ? ((p.hecho / p.total) * 100).toFixed(1) : 0}%`;
  bar.append(fill);
  let text = tOr(`phase.${p.fase}`, p.fase);
  if (p.fase === "descargando" || p.fase === "verificando") {
    const secs = (performance.now() - p.t0) / 1000;
    const speed = secs > 1 ? ` · ${((p.hecho - p.b0) / 1e6 / secs).toFixed(1)} MB/s` : "";
    text = `${text} ${p.n}/${p.de} · ${GB(p.hecho)} / ${GB(p.total)}${speed}`;
  }
  w.append(bar, el("div", "hint", text));
  return w;
}

function modelRow(it, d) {
  const box = el("div", `model${it.cabe === "no cabe" ? " dim" : ""}`);
  const head = el("div", "net-row");
  head.append(el("b", "grow", catText("name", it.id, it.nombre)), el("span", "tag", tOr(`level.${it.nivel}`, it.nivel)));
  if (it.tipo === "cerebro") head.append(el("span", `tag fit-${it.cabe.replace(" ", "")}`, `~${it.vram.toFixed(1)} GB · ${tOr(`fit.${it.cabe}`, it.cabe)}`));
  if (it.recomendado) head.append(el("span", "tag brain", t("model.recommended")));
  if (it.en_uso) head.append(el("span", "tag body", t("model.inUse")));
  box.append(head);
  if (it.nota) box.append(el("div", "hint", catText("note", it.id, it.nota)));
  const row = el("div", "net-row");
  const p = mods.progress.get(it.id) || (it.descargando ? { fase: "descargando…", hecho: 0, total: 0 } : null);
  if (p) {
    row.append(progressBar(p));
  } else if (it.instalado) {
    row.append(el("span", "grow hint", t("model.downloaded")));
    if (it.tipo === "voz") {
      const use = el("button", it.en_uso ? "ghost" : "", it.en_uso ? t("model.inUseBtn") : t("model.use"));
      use.type = "button";
      use.disabled = it.en_uso;
      use.onclick = async () => {
        try { await invoke("set_voice", { id: it.id }); await loadModels(); } catch (e) { add("error", String(e)); }
      };
      row.append(use);
    }
    if (it.tipo === "cerebro") {
      const use = el("button", it.en_uso ? "ghost" : "", it.en_uso ? t("model.inUseBtn") : t("model.use"));
      use.type = "button";
      use.disabled = it.en_uso || !it.cabe_aqui;
      use.title = !it.cabe_aqui ? t("model.noFit") : d.cerebro ? t("model.useNow") : t("model.useLater");
      use.onclick = async () => {
        try { mods.data = await invoke("model_use", { id: it.id, profile: mods.profile }); renderModels(); } catch (e) { add("error", String(e)); }
      };
      row.append(use);
    }
  } else {
    row.append(el("span", "grow hint", t("model.size", { size: GB(it.descarga) })));
    const get = el("button", "ghost", t("model.get"));
    get.type = "button";
    get.onclick = () => startDownload([it.id]);
    row.append(get);
  }
  box.append(row);
  return box;
}

function renderModels() {
  const d = mods.data;
  if (!d) return;
  const body = $("modelsBody");
  body.textContent = "";

  const pc = el("div", "net-card");
  const e = d.equipo;
  pc.append(
    el("h4", "", t("models.thisPC")),
    el(
      "div",
      "hint",
      e.gpu
        ? t("models.hw", { gpu: e.gpu, vram: e.vram.toFixed(1), cpu: e.cpu || "?", ram: Math.round(e.ram) })
        : t("models.hwCpu", { cpu: e.cpu || "CPU", ram: Math.round(e.ram) }),
    ),
  );
  const row = el("div", "net-row");
  row.append(el("span", "grow", t("models.recFor")));
  const sel = el("select");
  for (const p of d.perfiles) {
    const o = el("option", "", catText("name", p.id, p.nombre) + (p.id === e.perfil ? t("models.thisOne") : ""));
    o.value = p.id;
    o.selected = p.id === d.perfil;
    sel.append(o);
  }
  sel.onchange = async () => {
    mods.profile = sel.value;
    try { localStorage.setItem("kaltsit.perfil", sel.value); } catch {}
    await loadModels();
  };
  row.append(sel);
  pc.append(row);
  const prof = d.perfiles.find((p) => p.id === d.perfil) || d.perfiles[0];
  pc.append(
    el(
      "div",
      "hint",
      t(prof.cpu_only ? "models.budgetCpu" : "models.budget", {
        gb: prof.presupuesto.toFixed(1),
        note: prof.nota ? " · " + catText("note", prof.id, prof.nota) : "",
      }),
    ),
  );
  if (d.perfil !== e.perfil) pc.append(el("div", "hint warn", t("models.otherPC", { dir: d.carpeta })));
  body.append(pc);
  if (mods.engine) body.append(engineCard(mods.engine));

  for (const tipo of KINDS) {
    const items = d.elementos.filter((i) => i.tipo === tipo && i.nivel !== "propio");
    if (!items.length) continue;
    const card = el("div", "net-card");
    card.append(el("h4", "", t(`models.kind.${tipo}`)), el("div", "hint", t(`models.kind.${tipo}.hint`)));
    if (tipo === "voz" && d.voz) card.append(voiceControls(d.voz));
    for (const it of items) card.append(modelRow(it, d));
    body.append(card);
    if (tipo === "cerebro") body.append(ownModelsCard(d));
  }

  if (d.packs.length) {
    const card = el("div", "net-card");
    card.append(el("h4", "", t("models.packs")), el("div", "hint", t("models.packs.hint")));
    for (const p of d.packs) {
      const r = el("div", "net-row");
      r.append(el("b", "grow", catText("name", p.id, p.nombre)), el("span", "hint", p.incluye.join(" + ")));
      const pending = p.incluye.filter((id) => !d.elementos.find((i) => i.id === id)?.instalado);
      const b = el("button", "ghost", pending.length ? t("models.pack.get", { size: GB(p.descarga) }) : t("models.pack.done"));
      b.type = "button";
      b.disabled = !pending.length || pending.some((id) => mods.progress.has(id));
      b.onclick = () => startDownload(pending);
      r.append(b);
      card.append(r);
    }
    body.append(card);
  }
}

// «Mis modelos»: any GGUF the user wants her brain to run, by link or from a file here.
const own = { open: false, busy: false, confirm: null, draft: { name: "", modelo: "", mmproj: "", ctx: "8192" } };

function field(label, input, hint) {
  const f = el("div", "field");
  const l = el("label", "", label);
  f.append(l, input);
  if (hint) f.append(el("div", "hint", hint));
  return f;
}

function ownModelsCard(d) {
  const card = el("div", "net-card");
  card.append(el("h4", "", t("own.title")), el("div", "hint", t("own.hint")));
  const mine = d.elementos.filter((i) => i.nivel === "propio");
  if (!mine.length && !own.open) card.append(el("div", "hint", t("own.none")));
  for (const it of mine) {
    const box = modelRow(it, d);
    const row = box.lastChild;
    const rm = el("button", "ghost danger", own.confirm === it.id ? t("own.removeSure") : t("own.remove"));
    rm.type = "button";
    rm.disabled = it.en_uso || mods.progress.has(it.id);
    rm.title = it.en_uso ? t("own.remove.titleInUse") : t("own.remove.title");
    rm.onclick = async () => {
      if (own.confirm !== it.id) { own.confirm = it.id; return renderModels(); }
      own.confirm = null;
      try { mods.data = await invoke("model_remove", { id: it.id, deleteFiles: true, profile: mods.profile }); } catch (e) { add("error", String(e)); }
      renderModels();
    };
    row.append(rm);
    card.append(box);
  }
  if (!own.open) {
    const add_ = el("button", "ghost", t("own.add"));
    add_.type = "button";
    add_.onclick = () => { own.open = true; renderModels(); };
    const r = el("div", "net-row");
    r.append(el("span", "grow"), add_);
    card.append(r);
    return card;
  }
  const form = el("form", "form");
  const input = (key, ph) => {
    const i = el("input");
    i.placeholder = ph;
    i.value = own.draft[key];
    i.spellcheck = false;
    i.oninput = () => (own.draft[key] = i.value);
    return i;
  };
  const name = input("name", t("own.ph.name"));
  const modelo = input("modelo", t("own.ph.model"));
  const mmproj = input("mmproj", t("own.ph.mmproj"));
  const ctx = el("select");
  for (const n of [2048, 4096, 8192, 12288, 16384, 32768]) {
    const o = el("option", "", t("engine.tokens", { n }));
    o.value = n;
    o.selected = String(n) === own.draft.ctx;
    ctx.append(o);
  }
  ctx.onchange = () => (own.draft.ctx = ctx.value);
  form.append(
    field(t("own.f.name"), name),
    field(t("own.f.model"), modelo, t("own.f.model.hint")),
    field(t("own.f.mmproj"), mmproj),
  );
  const row = el("div", "form-row");
  row.append(field(t("own.f.ctx"), ctx));
  const cancel = el("button", "ghost", t("own.cancel"));
  cancel.type = "button";
  cancel.onclick = () => { own.open = false; renderModels(); };
  const ok = el("button", "", own.busy ? t("own.checking") : t("own.addBtn"));
  ok.type = "submit";
  ok.disabled = own.busy;
  row.append(cancel, ok);
  form.append(row);
  form.onsubmit = async (e) => {
    e.preventDefault();
    if (!own.draft.modelo.trim()) return modelo.focus();
    own.busy = true;
    renderModels();
    try {
      const r = await invoke("model_add", { name: own.draft.name, modelo: own.draft.modelo, mmproj: own.draft.mmproj, ctx: Number(own.draft.ctx), profile: mods.profile });
      mods.data = r;
      own.open = false;
      own.draft = { name: "", modelo: "", mmproj: "", ctx: "8192" };
      const it = r.elementos.find((i) => i.id === r.agregado);
      if (it && !it.instalado) add("sense", t("own.added", { name: it.nombre, size: GB(it.descarga) }));
    } catch (err) {
      add("error", String(err));
    }
    own.busy = false;
    renderModels();
  };
  card.append(form);
  return card;
}

async function startDownload(ids) {
  for (const id of ids) mods.progress.set(id, { fase: "esperando", hecho: 0, total: 0 });
  renderModels();
  try {
    await invoke("model_download", { ids });
  } catch (e) {
    for (const id of ids) mods.progress.delete(id);
    add("error", String(e));
    renderModels();
  }
}

const modelsOpen = () => isOpen("models");
listen("k:descarga", async (e) => {
  const p = e.payload;
  if (["listo", "error", "cancelado"].includes(p.fase)) {
    mods.progress.delete(p.id);
    if (p.fase === "error") add("error", t("dl.error", { id: p.id, error: p.error }));
    if (p.fase === "listo" && firstRun.models?.recomendacion?.id === p.id && !firstRun.models?.recomendacion?.en_uso) {
      await invoke("model_use", { id: p.id, profile: null }).catch(() => {});
    }
    if (firstRun.shown) {
      firstRun.models = await invoke("models_state", { profile: null }).catch(() => firstRun.models);
      paintFirstRunRecommend();
    }
    if (modelsOpen()) await loadModels();
    return;
  }
  // Speed is measured from when this file (and this phase) started.
  const prev = mods.progress.get(p.id);
  const same = prev && prev.archivo === p.archivo && prev.fase === p.fase;
  p.t0 = same ? prev.t0 : performance.now();
  p.b0 = same ? prev.b0 : p.hecho;
  mods.progress.set(p.id, p);
  if (firstRun.shown) paintFirstRunRecommend();
  if (modelsOpen()) renderModels();
});
// A model swapped (here or by the network): refresh what is "en uso". k:state comes often
// (every hormone change), so at most once every few seconds, and never under an open form.
let modelsRefresh = 0;
listen("k:state", () => {
  if (!modelsOpen() || own.open || Date.now() - modelsRefresh < 4000) return;
  modelsRefresh = Date.now();
  loadModels().catch(() => {});
});
views.loaders.models = async () => {
  if (!mods.data) $("modelsBody").replaceChildren(el("div", "loading", t("models.loading")));
  modelsRefresh = Date.now();
  await loadModels();
};

// ── blueprint panel: what she would build in Minecraft, before she spends anything ──
const plan = { data: null, mode: "iso", layer: null, step: 1, seen: null };
const WOODS = { oak: "#b8945f", spruce: "#7a5a3a", birch: "#d7c98c", jungle: "#b08560", acacia: "#b5633a", dark_oak: "#4a3420", mangrove: "#7a3b33", cherry: "#e3b3ad", bamboo: "#c9b75a", crimson: "#7b3a52", warped: "#2f7f7a", pale_oak: "#e8e0d4" };
const MATERIALS = [
  ["glass", "#bfe8ff"], ["torch", "#ffcc33"], ["lantern", "#ffcc33"], ["ladder", "#a07f4f"], ["deepslate", "#4f4f55"], ["cobblestone", "#8a8a8a"],
  ["stone_brick", "#8c8c8c"], ["mossy", "#6f8a5a"], ["smooth_stone", "#a8a8a8"], ["stone", "#9a9a9a"], ["brick", "#a0503c"], ["sandstone", "#dccb8f"],
  ["quartz", "#ece6dd"], ["terracotta", "#a35e45"], ["grass_block", "#6aa84f"], ["dirt", "#7a5535"], ["mud", "#5b4b3e"], ["leaves", "#4f8f3a"],
  ["snow", "#f5f9ff"], ["iron", "#d8d8d8"], ["gold", "#f0cf45"], ["obsidian", "#2a1f3d"], ["water", "#3a6fd8"], ["lava", "#e2611d"],
  ["blackstone", "#2f2a30"], ["andesite", "#8c8f8c"], ["diorite", "#d9d6d0"], ["granite", "#9a6b58"], ["purpur", "#a77ba7"], ["prismarine", "#5aa39b"],
  ["white", "#eeeeee"], ["red", "#b3312c"], ["blue", "#3b4fa8"], ["green", "#3f6b2a"], ["yellow", "#e0c13b"], ["black", "#222"], ["gray", "#5a5a5a"],
];
const tint = (hex, k = 0.22) => {
  const n = parseInt(hex.slice(1).padEnd(6, hex.slice(1)), 16);
  const mix = (c, b) => Math.round(c * (1 - k) + b * k);
  return [mix((n >> 16) & 255, 0x1f), mix((n >> 8) & 255, 0x5f), mix(n & 255, 0xa8)];
};
function blockColor(id) {
  for (const [w, c] of Object.entries(WOODS).sort((a, b) => b[0].length - a[0].length)) {
    if (id.startsWith(w + "_")) return /_log|_wood|stem/.test(id) ? shade(c, -0.35) : c;
  }
  for (const [k, c] of MATERIALS) if (id.includes(k)) return c;
  let h = 0;
  for (const ch of id) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 35% 55%)`;
}
function shade(hex, f) {
  if (!hex.startsWith("#")) return hex;
  const n = parseInt(hex.slice(1), 16);
  const t = (c) => Math.max(0, Math.min(255, Math.round(c + (f < 0 ? c : 255 - c) * f)));
  return `rgb(${t((n >> 16) & 255)},${t((n >> 8) & 255)},${t(n & 255)})`;
}
const rgb = (c, f = 0, a = 1) => {
  let [r, g, b] = c.startsWith("#") ? tint(c) : [120, 140, 160];
  const t = (v) => Math.max(0, Math.min(255, Math.round(v + (f < 0 ? v : 255 - v) * f)));
  return `rgba(${t(r)},${t(g)},${t(b)},${a})`;
};
const prettyId = (id) => id.replace(/_/g, " ");

function canvasFor(box) {
  const c = el("canvas");
  box.append(c);
  const ratio = window.devicePixelRatio || 1;
  const w = box.clientWidth || 440, h = 340;
  c.width = Math.round(w * ratio);
  c.height = Math.round(h * ratio);
  const g = c.getContext("2d");
  g.scale(ratio, ratio);
  return { g, w, h };
}

/** Isometric, from the front-right and above: the faces it shows are the top, the front (z = 0) and the right. */
function drawIso(box, bp, upTo) {
  const { g, w, h } = canvasFor(box);
  const cells = bp.blocks.filter((b) => b[1] <= upTo);
  if (!cells.length) return;
  const key = (x, y, z) => `${x},${y},${z}`;
  const solid = new Set(cells.map((b) => key(b[0], b[1], b[2])));
  const P = (x, y, z) => [(x + z) * 0.866, (x - z) * 0.5 - y];
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const [x, y, z] of cells) {
    for (const [px, py] of [P(x, y, z), P(x + 1, y + 1, z + 1), P(x + 1, y, z), P(x, y + 1, z + 1), P(x, y, z + 1), P(x + 1, y + 1, z)]) {
      minX = Math.min(minX, px); maxX = Math.max(maxX, px); minY = Math.min(minY, py); maxY = Math.max(maxY, py);
    }
  }
  const s = Math.min((w - 24) / (maxX - minX), (h - 24) / (maxY - minY), 40);
  const ox = (w - (maxX - minX) * s) / 2 - minX * s, oy = (h - (maxY - minY) * s) / 2 - minY * s;
  const S = (x, y, z) => { const [px, py] = P(x, y, z); return [ox + px * s, oy + py * s]; };
  const face = (pts, fill) => {
    g.beginPath();
    pts.forEach(([x, y], i) => (i ? g.lineTo(x, y) : g.moveTo(x, y)));
    g.closePath();
    g.fillStyle = fill;
    g.fill();
    g.stroke();
  };
  g.lineWidth = Math.max(0.5, Math.min(1, s / 14));
  g.strokeStyle = "rgba(232,242,255,0.55)";
  cells.sort((a, b) => (a[0] - a[2]) - (b[0] - b[2]) || a[1] - b[1]);
  for (const [x, y, z, id] of cells) {
    const col = blockColor(id);
    const a = /glass/.test(id) ? 0.45 : 0.92;
    if (!solid.has(key(x, y + 1, z))) face([S(x, y + 1, z), S(x + 1, y + 1, z), S(x + 1, y + 1, z + 1), S(x, y + 1, z + 1)], rgb(col, 0.18, a));
    if (!solid.has(key(x + 1, y, z))) face([S(x + 1, y, z), S(x + 1, y, z + 1), S(x + 1, y + 1, z + 1), S(x + 1, y + 1, z)], rgb(col, -0.32, a));
    if (!solid.has(key(x, y, z - 1))) face([S(x, y, z), S(x + 1, y, z), S(x + 1, y + 1, z), S(x, y + 1, z)], rgb(col, -0.12, a));
  }
}

/** One layer seen from above, the front row at the bottom. */
function drawLayer(box, bp, y) {
  const { g, w, h } = canvasFor(box);
  const [W, , D] = bp.size;
  const s = Math.min((w - 24) / W, (h - 24) / D, 28);
  const ox = (w - W * s) / 2, oy = (h - D * s) / 2;
  g.strokeStyle = "rgba(232,242,255,0.25)";
  for (let i = 0; i <= W; i++) { g.beginPath(); g.moveTo(ox + i * s, oy); g.lineTo(ox + i * s, oy + D * s); g.stroke(); }
  for (let j = 0; j <= D; j++) { g.beginPath(); g.moveTo(ox, oy + j * s); g.lineTo(ox + W * s, oy + j * s); g.stroke(); }
  g.strokeStyle = "rgba(232,242,255,0.7)";
  for (const [x, by, z, id] of bp.blocks) {
    if (by !== y) continue;
    const px = ox + x * s, py = oy + (D - 1 - z) * s;
    g.fillStyle = rgb(blockColor(id), 0, /glass/.test(id) ? 0.5 : 0.95);
    g.fillRect(px + 1, py + 1, s - 2, s - 2);
    g.strokeRect(px + 1, py + 1, s - 2, s - 2);
  }
  g.fillStyle = "rgba(232,242,255,0.8)";
  g.font = "10px Cascadia Mono, Consolas, monospace";
  g.fillText(t("plan.front"), ox, oy + D * s + 12);
}

function planOrder(o) {
  invoke("minecraft_order", { order: o }).catch((e) => add("error", String(e)));
}

const PHASES = ["clear", "build", "cleanup", "check", "waiting", "paused", "done", "cancelled"]; // plan.phase.<phase>

function renderPlan() {
  const body = $("planBody");
  const d = plan.data;
  body.textContent = "";
  if (!d) return body.append(el("div", "loading", t("plan.loading")));
  const bp = d.blueprint && d.blueprint.blocks ? d.blueprint : null;
  const pr = d.progress && d.progress.name ? d.progress : null;
  if (!bp && !pr) {
    body.append(el("div", "empty", d.connected ? t("plan.empty.connected") : t("plan.empty.offline")));
    return;
  }
  if (bp) {
    const card = el("div", "net-card");
    const head = el("div", "net-row");
    head.append(el("b", "grow", bp.name));
    head.append(el("span", "tag", t("plan.size", { w: bp.size[0], d: bp.size[2], h: bp.size[1] })));
    head.append(el("span", "tag", t("plan.blocks", { n: bp.blocks.length, plus: bp.partial ? "+" : "" })));
    const building = pr && !["done", "cancelled"].includes(pr.phase) && pr.name === bp.name;
    head.append(el("span", `tag ${building ? "body" : "warn"}`, building ? t("plan.building") : bp.status === "preview" ? t("plan.preview") : bp.status));
    card.append(head);
    if (bp.problem) card.append(el("div", "hint bad", t("plan.problem", { problem: bp.problem })));
    const box = el("div", "bp");
    card.append(box);
    const H = bp.size[1];
    if (plan.layer === null || plan.layer >= H || plan.seen !== bp.name) {
      plan.layer = plan.mode === "iso" ? H - 1 : 0;
      plan.seen = bp.name;
    }
    const tools = el("div", "bp-tools");
    for (const [m, label] of [["iso", t("plan.mode.iso")], ["layer", t("plan.mode.layer")]]) {
      const b = el("button", plan.mode === m ? "on" : "", label);
      b.type = "button";
      b.onclick = () => { plan.mode = m; plan.layer = m === "iso" ? H - 1 : 0; renderPlan(); };
      tools.append(b);
    }
    const title = el("div", "bp-title");
    title.append(el("b", "", bp.name), t("plan.turn", { w: bp.size[0], d: bp.size[2], h: bp.size[1], deg: bp.turns * 90 }));
    box.append(tools, title);
    requestAnimationFrame(() => (plan.mode === "iso" ? drawIso(box, bp, plan.layer) : drawLayer(box, bp, plan.layer)));
    if (H > 1) {
      const row = el("label", "bp-layer");
      const r = el("input");
      r.type = "range";
      r.min = 0;
      r.max = H - 1;
      r.value = plan.layer;
      const lab = el("span", "", t(plan.mode === "iso" ? "plan.upToLayer" : "plan.layer", { n: plan.layer + 1, h: H }));
      r.oninput = () => { plan.layer = Number(r.value); renderPlan(); };
      row.append(lab, r);
      card.append(row);
    }
    if (bp.status === "preview" && d.connected && !building) {
      const ctl = el("div", "plan-controls");
      const pad = el("div", "pad");
      const mv = (right, up, forward) => () => planOrder({ action: "preview_move", right: right * plan.step, up: up * plan.step, forward: forward * plan.step });
      const cells = [["⤒", mv(0, 1, 0), t("plan.move.up")], ["↑", mv(0, 0, 1), t("plan.move.far")], ["⤓", mv(0, -1, 0), t("plan.move.down")], ["←", mv(-1, 0, 0), t("plan.move.left")], ["↓", mv(0, 0, -1), t("plan.move.near")], ["→", mv(1, 0, 0), t("plan.move.right")]];
      for (const [arrow, fn, tip] of cells) {
        const b = el("button", "ghost", arrow);
        b.type = "button";
        b.title = t("plan.move", { dir: tip });
        b.onclick = fn;
        pad.append(b);
      }
      const col = el("div", "col");
      const step = el("select");
      for (const n of [1, 2, 3, 5, 10]) {
        const o = el("option", "", t(n > 1 ? "plan.step.other" : "plan.step.one", { n }));
        o.value = n;
        o.selected = n === plan.step;
        step.append(o);
      }
      step.onchange = () => (plan.step = Number(step.value));
      const rot = el("button", "ghost", t("plan.rotate"));
      rot.type = "button";
      rot.onclick = () => planOrder({ action: "preview_rotate", turns: 1 });
      col.append(field(t("plan.step"), step), rot);
      const act = el("div", "col");
      const go = el("button", "", t("plan.build"));
      go.type = "button";
      go.disabled = !!bp.problem;
      go.title = bp.problem ? t("plan.build.blocked") : t("plan.build.title");
      go.onclick = () => planOrder({ action: "build_confirm" });
      const no = el("button", "ghost danger", t("plan.discard"));
      no.type = "button";
      no.onclick = () => planOrder({ action: "build_discard" });
      act.append(go, no);
      ctl.append(pad, col, el("span", "grow"), act);
      card.append(ctl);
      const m = bp.moved || [0, 0, 0];
      if (m.some((v) => v)) card.append(el("div", "hint", t("plan.moved", { r: m[0], u: m[1], f: m[2] })));
    }
    body.append(card);

    const mats = el("div", "net-card");
    mats.append(el("h4", "", t("plan.materials")));
    const extra = [];
    if (bp.clear) extra.push(t("plan.clear", { n: bp.clear }));
    if (bp.foundation) extra.push(t("plan.foundation", { n: bp.foundation }));
    if (bp.creative) extra.push(t("plan.creative"));
    if (extra.length) mats.append(el("div", "hint", extra.join(" · ")));
    const table = el("table", "mats");
    let missing = 0;
    for (const [id, need, miss] of bp.materials) {
      const tr = el("tr");
      const name = el("td");
      const sw = el("span", "sw");
      sw.style.background = rgb(blockColor(id));
      name.append(sw, prettyId(id));
      tr.append(name, el("td", "n", String(need)), el("td", `n${miss ? " miss" : ""}`, miss ? t("plan.missing", { n: miss }) : "✓"));
      table.append(tr);
      missing += miss;
    }
    mats.append(table);
    if (missing) mats.append(el("div", "hint warn", t("plan.missingHint")));
    body.append(mats);
  }
  if (pr) {
    const card = el("div", "net-card");
    const head = el("div", "net-row");
    head.append(el("h4", "grow", t("plan.work", { name: pr.name })), el("span", `tag ${pr.phase === "waiting" ? "warn" : pr.phase === "done" ? "body" : ""}`, PHASES.includes(pr.phase) ? t(`plan.phase.${pr.phase}`) : pr.phase));
    card.append(head);
    const total = pr.placed + pr.remaining;
    const w = el("div", "progress");
    const bar = el("div", "pbar");
    const fill = el("i");
    fill.style.width = `${total ? ((pr.placed / total) * 100).toFixed(1) : 100}%`;
    bar.append(fill);
    w.append(bar, el("div", "hint", t("plan.progress", { placed: pr.placed, remaining: pr.remaining, broken: pr.broken ? t("plan.progress.broken", { n: pr.broken }) : "", skipped: pr.skipped ? t("plan.progress.skipped", { n: pr.skipped }) : "" })));
    card.append(w);
    if (pr.missing && pr.missing.length) card.append(el("div", "hint warn", t("plan.lacks", { list: pr.missing.map(([id, n]) => `${n}× ${prettyId(id)}`).join(", ") })));
    if (!["done", "cancelled"].includes(pr.phase) && d.connected) {
      const row = el("div", "net-row");
      row.append(el("span", "grow"));
      const paused = pr.phase === "paused";
      const pz = el("button", "ghost", paused ? t("plan.resume") : t("plan.pause"));
      pz.type = "button";
      pz.onclick = () => planOrder({ action: paused ? "resume_build" : "pause_build" });
      const cancel = el("button", "ghost danger", t("plan.cancel"));
      cancel.type = "button";
      cancel.onclick = () => planOrder({ action: "cancel_build" });
      row.append(pz, cancel);
      card.append(row);
    }
    body.append(card);
  }
}

listen("k:plano", (e) => {
  const before = plan.data && plan.data.blueprint && plan.data.blueprint.name;
  plan.data = e.payload;
  const bp = e.payload.blueprint;
  if (isOpen("plan")) renderPlan();
  else if (bp && bp.blocks && (bp.name !== before || bp.status === "preview")) {
    const tab = document.querySelector('#tabs [data-view="plan"]');
    if (tab && !tab.querySelector(".badge")) tab.append(el("span", "badge"));
  }
});
views.loaders.plan = async () => {
  const tab = document.querySelector('#tabs [data-view="plan"] .badge');
  if (tab) tab.remove();
  plan.data = await invoke("minecraft_plan");
  renderPlan();
};
window.addEventListener("resize", () => { if (isOpen("plan")) renderPlan(); });

// ── accessories panel ──
const acc = { data: null, progress: new Map(), confirm: null, loading: false };
const accKey = (id, mods) => `${id}|${mods}`;
const launcherHint = () => t("acc.launcherHint");

async function loadAccessories() {
  acc.loading = true;
  if (!acc.data) renderAccessories();
  try { acc.data = await invoke("accessories_state"); } finally { acc.loading = false; }
  renderAccessories();
}

function accButton(text, cls, onclick, disabled) {
  const b = el("button", cls, text);
  b.type = "button";
  b.disabled = !!disabled;
  b.onclick = onclick;
  return b;
}

function accSourceCard(d) {
  const card = el("div", "net-card");
  const head = el("div", "net-row");
  head.append(el("h4", "grow", t("acc.catalog")));
  const from = { github: t("acc.from.github"), local: t("acc.from.local"), "copia guardada": t("acc.from.saved") }[d.origen] || d.origen;
  head.append(el("span", `tag${d.error ? " warn" : " body"}`, d.error ? t("acc.offline", { from }) : t("acc.fromLabel", { from })));
  head.append(accButton(t("acc.refresh"), "ghost", () => loadAccessories().catch((e) => add("error", String(e))), acc.loading));
  card.append(head);
  card.append(el("div", "hint", t("acc.branch", { repo: d.repo, branch: d.rama, catalog: d.catalogo ? t("acc.catalogVersion", { v: d.catalogo }) : "" })));
  if (d.error) card.append(el("div", "hint warn", d.error + (d.local ? t("acc.usingCopy", { local: d.local }) : "")));
  const adv = el("details", "adv");
  adv.append(el("summary", "", t("acc.advanced")));
  const form = el("form", "form");
  const repo = el("input");
  repo.value = d.repo;
  repo.spellcheck = false;
  const rama = el("input");
  rama.value = d.rama;
  const token = el("input");
  token.type = "password";
  token.autocomplete = "off";
  token.placeholder = d.token ? t("acc.token.saved") : t("acc.token.private");
  const row = el("div", "form-row");
  row.append(field(t("acc.repo"), repo), field(t("acc.branchLabel"), rama));
  form.append(row, field(t("acc.tokenLabel"), token, t("acc.token.hint")));
  const actions = el("div", "form-row");
  actions.append(el("span", "grow"));
  if (d.token) actions.append(accButton(t("acc.token.remove"), "ghost danger", async () => { try { acc.data = await invoke("accessories_config", { token: "" }); renderAccessories(); } catch (e) { add("error", String(e)); } }));
  const save = el("button", "", t("acc.save"));
  save.type = "submit";
  actions.append(save);
  form.append(actions);
  form.onsubmit = async (e) => {
    e.preventDefault();
    const args = { repo: repo.value, rama: rama.value };
    if (token.value.trim()) args.token = token.value;
    try { acc.data = await invoke("accessories_config", args); renderAccessories(); } catch (err) { add("error", String(err)); }
  };
  adv.append(form);
  card.append(adv);
  return card;
}

function accInstanceRow(m, inst, info) {
  const box = el("div", "acc-inst");
  const head = el("div", "net-row");
  head.append(el("b", "grow", info.nombre), el("span", "tag", info.launcher));
  if (info.version) head.append(el("span", `tag${info.version === m.minecraft || !m.minecraft ? "" : " warn"}`, info.version));
  if (info.loader) head.append(el("span", `tag${inst.loader_ok ? "" : " bad"}`, info.loader));
  if (inst.instalada !== null && inst.instalada !== undefined) {
    const current = inst.instalada === m.version;
    head.append(el("span", `tag ${inst.desactivada ? "warn" : current ? "body" : "warn"}`, inst.desactivada ? t("acc.disabled", { v: inst.instalada }) : current ? t("acc.installed", { v: inst.instalada }) : t("acc.youHave", { v: inst.instalada || "?" })));
  }
  box.append(head, el("div", "acc-path", inst.mods));
  if (!inst.loader_ok) box.append(el("div", "hint bad", t("acc.badLoader", { loader: info.loader })));
  else if (!inst.version_ok) box.append(el("div", "hint warn", t("acc.badVersion", { mc: m.minecraft, v: info.version })));
  if (inst.falta.includes("fabric-api")) {
    const r = el("div", "net-row");
    r.append(el("span", "grow hint warn", t("acc.needFabricApi")));
    const p = acc.progress.get(accKey("fabric-api", inst.mods));
    if (p) r.append(accProgress(p));
    else r.append(accButton(t("acc.installFabricApi"), "ghost", () => accFabricApi(inst.mods, info.version || m.minecraft), !(info.version || m.minecraft)));
    box.append(r);
  }
  const row = el("div", "net-row");
  const p = acc.progress.get(accKey(m.id, inst.mods));
  if (p) {
    row.append(accProgress(p));
  } else {
    row.append(el("span", "grow"));
    row.append(accButton(t("acc.openFolder"), "ghost", () => invoke("open_folder", { path: info.existe ? inst.mods : inst.mods.replace(/[\\/]mods$/, "") }).catch((e) => add("error", String(e)))));
    const installed = inst.instalada !== null && inst.instalada !== undefined;
    if (installed) {
      const k = accKey(m.id, inst.mods);
      row.append(accButton(acc.confirm === k ? t("net.sure") : t("acc.uninstall"), "ghost danger", async () => {
        if (acc.confirm !== k) { acc.confirm = k; return renderAccessories(); }
        acc.confirm = null;
        try { acc.data = await invoke("accessory_uninstall", { id: m.id, mods: inst.mods }); } catch (e) { add("error", String(e)); }
        renderAccessories();
      }));
    }
    const current = installed && inst.instalada === m.version && !inst.desactivada;
    row.append(accButton(current ? t("acc.reinstall") : installed ? t("acc.updateTo", { v: m.version }) : t("acc.install"), current ? "ghost" : "", () => accInstall(m.id, inst.mods), !inst.loader_ok));
  }
  box.append(row);
  return box;
}

function accProgress(p) {
  const w = el("div", "grow progress");
  const bar = el("div", "pbar busy");
  bar.append(el("i"));
  w.append(bar, el("div", "hint", tOr(`acc.phase.${p.fase}`, p.fase)));
  return w;
}

function renderAccessories() {
  const body = $("accBody");
  const d = acc.data;
  if (!d) {
    body.replaceChildren(el("div", "loading", t("acc.loading")));
    return;
  }
  body.textContent = "";
  body.append(accSourceCard(d));
  if (!d.modulos.length) body.append(el("div", "empty", d.error ? t("acc.empty.error") : t("acc.empty")));
  for (const m of d.modulos) {
    const card = el("div", "net-card");
    const head = el("div", "net-row");
    head.append(el("b", "grow", m.nombre), el("span", "tag", `v${m.version}`));
    if (m.objetivo) head.append(el("span", "tag", m.objetivo));
    card.append(head);
    if (m.descripcion) card.append(el("div", "hint", m.descripcion));
    if (m.requiere.length) card.append(el("div", "hint", t("acc.needs", { list: m.requiere.join(", ") })));
    if (!m.instancias.length) card.append(el("div", "hint warn", t("acc.noMinecraft", { hint: launcherHint() })));
    m.instancias.forEach((inst, i) => card.append(accInstanceRow(m, inst, d.instancias[i])));
    body.append(card);
  }
  // Folders added by hand.
  const folders = el("div", "net-card");
  folders.append(el("h4", "", t("acc.folders")), el("div", "hint", launcherHint()));
  for (const i of d.instancias.filter((x) => x.propia)) {
    const r = el("div", "net-row");
    r.append(el("span", "grow acc-path", i.mods));
    r.append(accButton(t("acc.remove"), "ghost", async () => { try { acc.data = await invoke("accessories_config", { remove: i.mods }); renderAccessories(); } catch (e) { add("error", String(e)); } }));
    folders.append(r);
  }
  const form = el("form", "net-row");
  const path = el("input");
  path.placeholder = t("acc.path.placeholder");
  path.spellcheck = false;
  const ok = el("button", "ghost", t("acc.addFolder"));
  ok.type = "submit";
  form.append(path, ok);
  form.onsubmit = async (e) => {
    e.preventDefault();
    if (!path.value.trim()) return path.focus();
    try { acc.data = await invoke("accessories_config", { add: path.value }); renderAccessories(); } catch (err) { add("error", String(err)); }
  };
  folders.append(form);
  body.append(folders);
}

async function accInstall(id, mods) {
  acc.progress.set(accKey(id, mods), { fase: "descargando" });
  renderAccessories();
  try { await invoke("accessory_install", { id, mods }); } catch (e) { acc.progress.delete(accKey(id, mods)); add("error", String(e)); renderAccessories(); }
}

async function accFabricApi(mods, version) {
  acc.progress.set(accKey("fabric-api", mods), { fase: "descargando" });
  renderAccessories();
  try { await invoke("accessory_fabric_api", { mods, version }); } catch (e) { acc.progress.delete(accKey("fabric-api", mods)); add("error", String(e)); renderAccessories(); }
}

listen("k:accesorio", async (e) => {
  const p = e.payload;
  const k = accKey(p.id, p.mods);
  if (p.fase === "listo" || p.fase === "error") {
    acc.progress.delete(k);
    if (p.fase === "error") add("error", `${p.id}: ${p.error}`);
    else add("sense", t("acc.done", { what: p.id === "fabric-api" ? "Fabric API" : p.id, mods: p.mods, local: p.origen === "local" ? t("acc.done.local") : "" }));
    if (isOpen("accessories")) await loadAccessories().catch(() => renderAccessories());
    return;
  }
  acc.progress.set(k, p);
  if (isOpen("accessories")) renderAccessories();
});
views.loaders.accessories = loadAccessories;

setInterval(() => invoke("state").then(applyState).catch(() => {}), 15000);

boot().catch((e) => {
  repaintLanguage();
  add("error", t("boot.error", { e }));
});
