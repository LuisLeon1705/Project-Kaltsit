// On start: who lives on this desktop this time. One or more companions; they share the model
// (taking turns) and the chat. "Remember" skips this window next time (`--elegir` brings it back).
"use strict";

const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);

function paint() {
  for (const e of document.querySelectorAll("[data-i18n]")) e.textContent = t(e.dataset.i18n);
}

async function main() {
  const info = await invoke("characters");
  setLang(info.idioma || "es");
  paint();
  for (const c of info.personajes) {
    const row = document.createElement("label");
    row.className = "who";
    row.style.setProperty("--c", c.color);
    const box = document.createElement("input");
    box.type = "checkbox";
    box.value = c.id;
    box.checked = info.elegidos.includes(c.id);
    const name = document.createElement("b");
    name.textContent = c.name;
    const note = document.createElement("span");
    note.textContent = t(`sel.who.${c.id}`);
    // Her body on the desktop: her Arknights chibi or her pixel art.
    const body = document.createElement("select");
    body.className = "body";
    body.dataset.id = c.id;
    for (const k of ["spine", "pixel"]) {
      const o = document.createElement("option");
      o.value = k;
      o.textContent = t(`body.${k}`);
      o.selected = c.cuerpo === k;
      body.append(o);
    }
    const text = document.createElement("div");
    text.className = "text";
    text.append(name, note);
    row.append(box, text, body);
    $("list").append(row);
  }
  const chosen = () => [...document.querySelectorAll("#list input:checked")].map((x) => x.value);
  const sync = () => { $("go").disabled = !chosen().length; };
  $("list").addEventListener("change", sync);
  sync();
  $("go").onclick = async () => {
    $("go").disabled = true;
    try {
      const bodies = Object.fromEntries([...document.querySelectorAll("#list select.body")].map((s) => [s.dataset.id, s.value]));
      await invoke("start_crew", { ids: chosen(), remember: $("remember").checked, bodies });
    } catch (err) {
      $("error").textContent = String(err);
      sync();
    }
  };
}

main().catch((err) => { $("error").textContent = String(err); });
