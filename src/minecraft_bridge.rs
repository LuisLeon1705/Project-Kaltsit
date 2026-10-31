//! IPC bridge to Minecraft Fabric mod (kaltsit-companion) on 127.0.0.1:25599.
//!
//! Feeds telemetry tick-by-tick into `instinct::evaluate_instinct`, transmits
//! chat messages between Minecraft and Kal'tsit's mind, detects command intents,
//! and triggers speech.

use crate::instinct::{self, Telemetry};
use crate::lang::Lang;
use crate::life::Core;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub const MINECRAFT_PORT: u16 = 25599;

static CHAT_OUTBOX: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static COMMAND_OUTBOX: OnceLock<Mutex<Vec<Value>>> = OnceLock::new();
static CONNECTED: AtomicBool = AtomicBool::new(false);
static LAST_CHAT_WAS_DOCTOR: AtomicBool = AtomicBool::new(true);

pub fn last_speaker_is_doctor() -> bool {
    LAST_CHAT_WAS_DOCTOR.load(Ordering::Acquire)
}

static SESSION_START: AtomicU64 = AtomicU64::new(0);

pub fn session_start() -> u64 {
    SESSION_START.load(Ordering::Acquire)
}

pub fn mark_session_start() {
    let now = crate::clock::unix_now().floor() as u64;
    SESSION_START.store(now, Ordering::Release);
}
static MINECRAFT_STATE: OnceLock<Mutex<String>> = OnceLock::new();

fn command_outbox() -> &'static Mutex<Vec<Value>> {
    COMMAND_OUTBOX.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn broadcast_command(val: &Value) {
    if CONNECTED.load(Ordering::Acquire) {
        if let Ok(mut lock) = command_outbox().lock() {
            lock.push(val.clone());
        }
    }
}

/// The words of a message, lower case, without punctuation (accents kept: «sígueme» = «sigueme»
/// is handled by `plain`).
fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(plain).collect()
}

/// Without accents, so «quédate» and «quedate» are the same word.
fn plain(w: &str) -> String {
    w.chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            other => other,
        })
        .collect()
}

/// A quantity in an order: «15 de hierro», «un stack», «diez troncos».
pub fn parse_quantity(text: &str) -> Option<u32> {
    let w = words(text);
    for word in &w {
        if let Ok(n) = word.parse::<u32>() {
            if (1..=1000).contains(&n) {
                return Some(n);
            }
        }
    }
    let joined = format!(" {} ", w.join(" "));
    if joined.contains(" medio stack ") {
        return Some(32);
    }
    const NUMBERS: [(&str, u32); 14] = [
        ("stack", 64), ("sesenta", 60), ("cincuenta", 50), ("treinta", 30), ("veinte", 20), ("quince", 15), ("diez", 10),
        ("ocho", 8), ("seis", 6), ("cinco", 5), ("cuatro", 4), ("tres", 3), ("dos", 2), ("un", 1),
    ];
    NUMBERS.iter().find(|(n, _)| w.iter().any(|x| x == n || (*n == "un" && (x == "una" || x == "uno")))).map(|(_, v)| *v)
}

pub fn extract_target_material(text: &str) -> Option<String> {
    let w = words(text);
    let has = |options: &[&str]| w.iter().any(|x| options.iter().any(|o| x.starts_with(o)));
    let id = if has(&["hierro", "iron"]) {
        "iron_ore"
    } else if has(&["carbon", "coal"]) {
        "coal_ore"
    } else if has(&["cobre", "copper"]) {
        "copper_ore"
    } else if has(&["diamante", "diamond"]) {
        "diamond_ore"
    } else if has(&["oro", "gold"]) && !has(&["oroba"]) {
        "gold_ore"
    } else if has(&["redstone"]) {
        "redstone_ore"
    } else if has(&["lapiz", "lapislazuli", "lapis"]) {
        "lapis_ore"
    } else if has(&["pizarra", "deepslate"]) {
        "deepslate"
    } else if has(&["adoquin", "cobble"]) {
        "cobblestone"
    } else if has(&["piedra", "stone", "roca"]) {
        "stone"
    } else if has(&["roble", "oak"]) {
        "oak_log"
    } else if has(&["abedul", "birch"]) {
        "birch_log"
    } else if has(&["abeto", "spruce"]) {
        "spruce_log"
    } else if has(&["madera", "arbol", "tronco", "log"]) {
        "oak_log"
    } else if has(&["tierra", "dirt"]) {
        "dirt"
    } else if has(&["arena", "sand"]) {
        "sand"
    } else if has(&["amatista", "amethyst"]) {
        "amethyst_block"
    } else if has(&["terracota", "terracotta"]) {
        "terracotta"
    } else {
        return None;
    };
    Some(format!("minecraft:{id}"))
}

/// Names a model writes for "just talking": not a body action, so the block is taken out of the
/// text but nothing reaches the game.
const NOT_ACTIONS: &[&str] = &["respond", "reply", "answer", "talk", "speak", "say", "chat", "none", "nothing", "idle", "wait", "think"];

/// What she says, without any action block: what is shown, read aloud and kept in her history
/// (a block left in the history is copied by the model into every reply after it).
pub fn spoken(text: &str) -> String {
    extract_action_json(text).0
}

/// The action block her model wrote in its reply ({"action": …}), taken out of the text.
pub fn extract_action_json(text: &str) -> (String, Option<Value>) {
    let (clean, action) = extract_any_action_json(text);
    let real = action.filter(|v| {
        let name = v["action"].as_str().unwrap_or("").trim().to_lowercase();
        !name.is_empty() && !NOT_ACTIONS.contains(&name.as_str())
    });
    (clean, real)
}

fn extract_any_action_json(text: &str) -> (String, Option<Value>) {
    let mut clean = text.to_string();
    let mut action_val = None;
    if let (Some(start), Some(end)) = (clean.find('{'), clean.rfind('}')) {
        if end > start {
            if let Ok(val) = serde_json::from_str::<Value>(&clean[start..=end]) {
                if val.get("action").is_some() {
                    action_val = Some(val);
                    let mut cut_start = start;
                    let mut cut_end = end + 1;
                    if let Some(fence_pos) = clean[..start].rfind("```") {
                        cut_start = fence_pos;
                    }
                    if let Some(fence_pos) = clean[cut_end..].find("```") {
                        cut_end += fence_pos + 3;
                    }
                    clean.replace_range(cut_start..cut_end, "");
                }
            }
        }
    }
    (clean.trim().to_string(), action_val)
}

/// Only the action block her model wrote in its reply; the Doctor's words never become an
/// order on their own.
pub fn decided_action(reply: &str) -> (String, Option<Value>) {
    extract_action_json(reply)
}

/// Puente de acciones protegido en código por la máquina de estados `AgencyState`:
/// extrae y limpia cualquier JSON de la respuesta verbal, pero en `CriticalSuspension`
/// descarta toda acción (`None`) diga lo que diga el modelo.
pub fn decided_action_gated(
    reply: &str,
    agency: crate::homeostasis::AgencyState,
) -> (String, Option<Value>) {
    let (clean, action) = extract_action_json(reply);
    if agency.actions_allowed() {
        (clean, action)
    } else {
        (clean, None)
    }
}

/// A hint for her turn context when the Doctor's message looks like a request she can carry
/// out with an action block. She decides whether to include it or not.
pub fn order_hint(user: &str, text: &str, lang: Lang) -> Option<String> {
    let act = infer_order(text)?;
    Some(crate::lang::order_hint(lang, user, &act.to_string()))
}

/// A likely request in the Doctor's words, read by whole words to give her model a hint of the
/// JSON block it may choose to emit (never executed on its own).
pub fn infer_order(text: &str) -> Option<Value> {
    let w = words(text);
    if w.is_empty() {
        return None;
    }
    let joined = format!(" {} ", w.join(" "));
    let phrase = |p: &str| joined.contains(&format!(" {p} "));
    let word = |options: &[&str]| w.iter().any(|x| options.contains(&x.as_str()));
    let prefix = |options: &[&str]| w.iter().any(|x| options.iter().any(|o| x.starts_with(o)));
    let here = word(&["aqui", "aca", "zona", "lugar", "sitio", "area", "alrededor"]);
    let qty = parse_quantity(text);
    let mat = extract_target_material(text);

    // The blueprint on show: move it (by as many blocks as said), turn it, discard it, accept it.
    let moving = word(&["muevela", "muevelo", "mueve", "desplazala", "correla", "corre", "pasala", "subela", "bajala", "acercala", "alejala", "mas"]);
    if moving {
        let n = qty.unwrap_or(1) as i64;
        let right = if word(&["izquierda"]) { -n } else if word(&["derecha"]) { n } else { 0 };
        let up = if word(&["arriba", "subela"]) { n } else if word(&["abajo", "bajala"]) { -n } else { 0 };
        let forward = if word(&["adelante", "delante", "alejala", "lejos", "alla"]) { n } else if word(&["atras", "acercala", "cerca", "aca"]) { -n } else { 0 };
        if right != 0 || up != 0 || forward != 0 {
            return Some(json!({ "action": "preview_move", "right": right, "up": up, "forward": forward }));
        }
    }
    if word(&["girala", "giralo", "rotala", "rotalo"]) || ((word(&["gira", "rota"]) || phrase("dale la vuelta") || phrase("media vuelta")) && word(&["plano", "casa", "la", "lo", "vuelta"])) {
        let turns = if phrase("media vuelta") || phrase("dale la vuelta") { 2 } else if word(&["izquierda"]) { 3 } else { 1 };
        return Some(json!({ "action": "preview_rotate", "turns": turns }));
    }
    if word(&["descartala", "descartalo", "descarta", "borrala", "quitala"]) || phrase("no la construyas") || phrase("quita el plano") {
        return Some(json!({ "action": "build_discard" }));
    }
    // Short and plain: «constrúyela», «sí, hazla», «adelante». «Hazla más grande» is a change: her model answers it.
    if w.len() <= 4 && word(&["construyela", "construyelo", "hazla", "levantala", "confirma", "confirmo", "adelante", "aprobado"]) {
        return Some(json!({ "action": "build_confirm" }));
    }
    if phrase("tu cama") && word(&["esta", "esa", "aqui", "es"]) && !prefix(&["pon", "coloc"]) {
        return Some(json!({ "action": "set_bed" }));
    }

    // Building: control of a build under way first.
    let about_build = prefix(&["constru", "obra", "edific"]);
    if about_build && word(&["cancela", "cancelala", "cancelar", "olvida", "abandona"]) || phrase("deja de construir") || phrase("para de construir") {
        return Some(json!({ "action": "cancel_build" }));
    }
    if about_build && word(&["pausa", "pausala", "pausar", "deten", "detente"]) {
        return Some(json!({ "action": "pause_build" }));
    }
    if about_build && word(&["sigue", "continua", "continuar", "reanuda", "retoma", "termina"]) {
        return Some(json!({ "action": "resume_build" }));
    }
    if phrase("que sabes construir") || phrase("que puedes construir") || (word(&["estructuras", "plantillas", "planos"]) && word(&["que", "cuales", "lista", "tienes", "sabes"])) {
        return Some(json!({ "action": "list_structures" }));
    }
    const BUILDABLE: [&str; 17] = [
        "casa", "casita", "cabana", "refugio", "choza", "torre", "atalaya", "muro", "muralla", "pared", "puente", "corral", "cerca", "valla", "plataforma", "estructura", "edificio",
    ];
    if (prefix(&["constru", "levant", "edifica"]) || word(&["haz", "hazme", "haznos", "monta"])) && (word(&BUILDABLE) || prefix(&["constru"])) {
        return Some(json!({ "action": "build_structure", "name": text.trim() }));
    }

    // Torches: zones and her own initiative.
    let torches = prefix(&["antorcha"]);
    let negated = word(&["no", "sin", "nunca", "deja", "dejes", "basta"]);
    if torches && negated && !phrase("ya puedes") {
        return Some(if here { json!({ "action": "no_torch_zone", "radius": qty.unwrap_or(24) }) } else { json!({ "action": "auto_torches", "enabled": false }) });
    }
    if torches && (phrase("ya puedes") || phrase("vuelve a") || word(&["automaticamente", "solas", "cuenta"])) {
        return Some(if here { json!({ "action": "allow_torches_here" }) } else { json!({ "action": "auto_torches", "enabled": true }) });
    }
    if torches && prefix(&["pon", "coloc", "ilumin"]) {
        return Some(json!({ "action": "place_torch", "count": qty.unwrap_or(1) }));
    }

    // Guarding, staying, following.
    if word(&["vigila", "vigilar", "guardia", "custodia"]) || phrase("protege esta zona") {
        return Some(json!({ "action": "guard" }));
    }
    if phrase("deja de seguirme") || phrase("deja de seguir") || phrase("no me sigas") || phrase("dejes de seguirme") || phrase("quedate ahi") || phrase("quedate aqui") || word(&["quedate", "quieta", "espera", "esperame", "alto"]) || (w.len() <= 2 && word(&["para", "stop"])) {
        return Some(json!({ "action": "stay" }));
    }
    if word(&["sigueme", "seguirme", "ven", "vente", "acercate", "vamos", "follow"]) || phrase("vuelve conmigo") {
        return Some(json!({ "action": "follow" }));
    }

    if word(&["cama"]) && prefix(&["pon", "coloc", "ubica", "instala"]) {
        return Some(json!({ "action": "place_bed" }));
    }
    if word(&["mesa"]) && (prefix(&["crafteo", "trabajo"]) || prefix(&["pon", "coloc"])) {
        return Some(json!({ "action": "place_crafting_table" }));
    }
    if word(&["cofre", "cofres", "chest", "baul"]) && prefix(&["guarda", "mete", "deposita", "almacena"]) {
        return Some(json!({ "action": "store_in_chest" }));
    }
    if word(&["cofre", "cofres", "chest", "baul"]) && prefix(&["saca", "coge", "toma", "trae"]) {
        return Some(json!({ "action": "take_from_chest", "count": qty.unwrap_or(16), "block": mat.clone().unwrap_or_else(|| "minecraft:oak_log".into()) }));
    }
    if prefix(&["tala", "corta"]) && (prefix(&["madera", "arbol", "tronco", "roble", "abedul", "abeto"]) || word(&["tala", "talar"])) {
        return Some(json!({ "action": "chop_wood", "count": qty.unwrap_or(0), "block": mat.clone().unwrap_or_else(|| "minecraft:oak_log".into()) }));
    }
    if word(&["mina", "minar", "pica", "picar", "extrae", "extraer", "excava"]) {
        return Some(json!({ "action": "mine", "count": qty.unwrap_or(1), "block": mat.unwrap_or_else(|| "minecraft:stone".into()) }));
    }
    if word(&["ataca", "atacalo", "atacala", "mata", "matalo", "defiendeme", "pelea"]) {
        return Some(json!({ "action": "attack" }));
    }
    None
}

pub fn get_minecraft_state() -> String {
    MINECRAFT_STATE.get_or_init(|| Mutex::new("FOLLOWING".to_string())).lock().unwrap().clone()
}

pub fn set_minecraft_state(s: &str) {
    if let Ok(mut lock) = MINECRAFT_STATE.get_or_init(|| Mutex::new("FOLLOWING".to_string())).lock() {
        *lock = s.to_string();
    }
}

fn outbox() -> &'static Mutex<Vec<String>> {
    CHAT_OUTBOX.get_or_init(|| Mutex::new(Vec::new()))
}

/// Broadcasts a chat message from Kal'tsit into Minecraft.
pub fn broadcast_chat(text: &str) {
    if CONNECTED.load(Ordering::Acquire) {
        if let Ok(mut lock) = outbox().lock() {
            lock.push(text.to_string());
        }
    }
}

pub fn is_connected() -> bool {
    CONNECTED.load(Ordering::Acquire)
}

/// The last blueprint the mod showed (preview) and how its build goes, for the «Plano» tab.
static PLAN: OnceLock<Mutex<Value>> = OnceLock::new();

fn set_plan(key: &str, v: Value) {
    if let Ok(mut p) = PLAN.get_or_init(|| Mutex::new(json!({}))).lock() {
        p[key] = v;
    }
}

/// The «Plano» tab: the blueprint, the build's progress and whether Minecraft is connected.
pub fn plan_state() -> Value {
    let mut v = PLAN.get_or_init(|| Mutex::new(json!({}))).lock().map(|p| p.clone()).unwrap_or_else(|_| json!({}));
    v["connected"] = json!(is_connected());
    v
}

/// Orders the app may give the mod straight from the «Plano» tab.
pub fn plan_order(order: &Value) -> Result<(), String> {
    const ALLOWED: [&str; 8] = ["build_confirm", "build_discard", "preview_move", "preview_rotate", "pause_build", "resume_build", "cancel_build", "build_status"];
    let action = order["action"].as_str().unwrap_or_default();
    if !ALLOWED.contains(&action) {
        return Err(crate::lang::tr(format!("orden no permitida desde la app: {action}"), format!("command not allowed from the app: {action}")));
    }
    if !is_connected() {
        return Err(crate::lang::tr("Minecraft no está conectado", "Minecraft is not connected"));
    }
    broadcast_command(order);
    Ok(())
}

static CAPABILITIES: OnceLock<Mutex<(Vec<String>, Vec<String>)>> = OnceLock::new();

/// What she can build in Minecraft (templates, structures saved in that world), as the mod
/// reported it: her prompt names them.
pub fn buildables() -> (Vec<String>, Vec<String>) {
    CAPABILITIES.get_or_init(Default::default).lock().map(|c| c.clone()).unwrap_or_default()
}

pub fn spawn_minecraft_server(core: Arc<Core>) {
    thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", MINECRAFT_PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[MinecraftBridge] No se pudo escuchar en el puerto {MINECRAFT_PORT}: {e}");
                return;
            }
        };

        if cfg!(debug_assertions) {
            eprintln!("[MinecraftBridge] Servidor IPC iniciado en 127.0.0.1:{MINECRAFT_PORT}");
        }

        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
            let core = core.clone();
            handle_client(stream, core);
        }
    });
}

fn handle_client(mut stream: TcpStream, core: Arc<Core>) {
    if cfg!(debug_assertions) {
        eprintln!("[MinecraftBridge] Minecraft Companion Mod conectado!");
    }
    CONNECTED.store(true, Ordering::Release);
    mark_session_start();
    // Automatically enable minecraft mode to mute screen capture
    let _ = core.configure(&json!({ "minecraft": true }));
    let user = core.mind().user.clone();
    let link_msg = json!({
        "type": "link_identity",
        "doctor": user,
        "minecraft_username": user
    });
    let _ = writeln!(stream, "{}", link_msg);
    let _ = stream.flush();

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });

    // The bytes of the line on its way: a long one (a blueprint) can arrive in pieces across
    // read timeouts, and nothing of it may be lost in between.
    let mut pending: Vec<u8> = Vec::new();
    while CONNECTED.load(Ordering::Acquire) {
        match reader.read_until(b'\n', &mut pending) {
            Ok(0) => break, // EOF
            Ok(_) if !pending.ends_with(b"\n") => {}
            Ok(_) => {
                let line = String::from_utf8_lossy(&std::mem::take(&mut pending)).into_owned();
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    let msg_type = val["type"].as_str().unwrap_or_default();
                    match msg_type {
                        "telemetry" => {
                            if let Some(st) = val["state"].as_str() {
                                set_minecraft_state(st);
                            }
                            let t = Telemetry {
                                health: val["health"].as_f64().unwrap_or(20.0) as f32,
                                food: val["food"].as_f64().unwrap_or(20.0) as f32,
                                oxygen_ratio: val["oxygen_ratio"].as_f64().unwrap_or(1.0) as f32,
                                in_water: val["in_water"].as_bool().unwrap_or(false),
                                in_lava: val["in_lava"].as_bool().unwrap_or(false),
                                on_fire: val["on_fire"].as_bool().unwrap_or(false),
                                player_distance: val["player_distance"].as_f64().unwrap_or(0.0) as f32,
                                ..Default::default()
                            };
                            let action = instinct::evaluate_instinct(&t);
                            let (tension, rise, afecto) = {
                                let h = &core.mind().hormones;
                                (
                                    (h.get(crate::hormones::H::Adrenalina) + h.get(crate::hormones::H::Cortisol)) / 2.0,
                                    h.get(crate::hormones::H::Dopamina),
                                    h.get(crate::hormones::H::Oxitocina)
                                )
                            };
                            let resp = json!({
                                "type": "action",
                                "action": action.name(),
                                "tension": tension,
                                "rise": rise,
                                "afecto": afecto
                            });
                            let _ = writeln!(stream, "{}", resp);
                            let _ = stream.flush();
                        }
                        "state_change" => {
                            let to = val["to"].as_str().unwrap_or("FOLLOWING");
                            let from = val["from"].as_str().unwrap_or("");
                            let reason = val["reason"].as_str().unwrap_or("");
                            set_minecraft_state(to);
                            core.emit("k:minecraft_state", json!({
                                "state": to,
                                "from": from,
                                "reason": reason
                            }));
                            if to == "COMBAT" || to == "ALERT" {
                                core.mind().hormones.feel(crate::hormones::Stimulus::Read { tension: 0.7, rise: 0.7, afecto: 0.0 });
                            }
                        }
                        "blueprint" => {
                            set_plan("blueprint", val.clone());
                            set_plan("progress", Value::Null);
                            core.emit("k:plano", plan_state());
                        }
                        "blueprint_clear" => {
                            set_plan("blueprint", Value::Null);
                            core.emit("k:plano", plan_state());
                        }
                        "build_progress" => {
                            set_plan("progress", val.clone());
                            core.emit("k:plano", plan_state());
                        }
                        "capabilities" => {
                            let list = |k: &str| val[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
                            if let Ok(mut c) = CAPABILITIES.get_or_init(Default::default).lock() {
                                *c = (list("templates"), list("structures"));
                            }
                        }
                        "session_start" => {
                            mark_session_start();
                            if cfg!(debug_assertions) {
                                eprintln!("[MinecraftBridge] Nueva sesión de Minecraft iniciada en timestamp {}", session_start());
                            }
                        }
                        "chat" => {
                            let sender = val["sender"].as_str().unwrap_or("Doctor");
                            let text = val["text"].as_str().unwrap_or("");
                            if !text.is_empty() && sender != "Kal'tsit" && sender != "Kaltsit" {
                                let is_doctor = val["is_doctor"].as_bool().unwrap_or(false)
                                    || sender.eq_ignore_ascii_case("doctor")
                                    || sender.eq_ignore_ascii_case(&core.mind().user);
                                LAST_CHAT_WAS_DOCTOR.store(is_doctor, Ordering::Release);
                                let prompt = if is_doctor {
                                    text.to_string()
                                } else {
                                    format!("{sender}: {text}")
                                };
                                let _ = core.hear(prompt, None);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout is normal with short read timeout
            }
            Err(_) => break,
        }

        // Flush command outbox to Minecraft
        let pending_cmds = {
            if let Ok(mut lock) = command_outbox().lock() {
                std::mem::take(&mut *lock)
            } else {
                Vec::new()
            }
        };
        for cmd in pending_cmds {
            let mut obj = cmd.clone();
            if obj.get("type").is_none() {
                if let Some(map) = obj.as_object_mut() {
                    map.insert("type".to_string(), json!("command"));
                }
            }
            let _ = writeln!(stream, "{}", obj);
            let _ = stream.flush();
        }

        // Flush chat outbox to Minecraft
        let pending = {
            if let Ok(mut lock) = outbox().lock() {
                std::mem::take(&mut *lock)
            } else {
                Vec::new()
            }
        };
        for msg in pending {
            let resp = json!({
                "type": "chat",
                "text": msg
            });
            let _ = writeln!(stream, "{}", resp);
            let _ = stream.flush();
        }
    }

    CONNECTED.store(false, Ordering::Release);
    core.emit("k:plano", plan_state());
    let _ = core.configure(&json!({ "minecraft": false }));
    if cfg!(debug_assertions) {
        eprintln!("[MinecraftBridge] Minecraft desconectado.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(text: &str) -> Option<String> {
        infer_order(text).map(|v| v["action"].as_str().unwrap_or_default().to_string())
    }

    #[test]
    fn orders_are_read_by_whole_words_only() {
        assert_eq!(action("Kaltsit, tala madera por favor").as_deref(), Some("chop_wood"));
        assert_eq!(action("Pon una cama").as_deref(), Some("place_bed"));
        assert_eq!(action("pon una mesa de crafteo").as_deref(), Some("place_crafting_table"));
        assert_eq!(action("pon antorcha aqui").as_deref(), Some("place_torch"));
        assert_eq!(action("sígueme").as_deref(), Some("follow"));
        assert_eq!(action("quédate aquí").as_deref(), Some("stay"));
        assert_eq!(action("deja de seguirme").as_deref(), Some("stay"));
        assert_eq!(action("vigila la entrada").as_deref(), Some("guard"));
        assert_eq!(action("ataca al zombi").as_deref(), Some("attack"));
        assert_eq!(action("mina 15 de hierro").as_deref(), Some("mine"));
        assert_eq!(infer_order("mina 15 de hierro").unwrap()["count"], 15);
        // Words inside other words are not orders.
        assert_eq!(action("hola Kal'tsit, qué opinas de este lugar?"), None);
        assert_eq!(action("esto es para la ventana del salón"), None);
        assert_eq!(action("todos los días miro la ventana"), None);
        assert_eq!(parse_quantity("todos los días"), None);
        assert_eq!(parse_quantity("tala dos árboles"), Some(2));
    }

    #[test]
    fn torches_building_and_its_control() {
        assert_eq!(infer_order("no pongas antorchas aquí").unwrap()["action"], "no_torch_zone");
        assert_eq!(infer_order("no pongas antorchas en 40 bloques alrededor").unwrap()["radius"], 40);
        assert_eq!(infer_order("deja de poner antorchas").unwrap(), json!({ "action": "auto_torches", "enabled": false }));
        assert_eq!(action("ya puedes poner antorchas aquí").as_deref(), Some("allow_torches_here"));
        assert_eq!(infer_order("vuelve a poner antorchas").unwrap(), json!({ "action": "auto_torches", "enabled": true }));
        assert_eq!(action("constrúyeme una casa de 7x9 de piedra").as_deref(), Some("build_structure"));
        assert_eq!(action("haz una torre de 12").as_deref(), Some("build_structure"));
        assert_eq!(action("construye mi_casa").as_deref(), Some("build_structure"));
        assert_eq!(action("pausa la construcción").as_deref(), Some("pause_build"));
        assert_eq!(action("sigue construyendo").as_deref(), Some("resume_build"));
        assert_eq!(action("cancela la obra").as_deref(), Some("cancel_build"));
        assert_eq!(action("¿qué sabes construir?").as_deref(), Some("list_structures"));
    }

    #[test]
    fn the_blueprint_is_moved_turned_and_accepted_in_words() {
        assert_eq!(infer_order("muévela 3 bloques a la izquierda").unwrap(), json!({ "action": "preview_move", "right": -3, "up": 0, "forward": 0 }));
        assert_eq!(infer_order("más a la derecha").unwrap(), json!({ "action": "preview_move", "right": 1, "up": 0, "forward": 0 }));
        assert_eq!(infer_order("súbela dos").unwrap(), json!({ "action": "preview_move", "right": 0, "up": 2, "forward": 0 }));
        assert_eq!(infer_order("muévela 5 hacia atrás").unwrap()["forward"], -5);
        assert_eq!(infer_order("gírala").unwrap(), json!({ "action": "preview_rotate", "turns": 1 }));
        assert_eq!(infer_order("dale la vuelta a la casa").unwrap()["turns"], 2);
        assert_eq!(action("constrúyela").as_deref(), Some("build_confirm"));
        assert_eq!(action("sí, adelante").as_deref(), Some("build_confirm"));
        assert_eq!(action("descártala").as_deref(), Some("build_discard"));
        assert_eq!(action("esta es tu cama").as_deref(), Some("set_bed"));
        assert_eq!(action("pon tu cama").as_deref(), Some("place_bed"));
        // A change of design goes to her model, which sends a new plan.
        assert_eq!(action("hazla más grande y con dos pisos, con ventanas grandes"), None);
    }

    #[test]
    fn the_action_block_is_taken_out_of_her_reply() {
        let (clean, act) = extract_action_json("Voy para allá.\n```json\n{\"action\": \"follow\"}\n```");
        assert_eq!(clean, "Voy para allá.");
        assert_eq!(act.unwrap()["action"], "follow");
        // Without a block, her words never become an order.
        assert_eq!(extract_action_json("Voy a ver qué hay para comer.").1, None);
    }

    #[test]
    fn a_talking_block_is_hidden_and_is_not_an_order() {
        let reply = "{\"action\": \"respond\"}\nExactamente. ¿Estás intentando optimizar alguna ejecución?";
        let (clean, act) = extract_action_json(reply);
        assert_eq!(clean, "Exactamente. ¿Estás intentando optimizar alguna ejecución?");
        assert_eq!(act, None, "«respond» is not something her body does");
        assert_eq!(spoken(reply), clean);
        assert_eq!(spoken("```json\n{\"action\": \"none\"}\n```\nNo."), "No.");
    }

    #[test]
    fn without_her_json_block_no_order_is_issued_and_his_words_are_only_a_hint() {
        let hint = order_hint("Ana", "Kaltsit, tala madera por favor", Lang::Es).unwrap();
        assert!(hint.contains("\"action\":\"chop_wood\"") && hint.contains("Decides tú si lo haces"));
        assert_eq!(order_hint("Ana", "hola Kal'tsit, ¿qué opinas de este lugar?", Lang::Es), None);
        let english = order_hint("Ana", "Kaltsit, tala madera por favor", Lang::En).unwrap();
        assert!(english.starts_with("Ana seems to be asking you: ") && english.contains("\"action\":\"chop_wood\"") && english.contains("You decide whether to do it"));

        // If she refuses or proposes something else without a JSON block, no action is emitted.
        let (clean, act) = decided_action("No. Está anocheciendo y primero conviene asegurar el refugio.");
        assert_eq!(clean, "No. Está anocheciendo y primero conviene asegurar el refugio.");
        assert_eq!(act, None);

        // Only when she includes the JSON block in her reply does an action come out.
        let (clean, act) = decided_action("Está bien, cortaré unos troncos.\n{\"action\": \"chop_wood\", \"count\": 10, \"block\": \"minecraft:oak_log\"}");
        assert_eq!(clean, "Está bien, cortaré unos troncos.");
        assert_eq!(act.unwrap()["action"], "chop_wood");
    }
}
