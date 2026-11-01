// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accessories;
mod activity;
mod asr;
mod hearing;
mod openmic;
mod catalog;
mod character;
mod common;
mod clock;
mod cthg;
mod download;
mod import;
pub mod homeostasis;
mod hormones;
pub mod instinct;
pub mod minecraft_bridge;
mod life;
mod lang;
mod llm;
mod local;
mod migrate;
mod mind;
mod models;
mod modules;
mod motor;
mod net;
mod origin;
mod personal_core;
mod screen;
mod sensor;
mod voice;

use life::Core;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

type Shared = Arc<Core>;

// Async so they run off the main thread: the window never waits for her mind.
type Reply = Result<Value, String>;

#[tauri::command]
async fn history(crew: State<'_, Arc<life::Crew>>) -> Reply {
    // One chat for all: the lead's (the user's messages, what was seen) and every companion's
    // answers, in time order, each saying whose.
    let lead = crew.lead();
    let mut h = lead.mind().chat_json();
    let mut msgs: Vec<Value> = Vec::new();
    for core in &crew.all {
        let mine = core.mind().chat_json();
        for mut m in mine["messages"].as_array().cloned().unwrap_or_default() {
            let theirs = m["role"] == "assistant";
            if theirs || std::sync::Arc::ptr_eq(core, lead) {
                if theirs {
                    m["who"] = json!(core.who.id);
                }
                msgs.push(m);
            }
        }
    }
    msgs.sort_by(|a, b| a["at"].as_f64().unwrap_or(0.0).total_cmp(&b["at"].as_f64().unwrap_or(0.0)));
    h["messages"] = json!(msgs);
    Ok(h)
}

/// The state of companion `who` (the lead's without one), with everyone in the chat.
#[tauri::command]
async fn state(crew: State<'_, Arc<life::Crew>>, who: Option<String>) -> Reply {
    let mut s = crew.get(who.as_deref()).state_json();
    s["crew"] = json!(crew.all.iter().map(|c| json!({ "id": c.who.id, "name": c.who.name, "color": c.who.color })).collect::<Vec<_>>());
    Ok(s)
}

#[tauri::command]
async fn memory(crew: State<'_, Arc<life::Crew>>, who: Option<String>) -> Reply {
    Ok(crew.get(who.as_deref()).mind().memory_json())
}

/// The viewer's snapshot of a companion's memory (read only).
#[tauri::command]
async fn memory_graph(crew: State<'_, Arc<life::Crew>>, who: Option<String>) -> Reply {
    Ok(crew.get(who.as_deref()).mind().graph_json())
}

/// Opens (or brings forward) the 3D viewer of a companion's memory: one window each.
#[tauri::command]
async fn open_viewer(app: tauri::AppHandle, crew: State<'_, Arc<life::Crew>>, who: Option<String>) -> Reply {
    let core = crew.get(who.as_deref()).clone();
    let label = if Arc::ptr_eq(&core, crew.lead()) { "visor".to_string() } else { format!("visor-{}", core.who.id) };
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(Value::Null);
    }
    let w = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(format!("visor.html?who={}", core.who.id).into()))
        .title(lang::tr(format!("{} · memoria · {}", core.who.name, core.net.name), format!("{} · memory · {}", core.who.name, core.net.name)))
        .inner_size(1180.0, 780.0)
        .min_inner_size(640.0, 480.0)
        .build()
        .map_err(|e| e.to_string())?;
    // Like the chat: her eyes must not read her own memory on the screen.
    if !core.local.lock().unwrap_or_else(|e| e.into_inner()).chat_en_capturas {
        if let Ok(h) = w.hwnd() {
            screen::exclude_from_capture(h.0 as isize);
        }
    }
    Ok(Value::Null)
}

#[tauri::command]
async fn red(core: State<'_, Shared>) -> Reply {
    Ok(core.net_json())
}

#[tauri::command]
async fn send(crew: State<'_, Arc<life::Crew>>, text: String, voz: Option<Value>) -> Reply {
    let text: String = text.trim().chars().take(4000).collect();
    if text.is_empty() {
        return Ok(Value::Bool(false));
    }
    crew.inner().hear(text, voz).map(|_| Value::Bool(true))
}

#[tauri::command]
async fn look_now(core: State<'_, Shared>) -> Reply {
    core.inner().look_now().map(|_| Value::Null)
}

#[tauri::command]
async fn type_chat(core: State<'_, Shared>, text: String) -> Reply {
    core.inner().type_chat(&text);
    Ok(Value::Null)
}

#[tauri::command]
async fn set_config(core: State<'_, Shared>, crew: State<'_, Arc<life::Crew>>, vision: Option<bool>, wander: Option<bool>, min_gap: Option<u64>, reposo: Option<bool>, minecraft: Option<bool>, language: Option<String>) -> Reply {
    // The switches of the top bar are for all of them (rest, eyes, wandering, language);
    // Minecraft is the lead's.
    core.inner().configure(&json!({ "vision": vision, "wander": wander, "min_gap": min_gap, "reposo": reposo, "minecraft": minecraft, "language": language }))?;
    for other in crew.all.iter().filter(|c| !Arc::ptr_eq(c, core.inner())) {
        if let Err(e) = other.configure(&json!({ "vision": vision, "wander": wander, "min_gap": min_gap, "reposo": reposo, "language": language })) {
            eprintln!("{}: {e}", other.who.name);
        }
    }
    Ok(core.state_json())
}

#[tauri::command]
async fn set_user(core: State<'_, Shared>, name: String) -> Reply {
    core.inner().configure(&json!({ "user": name }))?;
    Ok(core.state_json())
}

/// First launch: the primary user's name and the language she thinks in. Only while pending.
#[tauri::command]
async fn first_run(core: State<'_, Shared>, name: String, language: String) -> Reply {
    core.inner().first_run(&name, &language)?;
    Ok(core.state_json())
}

/// "Pensar aquí": this computer asks to be her brain.
#[tauri::command]
async fn think_here(core: State<'_, Shared>) -> Reply {
    core.inner().think_here()?;
    Ok(core.net_json())
}

#[tauri::command]
async fn resolve_proposal(core: State<'_, Shared>, accept: bool) -> Reply {
    core.inner().resolve_proposal(accept)?;
    Ok(core.net_json())
}

/// Which computer's screen is to the left / right of this one.
#[tauri::command]
async fn set_neighbor(core: State<'_, Shared>, side: String, name: String) -> Reply {
    {
        let mut l = core.local.lock().unwrap_or_else(|e| e.into_inner());
        match side.as_str() {
            "izquierda" => l.vecinos.izquierda = name,
            "derecha" => l.vecinos.derecha = name,
            _ => return Err(lang::Ui::ErrUnknownSide.say(&[])),
        }
        l.save(&core.data);
        let mut s = core.net.status();
        s.left = l.vecinos.izquierda.clone();
        s.right = l.vecinos.derecha.clone();
    }
    Ok(core.net_json())
}

/// The chat window in screen captures (to share or screenshot it) or out of them.
#[tauri::command]
async fn set_chat_capture(core: State<'_, Shared>, visible: bool) -> Reply {
    core.set_chat_capturable(visible);
    Ok(core.state_json())
}

/// «Modelos → Motor»: the engine, its state, the VRAM and the engines found on this computer.
#[tauri::command]
async fn engine_state(core: State<'_, Shared>) -> Reply {
    Ok(core.engine_json())
}

#[tauri::command]
async fn engine_restart(core: State<'_, Shared>) -> Reply {
    core.inner().restart_model()?;
    Ok(core.engine_json())
}

/// Another engine, context size or GPU layers: written to local.json and, on the brain, applied
/// at once (with the previous settings back if it does not start).
#[tauri::command]
async fn engine_set(core: State<'_, Shared>, servidor: Option<String>, ctx: Option<u32>, gpu_auto: Option<bool>) -> Reply {
    let previous = core.local.lock().unwrap_or_else(|e| e.into_inner()).clone();
    {
        let mut l = core.local.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = servidor {
            if !std::path::Path::new(&s).is_file() {
                return Err(lang::Ui::ErrNoFile.say(&[&s]));
            }
            l.llama.servidor = s;
        }
        if let Some(c) = ctx {
            l.llama.args = motor::with_ctx(&l.llama.args, c.clamp(1024, 32768));
        }
        if let Some(a) = gpu_auto {
            l.llama.args = motor::with_gpu_auto(&l.llama.args, a);
        }
        l.save(&core.data);
    }
    core.inner().reload_model(previous);
    Ok(core.engine_json())
}

/// A companion's voice (the lead's without `who`): on or off, which one, how fast.
#[tauri::command]
async fn set_voice(crew: State<'_, Arc<life::Crew>>, who: Option<String>, active: Option<bool>, id: Option<String>, speed: Option<f32>) -> Reply {
    let core = crew.get(who.as_deref());
    core.set_voice(active, id, speed);
    Ok(core.voice_json())
}

/// Each companion's voice, for Modelos → Voz.
#[tauri::command]
async fn voices(crew: State<'_, Arc<life::Crew>>) -> Reply {
    Ok(json!(crew.all.iter().map(|c| json!({ "id": c.who.id, "name": c.who.name, "color": c.who.color, "voz": c.voice_json() })).collect::<Vec<_>>()))
}

/// A sentence in her voice, to hear how it sounds (even with the voice off).
#[tauri::command]
async fn voice_test(crew: State<'_, Arc<life::Crew>>, who: Option<String>) -> Reply {
    let core = crew.get(who.as_deref());
    let s = core.voice_setup()?;
    let user = core.mind().user.clone();
    let line = if s.idioma == "en" { "Doctor. This is how I sound now. If you would rather hear another voice, it is under Models.".to_string() } else { format!("Hola, {user}. Así sueno ahora. Si prefieres otra voz, está en Modelos.") };
    voice::say(s, &line, core.who.id, true);
    Ok(core.voice_json())
}

/// Dictation, push-to-talk: the microphone opens when the button of the chat is pressed.
#[tauri::command]
async fn mic_start() -> Result<(), String> {
    // Fail before recording if there is no ear to listen with.
    asr::setup(&catalog::Manifest::load(), "es")?;
    asr::start_recording()
}

/// Dictation: the button was released. Closes the microphone and returns what was said, in the
/// language of the app, for the box of the chat (it is not sent): with the fine ear, cleaned
/// and as turns of whoever spoke (her own voice from the speakers left out); without it, one
/// turn of nobody.
#[tauri::command]
async fn mic_stop(core: State<'_, Shared>) -> Reply {
    let samples = asr::stop_recording();
    // Under a third of a second is a click, not speech.
    if samples.len() < (asr::RATE / 3) as usize {
        return Ok(json!([]));
    }
    let lang = if lang::ui() == lang::Lang::En { "en" } else { "es" };
    let m = catalog::Manifest::load();
    let ear = asr::setup(&m, lang)?;
    let fine = hearing::setup(&m);
    let data = core.data.clone();
    let turns = tauri::async_runtime::spawn_blocking(move || hearing::listen(&ear, fine.as_ref(), &data, &samples)).await.map_err(|e| e.to_string())??;
    Ok(json!(turns))
}

/// The open microphone ("Auto"): on, she listens all the time and each thing said is sent by
/// itself when the speaker pauses.
#[tauri::command]
async fn open_mic(core: State<'_, Shared>, on: bool) -> Reply {
    if on {
        openmic::start(core.inner().clone())?;
    } else {
        openmic::stop();
    }
    let on = openmic::is_on();
    core.emit("k:openmic", json!({ "on": on, "oyendo": false }));
    Ok(json!({ "on": on }))
}

/// Someone said whose voice it is (the chip of a message heard by voice).
#[tauri::command]
async fn voice_name(core: State<'_, Shared>, id: u32, name: String) -> Reply {
    let mut v = hearing::Voices::load(&core.data);
    v.name(id, &name)?;
    v.save(&core.data)?;
    // Merged into another voice of that name: its clips go with it.
    if v.get(id).is_none() {
        hearing::drop_clips(&core.data, id);
    }
    Ok(json!(v.get(id).map(|x| x.nombre.clone()).unwrap_or_else(|| name.trim().to_string())))
}

/// The days a person has a log of what they worked on (`activity`), with the active seconds.
#[tauri::command]
async fn activity_days(core: State<'_, Shared>, person: String) -> Reply {
    Ok(json!(activity::days(&core.common, &person)))
}

/// The shared agenda (everyone's upcoming plans), the board of notes and the companions it
/// knows of (here or on a linked computer), for Memoria → Comunal and Personas.
#[tauri::command]
async fn common_bank(core: State<'_, Shared>) -> Reply {
    let root = core.common.parent().unwrap_or(&core.common).to_path_buf();
    let elsewhere: Vec<Value> = core.net.everyone().into_iter().map(|p| json!({ "equipo": p.name, "crew": p.status.crew })).collect();
    Ok(json!({ "agenda": common::agenda(&root), "notas": common::notes(&root), "equipos": elsewhere }))
}

/// Who lives on this computer (the selector's choice) and who could.
#[tauri::command]
async fn team(crew: State<'_, Arc<life::Crew>>) -> Reply {
    let data = crew.lead().common.parent().map(std::path::Path::to_path_buf).unwrap_or_else(|| "data".into());
    let t = Team::load(&data);
    Ok(json!({
        "personajes": character::ALL.iter().map(|c| json!({ "id": c.id, "name": c.name, "color": c.color })).collect::<Vec<_>>(),
        "aqui": crew.all.iter().map(|c| c.who.id).collect::<Vec<_>>(),
        "elegidos": if t.personajes.is_empty() { crew.all.iter().map(|c| c.who.id.to_string()).collect() } else { t.personajes },
        "recordar": t.recordar,
    }))
}

/// Another choice of who lives here: saved, and the app starts again with them.
#[tauri::command]
async fn set_team(app: tauri::AppHandle, crew: State<'_, Arc<life::Crew>>, ids: Vec<String>, remember: bool) -> Reply {
    let ids: Vec<String> = ids.iter().filter_map(|id| character::get(id)).map(|c| c.id.to_string()).collect();
    if ids.is_empty() {
        return Err(lang::tr("elige al menos una", "choose at least one"));
    }
    let data = crew.lead().common.parent().map(std::path::Path::to_path_buf).unwrap_or_else(|| "data".into());
    Team { personajes: ids, recordar: remember }.save(&data);
    for c in &crew.all {
        c.persist();
    }
    app.restart();
}

/// One day of a person's log: its sessions in order, and the time per program and window.
#[tauri::command]
async fn activity_day(core: State<'_, Shared>, person: String, day: String) -> Reply {
    let sessions = activity::day(&core.common, &person, &day);
    let apps: Vec<Value> = activity::by_app(&sessions).into_iter().map(|(app, secs, titles)| json!({ "app": app, "secs": secs, "titles": titles })).collect();
    Ok(json!({ "sessions": sessions, "apps": apps }))
}

/// A clip of a voice she does not know, to play it in the chat: a WAV data URI.
#[tauri::command]
async fn voice_clip(core: State<'_, Shared>, clip: String) -> Reply {
    use base64::Engine;
    let path = hearing::clip_path(&core.data, &clip).ok_or_else(|| lang::tr("ese clip ya no existe", "that clip no longer exists"))?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    Ok(json!(format!("data:audio/wav;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

/// Someone heard a clip she could not tell and said whose voice it is: registered from it.
#[tauri::command]
async fn voice_name_clip(core: State<'_, Shared>, clip: String, name: String) -> Reply {
    let fine = hearing::setup(&catalog::Manifest::load()).ok_or_else(|| lang::tr("falta el oído fino (Modelos → Oído)", "the fine ear is missing (Models → Ear)"))?;
    let data = core.data.clone();
    let id = tauri::async_runtime::spawn_blocking(move || hearing::enroll_clip(&fine, &data, &clip, &name)).await.map_err(|e| e.to_string())??;
    let v = hearing::Voices::load(&core.data);
    Ok(json!({ "id": id, "nombre": v.get(id).map(|x| x.nombre.clone()) }))
}

/// «Red → Vincular un equipo»: open (or close) the two minutes in which this computer offers
/// itself to the other one.
#[tauri::command]
async fn pairing(core: State<'_, Shared>, open: bool) -> Reply {
    if open {
        core.net.open_pairing();
    } else {
        core.net.close_pairing();
    }
    core.sync_links();
    Ok(core.net_json())
}

/// Ask a computer offering itself to link: both screens then show the same code.
#[tauri::command]
async fn link_request(core: State<'_, Shared>, id: String) -> Reply {
    core.net.request_link(&id)?;
    Ok(core.net_json())
}

/// The user compared the codes: accept or refuse.
#[tauri::command]
async fn link_answer(core: State<'_, Shared>, accept: bool) -> Reply {
    core.net.answer_link(accept)?;
    core.sync_links();
    Ok(core.net_json())
}

#[tauri::command]
async fn unlink(core: State<'_, Shared>, id: String) -> Reply {
    core.net.unlink(&id);
    core.sync_links();
    Ok(core.net_json())
}

/// Let the link work on networks Windows marks as public.
#[tauri::command]
async fn set_public_networks(core: State<'_, Shared>, allow: bool) -> Reply {
    core.net.allow_public.store(allow, std::sync::atomic::Ordering::Release);
    {
        let mut l = core.local.lock().unwrap_or_else(|e| e.into_inner());
        l.redes_publicas = allow;
        l.save(&core.data);
    }
    Ok(core.net_json())
}

/// «Modelos»: this computer and the catalog as it fits it (or the profile asked for).
#[tauri::command]
async fn models_state(core: State<'_, Shared>, profile: Option<String>) -> Reply {
    Ok(models::state(&core, profile))
}

#[tauri::command]
async fn model_download(core: State<'_, Shared>, ids: Vec<String>) -> Reply {
    models::download(core.inner().clone(), ids)?;
    Ok(Value::Null)
}

#[tauri::command]
async fn model_use(core: State<'_, Shared>, id: String, profile: Option<String>) -> Reply {
    models::use_model(core.inner(), &id)?;
    Ok(models::state(&core, profile))
}

/// «Modelos → Mis modelos»: a brain model of the user's, by link or from a file on disk.
#[tauri::command]
async fn model_add(core: State<'_, Shared>, name: String, modelo: String, mmproj: String, ctx: Option<u32>, profile: Option<String>) -> Reply {
    let id = models::add_own(&name, &modelo, &mmproj, ctx)?;
    let mut v = models::state(&core, profile);
    v["agregado"] = json!(id);
    Ok(v)
}

#[tauri::command]
async fn model_remove(core: State<'_, Shared>, id: String, delete_files: bool, profile: Option<String>) -> Reply {
    models::remove_own(&core, &id, delete_files)?;
    Ok(models::state(&core, profile))
}

/// A companion's Spine 3.8 chibi (Kal'tsit's `003_kalts`, Lappland's `140_whitew`), downloaded
/// on demand from `Ark-Models` into `modelos/spine/<id>/`.
#[tauri::command]
async fn chibi_model(crew: State<'_, Arc<life::Crew>>, who: Option<String>) -> Reply {
    models::chibi_model(&crew.get(who.as_deref()).who.spine)
}

/// Her body on the desktop: which one ("spine" or "pixel"), her Spine model's folder, and the
/// pose she is resting in now (a window that loads, or reloads with another body, starts there:
/// asleep if she is resting).
#[tauri::command]
async fn body_of(crew: State<'_, Arc<life::Crew>>, who: Option<String>) -> Reply {
    let core = crew.get(who.as_deref());
    let rest = core.body.lock().unwrap_or_else(|e| e.into_inner()).rest;
    Ok(json!({ "cuerpo": core.body(), "spine": core.who.spine.id, "pose": rest }))
}

/// Every companion's body, for the selector in Modelos.
#[tauri::command]
async fn bodies(crew: State<'_, Arc<life::Crew>>) -> Reply {
    Ok(json!(crew.all.iter().map(|c| json!({ "id": c.who.id, "name": c.who.name, "cuerpo": c.body() })).collect::<Vec<_>>()))
}

/// Changes her body ("spine" or "pixel"); her window on the desktop reloads with it.
#[tauri::command]
async fn set_body(crew: State<'_, Arc<life::Crew>>, who: String, cuerpo: String) -> Reply {
    if cuerpo != "spine" && cuerpo != "pixel" {
        return Err(lang::tr("cuerpo desconocido", "unknown body"));
    }
    let core = crew.get(Some(&who));
    {
        let mut l = core.local.lock().unwrap_or_else(|e| e.into_inner());
        l.cuerpo = cuerpo.clone();
        l.save(&core.data);
    }
    core.emit("k:cuerpo", json!({ "cuerpo": cuerpo }));
    Ok(json!({ "cuerpo": core.body() }))
}

/// «Accesorios»: the catalog of the accessories repository and the Minecraft instances here.
#[tauri::command]
async fn accessories_state(core: State<'_, Shared>) -> Reply {
    Ok(accessories::state(&core))
}

#[tauri::command]
async fn accessory_install(core: State<'_, Shared>, id: String, mods: String) -> Reply {
    accessories::install(core.inner().clone(), id, mods)?;
    Ok(Value::Null)
}

#[tauri::command]
async fn accessory_uninstall(core: State<'_, Shared>, id: String, mods: String) -> Reply {
    accessories::uninstall(&core, &id, &mods)?;
    Ok(accessories::state(&core))
}

#[tauri::command]
async fn accessory_fabric_api(core: State<'_, Shared>, mods: String, version: String) -> Reply {
    accessories::install_fabric_api(core.inner().clone(), mods, version)?;
    Ok(Value::Null)
}

#[tauri::command]
async fn accessories_config(core: State<'_, Shared>, repo: Option<String>, rama: Option<String>, token: Option<String>, add: Option<String>, remove: Option<String>) -> Reply {
    accessories::configure(&core, repo, rama, token, add, remove)?;
    Ok(accessories::state(&core))
}

/// «Plano»: the blueprint Minecraft is showing (or building) and whether it is connected.
#[tauri::command]
async fn minecraft_plan() -> Reply {
    Ok(minecraft_bridge::plan_state())
}

/// The «Plano» buttons: build it, discard it, move or turn it, pause or cancel the build.
#[tauri::command]
async fn minecraft_order(order: Value) -> Reply {
    minecraft_bridge::plan_order(&order)?;
    Ok(Value::Null)
}

/// Opens a folder in the Explorer (a mods folder, the models folder).
#[tauri::command]
async fn open_folder(path: String) -> Reply {
    let p = std::path::Path::new(&path);
    let dir = if p.is_dir() { p } else { p.parent().filter(|d| d.is_dir()).ok_or_else(|| format!("no existe {path}"))? };
    std::process::Command::new("explorer").arg(dir).spawn().map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

fn arg(name: &str) -> Option<String> {
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == name {
            return it.next();
        }
    }
    None
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Who lives here: the selector's last choice, and whether to skip it next time.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct Team {
    personajes: Vec<String>,
    recordar: bool,
}

impl Team {
    fn path(data: &std::path::Path) -> std::path::PathBuf {
        data.join("comun").join("equipo.json")
    }

    fn load(data: &std::path::Path) -> Team {
        std::fs::read_to_string(Self::path(data)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self, data: &std::path::Path) {
        let p = Self::path(data);
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(data));
        let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

/// What `launch` needs while the selector waits for a choice.
struct Waiting {
    net: Arc<net::Net>,
    data: std::path::PathBuf,
    local: local::Local,
}

/// The selector: who can live here, who was chosen last time, and the language to ask in.
#[tauri::command]
async fn characters(waiting: State<'_, std::sync::Mutex<Option<Waiting>>>) -> Reply {
    let w = waiting.lock().unwrap_or_else(|e| e.into_inner());
    let data = w.as_ref().map(|w| w.data.clone()).unwrap_or_else(|| "data".into());
    let team = Team::load(&data);
    let chosen = if team.personajes.is_empty() { vec![character::KALTSIT.id.to_string()] } else { team.personajes };
    let lang = if lang::ui() == lang::Lang::En { "en" } else { "es" };
    Ok(json!({
        "personajes": character::ALL.iter().map(|c| {
            // Her body as chosen on this computer before, or her default.
            let chosen = local::Local::load(&c.data_dir(&data)).cuerpo;
            let body = if chosen == "spine" || chosen == "pixel" { chosen } else { c.body.to_string() };
            json!({ "id": c.id, "name": c.name, "color": c.color, "cuerpo": body })
        }).collect::<Vec<_>>(),
        "elegidos": chosen,
        "idioma": lang,
    }))
}

/// The selector's answer: start them, and remember the choice.
#[tauri::command]
async fn start_crew(app: tauri::AppHandle, waiting: State<'_, std::sync::Mutex<Option<Waiting>>>, ids: Vec<String>, remember: bool, bodies: Option<std::collections::HashMap<String, String>>) -> Reply {
    let chosen: Vec<&'static character::Character> = ids.iter().filter_map(|id| character::get(id)).collect();
    if chosen.is_empty() {
        return Err(lang::tr("elige al menos una", "choose at least one"));
    }
    let w = waiting.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or_else(|| lang::tr("ya están en marcha", "they are already running"))?;
    Team { personajes: chosen.iter().map(|c| c.id.to_string()).collect(), recordar: remember }.save(&w.data);
    // The body chosen for each, kept in her own settings (Kal'tsit's are this computer's).
    for c in &chosen {
        if let Some(b) = bodies.as_ref().and_then(|m| m.get(c.id)).filter(|b| *b == "spine" || *b == "pixel") {
            let dir = c.data_dir(&w.data);
            let mut l = if dir == w.data { w.local.clone() } else { local::Local::load(&dir) };
            l.cuerpo = b.clone();
            let _ = std::fs::create_dir_all(&dir);
            l.save(&dir);
        }
    }
    let w = Waiting { local: local::Local::load(&w.data), ..w };
    let handle = app.clone();
    // Windows are made on the main thread.
    app.run_on_main_thread(move || {
        if let Err(e) = launch(&handle, chosen, w.net, w.data, w.local) {
            eprintln!("{e}");
        }
        if let Some(sel) = handle.get_webview_window("selector") {
            let _ = sel.close();
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

/// Starts the chosen companions: a mind, a body and a window on the desktop each, one chat for
/// all. The first leads: the link with other computers, the roles, Minecraft and the computer's
/// activity go through her. Companions sharing a model server take turns at it (`llm::turn`).
fn launch(app: &tauri::AppHandle, chosen: Vec<&'static character::Character>, net: Arc<net::Net>, data: std::path::PathBuf, root_local: local::Local) -> Result<(), Box<dyn std::error::Error>> {
    let model = origin::served_model(&format!("http://127.0.0.1:{}", root_local.llama.puerto)).unwrap_or_else(|| "gemma-4-12b".into());
    common::adopt_old_activity(&data);
    let mut minds = Vec::new();
    for c in &chosen {
        let dir = c.data_dir(&data);
        // A companion's first run on this computer starts from its model settings.
        let local = if dir == data {
            root_local.clone()
        } else {
            if !dir.join("local.json").exists() {
                let mut l = local::Local::default();
                l.llama = root_local.llama.clone();
                l.nombre = root_local.nombre.clone();
                let _ = std::fs::create_dir_all(&dir);
                let _ = l.save(&dir);
            }
            local::Local::load(&dir)
        };
        let mind = mind::Mind::open_for(&dir, &model, c).map_err(|e| format!("{}: {e}", c.name))?;
        minds.push((dir, local, mind));
    }
    // Someone already told one of them who they are: the others need not ask again.
    if let Some((user, language)) = minds.iter().find(|(_, _, m)| !m.first_run_pending()).map(|(_, _, m)| (m.user.clone(), (if m.lang() == lang::Lang::En { "en" } else { "es" }).to_string())) {
        for (_, _, m) in minds.iter_mut().filter(|(_, _, m)| m.first_run_pending()) {
            if let Err(e) = m.first_run(&user, &language) {
                eprintln!("{e}");
            }
        }
    }
    let all: Vec<Arc<Core>> = minds.into_iter().map(|(dir, local, mind)| Arc::new(Core::new(mind, app.clone(), net.clone(), local, dir))).collect();
    let crew = Arc::new(life::Crew { all });
    let lead = crew.lead().clone();
    // The state is managed before any page exists, so their first calls always find it.
    app.manage(lead.clone());
    app.manage(crew.clone());
    // A mouth follows the voice of whoever is speaking, on this computer only (where it plays).
    let handle = app.clone();
    voice::on_play(move |who, level| {
        let _ = handle.emit("k:voz", json!({ "level": level, "who": who }));
    });
    // …and the fine ear learns each one's voice from what she says, to know it when the
    // microphone hears it back from the speakers.
    let c = crew.clone();
    voice::on_piece(move |who, samples, rate| {
        if let Some(fine) = hearing::setup(&catalog::Manifest::load()) {
            let (data, samples) = (c.get(Some(who)).data.clone(), samples.to_vec());
            std::thread::spawn(move || hearing::learn_own(&fine, &data, &samples, rate));
        }
    });
    let names: Vec<&str> = crew.all.iter().map(|c| c.who.name).collect();
    let chat = WebviewWindowBuilder::new(app, "chat", WebviewUrl::App("chat.html".into()))
        .title(format!("{} · {}", names.join(" · "), lead.net.name))
        .inner_size(520.0, 760.0)
        .min_inner_size(380.0, 460.0)
        .build()?;
    // Their eyes must not read their own chat: by default it stays out of every capture.
    if !lead.local.lock().unwrap_or_else(|e| e.into_inner()).chat_en_capturas {
        if let Ok(h) = chat.hwnd() {
            screen::exclude_from_capture(h.0 as isize);
        }
    }
    for core in &crew.all {
        let overlay = WebviewWindowBuilder::new(app, core.overlay_label(), WebviewUrl::App("overlay.html".into()))
            .title(lang::tr(format!("{} · escritorio", core.who.name), format!("{} · desktop", core.who.name)))
            .inner_size(380.0, 440.0)
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(false)
            .focused(false)
            .build()?;
        // She never takes a click: everything under her stays usable.
        let _ = overlay.set_ignore_cursor_events(true);
        // She does not appear in her own captures (nor in anyone's). KALTSIT_CAPTURABLE=1
        // keeps her visible to screenshots, for testing.
        if std::env::var("KALTSIT_CAPTURABLE").is_err() {
            if let Ok(h) = overlay.hwnd() {
                screen::exclude_from_capture(h.0 as isize);
            }
        }
        core.place_start();
        // Her voice loads in the background, so the first reply does not wait for it.
        if core.local.lock().unwrap_or_else(|e| e.into_inner()).voz.activa {
            if let Ok(s) = core.voice_setup() {
                voice::warm(s);
            }
        }
        life::spawn_body(core.clone());
        life::spawn_eyes(core.clone());
        life::spawn_mind_clock(core.clone());
    }
    // The link with the other computers, through the lead. Without it they still work alone. Her
    // brain, body and memory go only to computers where she leads too; the bank to all of them.
    lead.net.set_crew(crew.all.iter().map(|c| c.who.id.to_string()).collect());
    spawn_bank_sync(crew.clone());
    let c = lead.clone();
    if let Err(e) = lead.net.serve(move |m, p, from, v| c.handle(m, p, from, v)) {
        eprintln!("{e}");
    }
    if let Err(e) = lead.net.start_discovery() {
        eprintln!("{e}");
    }
    life::spawn_activity(lead.clone());
    life::spawn_outbox(lead.clone());
    life::spawn_roles(lead.clone());
    // The others think on this computer whenever it can.
    for core in crew.all.iter().skip(1) {
        core.think_alongside(&lead);
    }
    minecraft_bridge::spawn_minecraft_server(lead.clone());
    scripted(&lead);
    Ok(())
}

/// Every 20 s, the shared bank of each linked computer (whoever lives there) merges into this one.
fn spawn_bank_sync(crew: Arc<life::Crew>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(20));
        let lead = crew.lead();
        let root = lead.common.parent().unwrap_or(&lead.common).to_path_buf();
        let here: Vec<&str> = crew.all.iter().map(|c| c.who.id).collect();
        for p in lead.net.everyone() {
            let theirs = lead.net.get(&p.addr, "/comun", 10).ok().and_then(|v| serde_json::from_value::<common::Bank>(v).ok());
            if theirs.is_some_and(|b| common::absorb(&root, &b, &here)) {
                lead.emit("k:comun", json!({}));
            }
        }
    });
}

/// For scripted tests: ask for the brain, walk into the right-hand neighbour, or say something
/// first, after some seconds (KALTSIT_ACEPTAR_RELEVO=1 on the other side accepts on its own).
fn scripted(core: &Arc<Core>) {
    if let Some(secs) = arg("--pensar-aqui-en").and_then(|s| s.parse::<u64>().ok()) {
        let c = core.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            if let Err(e) = c.think_here() {
                eprintln!("{e}");
            }
        });
    }
    if let Some(secs) = arg("--cruzar-en").and_then(|s| s.parse::<u64>().ok()) {
        let c = core.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            c.cross();
        });
    }
    if let Some(t) = arg("--decir") {
        let c = core.clone();
        std::thread::spawn(move || {
            for _ in 0..300 {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let ready = match c.role() {
                    life::Role::Brain => c.motor.healthy(),
                    life::Role::Terminal(_) => true,
                    life::Role::Alone => false,
                };
                if ready {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let _ = c.hear(t, None);
                    return;
                }
            }
        });
    }
}

fn main() {
    // `--semilla <archivo>`: where the owner's personal seed is (see `cthg::seed::personal_seed`).
    // Made absolute before the working directory moves.
    if let Some(p) = arg("--semilla") {
        cthg::seed::set_personal_override(std::path::absolute(&p).unwrap_or_else(|_| p.into()));
    }
    // Dynamic root anchoring: ensures the working directory is always the application's root directory
    // regardless of where or how the executable is launched (e.g. from Desktop, target/release, etc.),
    // so data, models, and cache are always created inside the project root and never spill outside.
    if let Ok(exe) = std::env::current_exe() {
        let mut root = exe.parent();
        if let Some(p) = root {
            if p.file_name().map_or(false, |n| n == "release" || n == "debug") {
                if let Some(target) = p.parent() {
                    if target.file_name().map_or(false, |n| n == "target") {
                        root = target.parent();
                    }
                }
            }
        }
        if let Some(r) = root {
            let _ = std::env::set_current_dir(r);
        }
    }

    // `iniciar.cmd` starts her from the project folder: the memory lives in its `data\`.
    let data = arg("--data").map(std::path::PathBuf::from).unwrap_or_else(|| "data".into());
    // `--migrar-sectores`: an explicit order, never automatic. Migrates the memory of `data` to
    // sectors (with a full backup) and exits; no window, and nothing else is opened first.
    if std::env::args().any(|a| a == "--migrar-sectores") {
        std::process::exit(migrate::command(&data));
    }
    // This computer's settings (its name, ports, link) live in `data`, Kal'tsit's folder; each
    // companion's own voice and model are in her own folder's local.json.
    let local = local::Local::load(&data);
    // `--volcar`: print Kal'tsit's memory modules (with origins) as JSON and exit.
    if std::env::args().any(|a| a == "--volcar") {
        let model = origin::served_model(&format!("http://127.0.0.1:{}", local.llama.puerto)).unwrap_or_else(|| "gemma-4-12b".into());
        match mind::Mind::open(&data, &model) {
            Ok(m) => println!("{}", serde_json::to_string_pretty(&m.memory_json()).unwrap_or_default()),
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    // Stable per computer (and per port, so two copies on one computer can be told apart).
    let here = origin::here("");
    let id = format!("{}-{:08x}", here.host, fnv(&format!("{}|{}", here.mac, local.puerto)) as u32);
    let net = net::Net::new(id, local.nombre.clone(), local.puerto, local.descubrimiento, &local.identidad, &local.vinculados, local.pares.clone());
    net.allow_public.store(local.redes_publicas, std::sync::atomic::Ordering::Release);
    {
        let mut s = net.status();
        s.think = local.can_think();
        s.left = local.vecinos.izquierda.clone();
        s.right = local.vecinos.derecha.clone();
        s.body = true;
    }

    tauri::Builder::default()
        .setup(move |app| {
            // Who lives here this time: `--personajes kaltsit,lappland`, or the choice remembered
            // (data/comun/equipo.json), or the selector asks (always with `--elegir`).
            let team = Team::load(&data);
            let given: Option<Vec<String>> = arg("--personajes").map(|s| s.split(',').map(|x| x.trim().to_string()).collect());
            let ids = given.or_else(|| (team.recordar && !std::env::args().any(|a| a == "--elegir")).then(|| team.personajes.clone()));
            let chosen: Vec<&'static character::Character> = ids.iter().flatten().filter_map(|id| character::get(id)).collect();
            if !chosen.is_empty() {
                launch(app.handle(), chosen, net.clone(), data.clone(), local.clone())?;
                return Ok(());
            }
            app.manage(std::sync::Mutex::new(Some(Waiting { net: net.clone(), data: data.clone(), local: local.clone() })));
            WebviewWindowBuilder::new(app, "selector", WebviewUrl::App("selector.html".into()))
                .title("Kal'tsit")
                .inner_size(440.0, 380.0)
                .resizable(false)
                .build()?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // The selector closed without a choice: nobody to start, the app ends.
            if let (WindowEvent::Destroyed, "selector") = (event, window.label()) {
                if window.app_handle().try_state::<Arc<life::Crew>>().is_none() {
                    window.app_handle().exit(0);
                }
            }
            // Closing the chat closes them all on this computer: memories written, models stopped.
            if let (WindowEvent::CloseRequested { .. }, "chat") = (event, window.label()) {
                if let Some(crew) = window.app_handle().try_state::<Arc<life::Crew>>() {
                    for core in &crew.all {
                        if core.role() == life::Role::Brain {
                            let _ = core.mind().save();
                        }
                        core.motor.stop();
                    }
                }
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![history, state, memory, memory_graph, open_viewer, red, send, type_chat, look_now, set_config, set_user, first_run, think_here, resolve_proposal, set_neighbor, pairing, link_request, link_answer, unlink, set_public_networks, set_chat_capture, engine_state, engine_restart, engine_set, set_voice, voices, voice_test, mic_start, mic_stop, voice_name, voice_clip, voice_name_clip, open_mic, activity_days, activity_day, common_bank, team, set_team, characters, start_crew, body_of, bodies, set_body, models_state, model_download, model_use, model_add, model_remove, chibi_model, accessories_state, accessory_install, accessory_uninstall, accessory_fabric_api, accessories_config, open_folder, minecraft_plan, minecraft_order])
        .run(tauri::generate_context!())
        .expect("no se pudo iniciar Kal'tsit");
}
