//! Her life across the computers of one network.
//!
//! One computer is her **brain**: it runs the model, holds the authoritative memory and
//! decides what she says and does. The others are **terminals**: they show the chat, lend their
//! screen and eyes, and keep a replica of her memory that the brain updates after every save.
//! With no brain reachable a computer is **alone**: it keeps the memory and shows her, but she
//! cannot think until one computer is chosen to (in the app, confirmed by the current brain).
//!
//! She exists once: her body is on one screen at a time. She goes to the screen where the user
//! is working, and while wandering she may walk off an edge into the neighbouring computer.

use crate::clock;
use crate::cthg::model::Role as Speaker;
use crate::hormones::Stimulus;
use crate::lang::{self, Ui};
use crate::llm::{Llm, Msg, SLOT_ANALYSIS, SLOT_VOICE};
use crate::local::{self, Local};
use crate::mind::{self, unix_now, Mind};
use crate::modules::Files;
use crate::motor::Motor;
use crate::net::Net;
use crate::screen;
use base64::Engine as _;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

/// Share of thumbnail cells that must change to count as "something new" (about 70 of 2304).
const CHANGE: f32 = 0.03;
/// Walking speed in logical pixels per second.
const SPEED: f64 = 85.0;
/// Width of the screenshot sent to the vision encoder.
const SHOT_W: u32 = 1280;
/// She follows the user when the screen she is on has been idle this long...
const AWAY_S: f64 = 60.0;
/// ...and someone is typing or moving the mouse on another one.
const ACTIVE_S: f64 = 5.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Brain,
    Terminal(String),
    Alone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn name(self) -> &'static str {
        match self {
            Side::Left => "izquierda",
            Side::Right => "derecha",
        }
    }
    fn parse(s: &str) -> Side {
        if s == "izquierda" { Side::Left } else { Side::Right }
    }
    fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

#[derive(Default)]
pub struct Eyes {
    /// Thumbnail of this screen the last time she looked at it.
    pub thumb: Vec<u8>,
    pub last_look: f64,
    pub seen: Option<String>,
    pub last_spoke: Option<f64>,
    pub lived: u32,
    pub last_reflect: f64,
}

pub struct Body {
    /// Left edge of the overlay window, physical pixels.
    pub x: f64,
    pub target: Option<f64>,
    pub rest: &'static str,
    pub idle_until: f64,
    /// Walking off this screen through that side.
    pub leaving: Option<Side>,
}

pub struct Core {
    pub mind: Mutex<Mind>,
    pub llm: Llm,
    pub app: AppHandle,
    pub busy: AtomicBool,
    /// Unknown voices she was already told about this session: she asks once, not every time.
    told_unknown: Mutex<Vec<u64>>,
    pub eyes: Mutex<Eyes>,
    pub body: Mutex<Body>,
    pub sensor: Mutex<crate::sensor::Sensor>,
    pub said: AtomicU64,
    pub net: Arc<Net>,
    pub motor: Motor,
    pub local: Mutex<Local>,
    pub data: PathBuf,
    pub role: Mutex<Role>,
    /// Brain's view: the computer her body is on.
    pub body_on: Mutex<String>,
    /// Her body is on this screen.
    pub here: AtomicBool,
    pending_enter: Mutex<Option<(String, Side, f64)>>,
    outbox: Mutex<Vec<Value>>,
    linked: Mutex<HashSet<String>>,
    sent: Mutex<HashMap<String, HashMap<String, String>>>,
    /// Brain: a computer asking to be the brain (id, name).
    proposal: Mutex<Option<(String, String)>>,
    /// Terminal: the brain this computer asked to take over from.
    asked: Mutex<Option<String>>,
    /// Last work area seen: a hidden window cannot always tell which monitor it is on.
    last_area: Mutex<Option<(f64, f64, f64, f64, f64, f64)>>,
}

/// What happens to her body, for the console of a debug build.
fn trace(msg: &str) {
    if cfg!(debug_assertions) {
        let t = clock::now_local().map(|l| format!("{:02}:{:02}", l.hour, l.minute)).unwrap_or_default();
        eprintln!("[{t}] {msg}");
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Frees the voice when a call ends, however it ends.
struct Free<'a>(&'a AtomicBool);

impl Drop for Free<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn hash(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in b {
        h = (h ^ *x as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub fn files_to_json(files: &Files) -> Value {
    let b64 = base64::engine::general_purpose::STANDARD;
    Value::Object(files.iter().map(|(n, b)| (n.clone(), Value::String(b64.encode(b)))).collect())
}

pub fn files_from_json(v: &Value) -> Result<Files, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let obj = v.as_object().ok_or("memoria sin archivos")?;
    obj.iter().map(|(n, s)| Ok((n.clone(), b64.decode(s.as_str().unwrap_or("")).map_err(|e| format!("{n}: {e}"))?))).collect()
}

impl Core {
    pub fn new(mind: Mind, app: AppHandle, net: Arc<Net>, local: Local, data: PathBuf) -> Core {
        let motor = Motor::new(local.llama.clone());
        let llm = Llm { url: motor.url(), model: "local".into() };
        let me = net.id.clone();
        lang::set_ui(mind.lang());
        Core {
            mind: Mutex::new(mind),
            llm,
            app,
            busy: AtomicBool::new(false),
            told_unknown: Mutex::new(Vec::new()),
            eyes: Mutex::new(Eyes::default()),
            body: Mutex::new(Body { x: 0.0, target: None, rest: "Relax", idle_until: unix_now() + 20.0, leaving: None }),
            sensor: Mutex::new(crate::sensor::Sensor::default()),
            said: AtomicU64::new(0),
            net,
            motor,
            local: Mutex::new(local),
            data,
            role: Mutex::new(Role::Alone),
            body_on: Mutex::new(me),
            here: AtomicBool::new(true),
            pending_enter: Mutex::new(None),
            outbox: Mutex::new(Vec::new()),
            linked: Mutex::new(HashSet::new()),
            sent: Mutex::new(HashMap::new()),
            proposal: Mutex::new(None),
            asked: Mutex::new(None),
            last_area: Mutex::new(None),
        }
    }

    pub fn mind(&self) -> MutexGuard<'_, Mind> {
        lock(&self.mind)
    }

    pub fn type_chat(&self, text: &str) {
        let now = crate::clock::unix_now();
        lock(&self.sensor).observe(text, now);
    }

    pub fn role(&self) -> Role {
        lock(&self.role).clone()
    }

    fn is_brain(&self) -> bool {
        self.role() == Role::Brain
    }

    fn me(&self) -> &str {
        &self.net.id
    }

    fn brain_addr(&self) -> Option<String> {
        match self.role() {
            Role::Terminal(b) => self.net.peer(&b).map(|p| p.addr),
            _ => None,
        }
    }

    // ── events ──

    /// Shown on this computer and, from the brain, on every linked one.
    pub(crate) fn emit(&self, ev: &str, v: Value) {
        let _ = self.app.emit(ev, v.clone());
        if self.is_brain() && !lock(&self.linked).is_empty() {
            lock(&self.outbox).push(json!([ev, v]));
        }
    }

    fn emit_local(&self, ev: &str, v: Value) {
        let _ = self.app.emit(ev, v);
    }

    fn status(&self, s: &str) {
        self.emit("k:status", json!({ "text": s }));
    }

    pub fn state_json(&self) -> Value {
        let role = self.role();
        let m = self.mind();
        lang::set_ui(m.lang());
        json!({
            "user": m.user,
            "idioma": m.config.language,
            "primer_arranque": m.first_run_pending(),
            "config": m.config,
            "hormonas": m.hormones.json(),
            "ventana": m.hormones.window_tokens(),
            "ocupada": self.busy.load(Ordering::Acquire),
            "chat_en_capturas": lock(&self.local).chat_en_capturas,
            "voz": { "activa": lock(&self.local).voz.activa },
            "minecraft_state": crate::minecraft_bridge::get_minecraft_state(),
            "moral": m.moral_json(),
            "llm": match &role {
                Role::Brain => Ui::LlmHere.fill(m.lang(), &[&m.model()]),
                Role::Terminal(b) => Ui::LlmThinksOn.fill(m.lang(), &[&self.net.peer(b).map(|p| p.name).unwrap_or_else(|| Ui::LlmAnother.text(m.lang()).to_string())]),
                Role::Alone => Ui::LlmNone.text(m.lang()).into(),
            },
            "online": match role {
                Role::Brain => if m.config.reposo { false } else { self.motor.healthy() },
                Role::Terminal(_) => true,
                Role::Alone => false,
            },
        })
    }

    /// The network as the app shows it.
    pub fn net_json(&self) -> Value {
        let l = lock(&self.local).clone();
        let role = self.role();
        let proposal = lock(&self.proposal).clone();
        json!({
            "yo": {
                "id": self.me(), "nombre": self.net.name, "rol": match &role { Role::Brain => "cerebro", Role::Terminal(_) => "terminal", Role::Alone => "solo" },
                "puede_pensar": l.can_think(), "cuerpo": self.here.load(Ordering::Acquire), "huella": self.net.huella(),
                "puerto": l.puerto, "modelo": l.llama.modelo,
            },
            "vecinos": { "izquierda": l.vecinos.izquierda, "derecha": l.vecinos.derecha },
            "equipos": self.net.alive(),
            "propuesta": proposal.map(|(id, name)| json!({ "id": id, "nombre": name })),
            "esperando": lock(&self.asked).clone(),
            "vinculados": self.net.vinculos().iter().filter(|v| v.confirmado).map(|v| json!({
                "id": v.id, "nombre": v.nombre,
                "huella": crate::net::fingerprint_of(&v.publica),
                "en_linea": self.net.peer(&v.id).is_some(),
                "sesion": self.net.session_age(&v.id).map(|s| s.round()),
            })).collect::<Vec<_>>(),
            "vincular": {
                "abierto": self.net.pairing_open(),
                "quedan": self.net.pairing_left().round(),
                "candidatos": self.net.candidates(),
                "en_curso": self.net.linking(),
            },
            "redes": {
                "privadas": self.net.private_list(),
                "perfiles": self.net.profiles().into_iter().map(|(n, c)| json!({ "nombre": n, "tipo": c })).collect::<Vec<_>>(),
                "publicas_permitidas": l.redes_publicas,
            },
        })
    }

    /// The linked computers into local.json, when they changed.
    pub fn sync_links(&self) {
        if self.net.take_dirty() {
            let mut l = lock(&self.local);
            l.vinculados = self.net.vinculos();
            l.save(&self.data);
        }
    }

    fn net_changed(&self) {
        self.emit_local("k:red", self.net_json());
        self.emit_local("k:state", self.state_json());
    }

    // ── body ──

    fn overlay(&self) -> Option<WebviewWindow> {
        self.app.get_webview_window("overlay")
    }

    /// Work area of the primary monitor (she lives there on each computer) and the overlay's own
    /// size, physical pixels; the last area seen if the system does not answer. Asking the window
    /// for "its" monitor is not used: off an edge, it would be the neighbouring monitor.
    fn area(&self) -> Option<(f64, f64, f64, f64, f64, f64)> {
        let found = self.overlay().and_then(|w| {
            let mon = self.app.primary_monitor().ok().flatten().or_else(|| w.current_monitor().ok().flatten())?;
            let a = mon.work_area();
            let size = w.outer_size().ok().filter(|s| s.width > 0)?;
            Some((a.position.x as f64, a.position.y as f64, a.size.width as f64, a.size.height as f64, size.width as f64, size.height as f64))
        });
        match found {
            Some(a) => {
                *lock(&self.last_area) = Some(a);
                Some(a)
            }
            None => *lock(&self.last_area),
        }
    }

    /// A command for her body, wherever it is.
    fn body_cmd(&self, cmd: Value) {
        let on = lock(&self.body_on).clone();
        if on == self.me() {
            self.apply_body(&cmd);
        } else if let Some(p) = self.net.peer(&on) {
            let net = self.net.clone();
            std::thread::spawn(move || {
                let _ = net.post(&p.addr, "/cuerpo", &cmd, 5);
            });
        }
    }

    fn anim(&self, name: &str, looping: bool) {
        self.body_cmd(json!({ "c": "anim", "name": name, "loop": looping }));
    }

    /// Body commands on this screen.
    pub fn apply_body(&self, cmd: &Value) {
        match cmd["c"].as_str().unwrap_or("") {
            "anim" => self.emit_local("k:anim", json!({ "name": cmd["name"], "loop": cmd["loop"] })),
            "walk" => self.walk_local(cmd["x"].as_f64().unwrap_or(0.5)),
            "salir" => self.leave_local(Side::parse(cmd["lado"].as_str().unwrap_or(""))),
            "entrar" => self.enter_local(Side::parse(cmd["lado"].as_str().unwrap_or(""))),
            "ocultar" => self.set_here(false),
            "mostrar" => {
                self.set_here(true);
                self.place_start();
            }
            _ => {}
        }
    }

    fn set_here(&self, here: bool) {
        if self.here.load(Ordering::Acquire) != here {
            trace(if here { "cuerpo: aparece en esta pantalla" } else { "cuerpo: deja esta pantalla" });
        }
        self.here.store(here, Ordering::Release);
        self.net.status().body = here;
        if let Some(o) = self.overlay() {
            let _ = if here { o.show() } else { o.hide() };
            let _ = o.set_ignore_cursor_events(true);
        }
    }

    /// Walks so her centre ends at `frac` of this screen's width (0 left, 1 right).
    fn walk_local(&self, frac: f64) {
        let Some((ax, _, aw, _, ww, _)) = self.area() else { return };
        let target = (ax + frac.clamp(0.0, 1.0) * aw - ww / 2.0).clamp(ax, ax + aw - ww);
        let mut b = lock(&self.body);
        if b.leaving.is_some() || (target - b.x).abs() < 4.0 {
            return;
        }
        let left = target < b.x;
        b.target = Some(target);
        drop(b);
        self.emit_local("k:face", json!({ "left": left }));
        self.emit_local("k:anim", json!({ "name": "Move", "loop": true }));
    }

    fn leave_local(&self, side: Side) {
        trace(&format!("cuerpo: sale por la {}", side.name()));
        let Some((ax, _, aw, _, ww, _)) = self.area() else {
            // No idea where the edge is: she just goes.
            return self.left_screen();
        };
        let mut b = lock(&self.body);
        b.leaving = Some(side);
        // To the edge, and gone: walking further would spill onto a neighbouring monitor.
        b.target = Some(match side {
            Side::Left => ax,
            Side::Right => ax + aw - ww,
        });
        drop(b);
        self.emit_local("k:face", json!({ "left": side == Side::Left }));
        self.emit_local("k:anim", json!({ "name": "Move", "loop": true }));
    }

    fn enter_local(&self, side: Side) {
        trace(&format!("cuerpo: entra por la {}", side.name()));
        // Shown first: some systems only report a window's monitor once it is visible.
        self.set_here(true);
        let Some((ax, ay, aw, ah, ww, wh)) = self.area() else {
            self.place_start();
            return;
        };
        let (start, end) = match side {
            Side::Left => (ax, ax + 0.12 * aw),
            Side::Right => (ax + aw - ww, ax + aw - ww - 0.12 * aw),
        };
        {
            let mut b = lock(&self.body);
            b.x = start;
            b.target = Some(end);
            b.leaving = None;
            b.rest = "Relax";
        }
        if let Some(w) = self.overlay() {
            let _ = w.set_position(PhysicalPosition::new(start as i32, (ay + ah - wh + 4.0) as i32));
        }
        self.emit_local("k:face", json!({ "left": side == Side::Right }));
        self.emit_local("k:anim", json!({ "name": "Move", "loop": true }));
    }

    pub fn place_start(&self) {
        let Some((ax, ay, aw, ah, ww, wh)) = self.area() else { return };
        let x = ax + aw - ww - 40.0;
        lock(&self.body).x = x;
        if let Some(w) = self.overlay() {
            let _ = w.set_position(PhysicalPosition::new(x as i32, (ay + ah - wh + 4.0) as i32));
        }
    }

    /// She left this screen: the brain sends her into the next one.
    fn left_screen(&self) {
        self.set_here(false);
        if self.is_brain() {
            self.exited();
        } else if let Some(addr) = self.brain_addr() {
            let net = self.net.clone();
            std::thread::spawn(move || {
                let _ = net.post(&addr, "/salio", &json!({}), 5);
            });
        }
    }

    /// Brain: moves her body to computer `to`, leaving the current screen through `side`.
    fn transfer(&self, to: &str, side: Side) {
        let from = lock(&self.body_on).clone();
        if from == to || lock(&self.pending_enter).is_some() {
            return;
        }
        let gone = from != self.me() && self.net.peer(&from).is_none();
        trace(&format!("cerebro: la lleva de {from} a {to} por la {}", side.name()));
        *lock(&self.pending_enter) = Some((to.to_string(), side.opposite(), unix_now()));
        if gone {
            return self.exited();
        }
        self.body_cmd(json!({ "c": "salir", "lado": side.name() }));
    }

    fn exited(&self) {
        let Some((to, side, _)) = lock(&self.pending_enter).take() else { return };
        *lock(&self.body_on) = to;
        self.body_cmd(json!({ "c": "entrar", "lado": side.name() }));
        self.net_changed();
    }

    /// Body side: she wants to walk into the computer named `name` through `side`.
    fn request_move(&self, name: &str, side: Side) {
        let Some(p) = self.net.peer_by_name(name) else { return };
        if self.is_brain() {
            self.transfer(&p.id, side);
        } else if let Some(addr) = self.brain_addr() {
            let net = self.net.clone();
            let body = json!({ "to": p.id, "lado": side.name() });
            std::thread::spawn(move || {
                let _ = net.post(&addr, "/traslado", &body, 5);
            });
        }
    }

    /// For tests: she walks into a neighbouring computer now (right first).
    pub fn cross(&self) {
        let (left, right) = {
            let l = lock(&self.local);
            (l.vecinos.izquierda.clone(), l.vecinos.derecha.clone())
        };
        if !right.is_empty() {
            self.request_move(&right, Side::Right);
        } else if !left.is_empty() {
            self.request_move(&left, Side::Left);
        }
    }

    // ── memory across computers ──

    /// Saves (brain only) and sends what changed to every linked computer.
    fn persist(&self) {
        if !self.is_brain() {
            return;
        }
        let files = match self.mind().save() {
            Ok(f) => f,
            Err(e) => return self.emit("k:error", json!({ "text": Ui::ErrSaveMemory.say(&[&e.to_string()]) })),
        };
        let peers: Vec<String> = lock(&self.linked).iter().cloned().collect();
        for p in peers {
            self.replicate(&p, &files);
        }
    }

    fn replicate(&self, peer: &str, files: &Files) {
        let Some(p) = self.net.peer(peer) else { return };
        let changed: Files = {
            let sent = lock(&self.sent);
            let known = sent.get(peer);
            files.iter().filter(|(n, b)| known.and_then(|k| k.get(n)) != Some(&hash(b))).cloned().collect()
        };
        if changed.is_empty() {
            return;
        }
        let hashes: Vec<(String, String)> = changed.iter().map(|(n, b)| (n.clone(), hash(b))).collect();
        // Synchronous on purpose: replication is small and stays in the order of the saves.
        let body = json!({ "files": files_to_json(&changed) });
        if self.net.post(&p.addr, "/memoria", &body, 30).is_ok() {
            let mut sent = lock(&self.sent);
            let e = sent.entry(peer.to_string()).or_default();
            for (n, h) in hashes {
                e.insert(n, h);
            }
        }
    }

    /// Brain: a computer joined. Whatever it lived on its own is merged in, then it gets the
    /// whole memory back as its replica.
    fn link(&self, peer: &crate::net::Peer) {
        let theirs = self.net.get(&peer.addr, "/memoria", 30).and_then(|v| files_from_json(&v["files"]));
        if let Ok(files) = theirs {
            if self.take_voice(Duration::from_secs(60)) {
                let _free = Free(&self.busy);
                match self.mind().absorb(&files) {
                    Ok(n) if n > 0 => self.emit("k:log", json!({ "role": "sense", "text": Ui::LogRemembers.say(&[&n.to_string(), &peer.name]) })),
                    Ok(_) => {}
                    Err(e) => self.emit("k:error", json!({ "text": Ui::ErrMergeMemory.say(&[&peer.name, &e.to_string()]) })),
                }
            }
        }
        lock(&self.sent).remove(&peer.id);
        lock(&self.linked).insert(peer.id.clone());
        self.persist();
        let files = self.mind().files();
        self.replicate(&peer.id, &files);
        // Only one body: if hers is not there, that screen stays empty.
        if *lock(&self.body_on) != peer.id {
            let _ = self.net.post(&peer.addr, "/cuerpo", &json!({ "c": "ocultar" }), 5);
        }
        self.net_changed();
    }

    // ── roles ──

    /// This computer becomes her brain: its model starts and the others follow it.
    pub fn become_brain(&self, body_on: Option<String>) -> Result<(), String> {
        if !self.mind().config.reposo {
            self.status(&Ui::StWaking.say(&[]));
            if let Err(e) = self.motor.start(Duration::from_secs(240)) {
                self.status("");
                self.emit_local("k:error", json!({ "text": e }));
                return Err(e);
            }
            if let Some(name) = crate::origin::served_model(&self.llm.url) {
                self.mind().set_model(&name);
            }
        }
        *lock(&self.role) = Role::Brain;
        {
            let mut s = self.net.status();
            s.brain = true;
            s.brain_since = unix_now();
        }
        lock(&self.linked).clear();
        let body = body_on
            .or_else(|| self.here.load(Ordering::Acquire).then(|| self.me().to_string()))
            .or_else(|| self.net.alive().into_iter().find(|p| p.status.body).map(|p| p.id))
            .unwrap_or_else(|| self.me().to_string());
        if body == self.me() && !self.here.load(Ordering::Acquire) {
            self.set_here(true);
            self.place_start();
        }
        *lock(&self.body_on) = body;
        self.status("");
        self.net_changed();
        Ok(())
    }

    /// `local.json` changed its model (chosen in «Modelos»). The brain swaps it at once, after
    /// what she is saying; if the new one does not start, the previous one comes back.
    pub fn reload_model(self: &Arc<Self>, previous: Local) {
        let now = Local::load(&self.data);
        self.motor.set_cfg(now.llama.clone());
        self.net.status().think = now.can_think();
        *lock(&self.local) = now.clone();
        if !self.is_brain() {
            self.net_changed();
            return;
        }
        let core = self.clone();
        std::thread::spawn(move || {
            core.take_voice(Duration::from_secs(120));
            let _free = Free(&core.busy);
            let name = |l: &Local| Path::new(&l.llama.modelo).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            core.status(&Ui::StSwitching.say(&[&name(&now)]));
            core.motor.stop();
            if core.motor.healthy() {
                core.status("");
                return core.emit_local("k:error", json!({ "text": Ui::ErrPortTaken.say(&[]) }));
            }
            let result = core.motor.start(Duration::from_secs(240));
            if let Err(e) = result {
                core.emit_local("k:error", json!({ "text": Ui::ErrSwitchFailed.say(&[&name(&now), &e, &name(&previous)]) }));
                previous.save(&core.data);
                core.motor.set_cfg(previous.llama.clone());
                *lock(&core.local) = previous;
                if let Err(e) = core.motor.start(Duration::from_secs(240)) {
                    core.emit_local("k:error", json!({ "text": e }));
                }
            }
            if let Some(m) = crate::origin::served_model(&core.llm.url) {
                core.mind().set_model(&m);
            }
            core.status("");
            core.net_changed();
        });
    }

    /// The engine as «Modelos → Motor» shows it.
    pub fn engine_json(&self) -> Value {
        use crate::motor;
        let cfg = self.motor.cfg();
        let st = self.motor.status();
        let size = |p: &str| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let ctx = motor::ctx_of(&cfg.args);
        let estimate = crate::catalog::estimate_gb(size(&cfg.modelo) + if cfg.mmproj.is_empty() { 0 } else { size(&cfg.mmproj) }, ctx.unwrap_or(4096));
        let loaded = if st["responde"].as_bool().unwrap_or(false) { crate::origin::served_model(&self.llm.url) } else { None };
        let failed = !st["responde"].as_bool().unwrap_or(false) && !st["propio"].as_bool().unwrap_or(false);
        json!({
            "estado": st,
            "cerebro": self.is_brain(),
            "puede_pensar": lock(&self.local).can_think(),
            "servidor": cfg.servidor,
            "version": motor::engine_version(&cfg.servidor),
            "modelo": Path::new(&cfg.modelo).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
            "cargado": loaded,
            "ctx": ctx,
            "gpu_auto": motor::gpu_auto(&cfg.args),
            "estimado": estimate,
            "vram": crate::catalog::vram_now().map(|(u, t)| json!({ "usada": u, "total": t })),
            "motores": motor::engines(&cfg.servidor).into_iter().map(|(p, o)| json!({ "ruta": p, "origen": o, "version": motor::engine_version(&p) })).collect::<Vec<_>>(),
            "log": if failed { self.motor.log_tail(14) } else { Vec::new() },
        })
    }

    /// Restarts the model with the settings of `local.json` (only the brain runs one).
    pub fn restart_model(self: &Arc<Self>) -> Result<(), String> {
        if !self.is_brain() {
            return Err(Ui::ErrNotBrain.say(&[]));
        }
        let now = lock(&self.local).clone();
        self.reload_model(now);
        Ok(())
    }

    // ── voice ──

    /// The voice this computer speaks with: the one chosen, or the first one downloaded.
    pub fn voice_setup(&self) -> Result<crate::voice::Setup, String> {
        let v = lock(&self.local).voz.clone();
        let m = crate::catalog::Manifest::load();
        let id = if v.id.is_empty() {
            let root = m.root();
            let mut voices: Vec<&crate::catalog::Item> = m.elementos.iter().filter(|i| i.voz.is_some() && i.installed(&root)).collect();
            // Her own (cloned) voice first, then the light ones.
            voices.sort_by_key(|i| (i.voz.as_ref().is_none_or(|v| v.referencia.is_none()), i.nivel != "ligero"));
            voices.first().map(|i| i.id.clone()).ok_or("no hay ninguna voz descargada (Modelos → Voz)")?
        } else {
            v.id
        };
        crate::voice::setup(&m, &id, v.velocidad)
    }

    /// The language this computer's voice speaks, if it is on and ready.
    fn voice_language(&self) -> Option<String> {
        if !lock(&self.local).voz.activa {
            return None;
        }
        self.voice_setup().ok().map(|s| s.idioma)
    }

    /// Says it on this computer, if its voice is on and speaks `lang`; otherwise answers which
    /// language it needs (the brain then sends it translated).
    pub fn speak_here(&self, text: &str, lang: &str) -> Option<String> {
        let s = self.voice_setup().ok().filter(|_| lock(&self.local).voz.activa)?;
        if s.idioma != lang {
            return Some(s.idioma);
        }
        crate::voice::say(s, text);
        None
    }

    /// Her reply as she would say it out loud in English, for a voice that speaks English (her
    /// own, cloned from the game). On the analysis slot, so the chat's cached prompt stays.
    fn in_english(&self, text: &str) -> Option<String> {
        let messages = vec![
            Msg::text(
                "system",
                "You turn what Kal'tsit (Arknights) just said in Spanish into what she says out loud in English: \
the same meaning, in her voice from the game, dry and precise, never longer than the original. \
Leave out code, links, lists, asterisks, sighs or ellipses ('...'); never add anything. Answer only with JSON.",
            ),
            Msg::text("user", crate::mind::strip_actions(text)),
        ];
        let schema = json!({ "type": "object", "properties": { "en": { "type": "string" } }, "required": ["en"] });
        let v = self.llm.chat_json(&messages, &schema, SLOT_ANALYSIS, 0.2, 400).ok()?;
        v["en"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
    }

    /// Her reply out loud, where her body is, in the language of the voice there.
    fn voice_out(&self, text: &str) {
        let on = lock(&self.body_on).clone();
        if on == self.me() {
            match self.voice_language().as_deref() {
                Some("en") => {
                    if let Some(en) = self.in_english(text) {
                        self.speak_here(&en, "en");
                    }
                }
                Some(lang) => {
                    self.speak_here(text, lang);
                }
                None => {}
            }
            return;
        }
        let Some(p) = self.net.peer(&on) else { return };
        // That computer says it if its voice speaks Spanish, or answers which language it needs.
        let wants = self.net.post(&p.addr, "/voz", &json!({ "text": text, "idioma": "es" }), 10).ok().and_then(|v| v["idioma"].as_str().map(String::from));
        if wants.as_deref() == Some("en") {
            if let Some(en) = self.in_english(text) {
                let _ = self.net.post(&p.addr, "/voz", &json!({ "text": en, "idioma": "en" }), 10);
            }
        }
    }

    pub fn voice_json(&self) -> Value {
        let v = lock(&self.local).voz.clone();
        let setup = self.voice_setup();
        let m = crate::catalog::Manifest::load();
        let name = setup.as_ref().ok().and_then(|s| m.elementos.iter().find(|i| i.voz.as_ref().is_some_and(|z| m.root().join(&z.carpeta) == s.dir)).map(|i| (i.id.clone(), i.nombre.clone())));
        json!({
            "activa": v.activa,
            "velocidad": v.velocidad,
            "lista": setup.is_ok(),
            "id": name.as_ref().map(|n| n.0.clone()),
            "idioma": setup.as_ref().ok().map(|s| s.idioma.clone()),
            "nombre": name.map(|n| n.1),
            "falta": setup.err(),
            "error": crate::voice::last_error(),
        })
    }

    /// The voice settings of this computer.
    pub fn set_voice(&self, active: Option<bool>, id: Option<String>, speed: Option<f32>) {
        {
            let mut l = lock(&self.local);
            if let Some(a) = active {
                l.voz.activa = a;
            }
            if let Some(i) = id {
                l.voz.id = i;
            }
            if let Some(s) = speed {
                l.voz.velocidad = s.clamp(0.6, 1.6);
            }
            l.save(&self.data);
        }
        if !lock(&self.local).voz.activa {
            crate::voice::stop();
        } else if let Ok(s) = self.voice_setup() {
            crate::voice::warm(s);
        }
    }

    /// This computer follows `brain`: its own model stops, freeing the VRAM.
    fn become_terminal(&self, brain: &str, keep_body: bool) {
        *lock(&self.role) = Role::Terminal(brain.to_string());
        self.net.status().brain = false;
        self.motor.stop();
        lock(&self.linked).clear();
        *lock(&self.asked) = None;
        if !keep_body {
            self.set_here(false);
        }
        self.net_changed();
    }

    /// The user wants this computer to think. Alone: at once. Otherwise the brain must agree.
    pub fn think_here(self: &Arc<Self>) -> Result<(), String> {
        if !lock(&self.local).can_think() {
            return Err(Ui::ErrNoModel.say(&[]));
        }
        match self.role() {
            Role::Brain => Ok(()),
            Role::Alone => {
                let core = self.clone();
                std::thread::spawn(move || {
                    let _ = core.become_brain(None);
                });
                Ok(())
            }
            Role::Terminal(b) => {
                let p = self.net.peer(&b).ok_or_else(|| Ui::ErrBrainSilent.say(&[]))?;
                self.net.post(&p.addr, "/propuesta", &json!({ "nombre": self.net.name }), 5)?;
                *lock(&self.asked) = Some(p.name.clone());
                self.status(&Ui::StWaitingConfirm.say(&[&p.name]));
                self.net_changed();
                Ok(())
            }
        }
    }

    /// Brain: the user answered another computer's request to be the brain.
    pub fn resolve_proposal(self: &Arc<Self>, accept: bool) -> Result<(), String> {
        let (id, name) = lock(&self.proposal).take().ok_or_else(|| Ui::ErrNoProposal.say(&[]))?;
        let p = self.net.peer(&id).ok_or_else(|| Ui::ErrPeerGone.say(&[&name]))?;
        self.net_changed();
        if !accept {
            let _ = self.net.post(&p.addr, "/relevo", &json!({ "ok": false }), 5);
            return Ok(());
        }
        let core = self.clone();
        std::thread::spawn(move || {
            // Finish what she is saying, then hand over the whole memory.
            core.take_voice(Duration::from_secs(120));
            let _free = Free(&core.busy);
            core.status(&Ui::StHanding.say(&[&name]));
            let files = match core.mind().save() {
                Ok(f) => f,
                Err(e) => return core.emit("k:error", json!({ "text": e })),
            };
            let body_on = lock(&core.body_on).clone();
            let keep = body_on == core.me();
            let msg = json!({ "ok": true, "files": files_to_json(&files), "cuerpo": body_on });
            match core.net.post(&p.addr, "/relevo", &msg, 60) {
                Ok(_) => {
                    core.status("");
                    core.become_terminal(&p.id, keep);
                }
                Err(e) => core.emit("k:error", json!({ "text": Ui::ErrHandover.say(&[&e]) })),
            }
        });
        Ok(())
    }

    // ── voice ──

    fn take_voice(&self, wait: Duration) -> bool {
        let end = std::time::Instant::now() + wait;
        loop {
            if self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return true;
            }
            if std::time::Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn speak_stream(&self, prepared: &mind::Prepared, temperature: f32, max_tokens: u32) -> Result<String, String> {
        let id = self.said.fetch_add(1, Ordering::Relaxed);
        self.emit("k:start", json!({ "id": id }));
        self.anim("Interact", false);
        let raw = self.llm.chat_stream(&prepared.messages, temperature, max_tokens, |tok| {
            self.emit("k:tok", json!({ "id": id, "v": tok }));
            true
        })?;
        let reply = mind::clean_reply(&raw);
        // The bubble ends with what she says; an action block is for the game, never for the chat.
        self.emit("k:said", json!({ "id": id, "text": crate::minecraft_bridge::spoken(&reply) }));
        lock(&self.eyes).last_spoke = Some(unix_now());
        Ok(reply)
    }

    fn speak_line(&self, text: &str) {
        let id = self.said.fetch_add(1, Ordering::Relaxed);
        self.emit("k:start", json!({ "id": id, "alone": true }));
        self.anim("Interact", false);
        self.emit("k:said", json!({ "id": id, "text": text, "alone": true }));
        lock(&self.eyes).last_spoke = Some(unix_now());
    }

    /// Something the user wrote, on any computer.
    /// A message. `voz`: it was said out loud (the dictation's turns), not typed.
    pub fn hear(self: &Arc<Self>, text: String, voz: Option<Value>) -> Result<(), String> {
        let mut u = lock(&self.sensor).take_for(&text);
        u.voz = voz.filter(|v| v.as_array().is_some_and(|a| !a.is_empty()));
        self.hear_utterance(u)
    }

    pub fn hear_utterance(self: &Arc<Self>, u: crate::sensor::Utterance) -> Result<(), String> {
        crate::voice::stop();
        if self.mind().config.reposo {
            let _ = self.configure(&json!({ "reposo": false }));
        }
        match self.role() {
            Role::Brain => {
                let core = self.clone();
                std::thread::spawn(move || {
                    if !core.take_voice(Duration::from_secs(120)) {
                        return core.emit("k:error", json!({ "text": Ui::ErrStillBusy.say(&[]) }));
                    }
                    let _free = Free(&core.busy);
                    if let Err(e) = core.answer_with_ghosts(&u.said, &u.ghosts, u.voz.as_ref(), u.sonido.as_ref()) {
                        core.status("");
                        core.emit("k:error", json!({ "text": e }));
                    }
                });
                Ok(())
            }
            Role::Terminal(_) => {
                let addr = self.brain_addr().ok_or_else(|| Ui::ErrBrainSilent.say(&[]))?;
                self.net.post(&addr, "/oir", &json!({ "text": u.said, "ghosts": u.ghosts, "voz": u.voz, "sonido": u.sonido }), 10).map(|_| ())
            }
            Role::Alone => Err(Ui::ErrNobodyThinks.say(&[])),
        }
    }

    /// The screen where her body is, if it changed since she last looked at it.
    fn current_screen(&self) -> Option<String> {
        if !self.mind().config.vision || self.mind().config.minecraft {
            return None;
        }
        let on = lock(&self.body_on).clone();
        if on == self.me() {
            return self.capture_if_changed();
        }
        let p = self.net.peer(&on)?;
        self.net.get(&p.addr, "/ojos", 15).ok()?["jpeg"].as_str().map(String::from)
    }

    /// Her own windows (chat, memory viewer) as her eyes must skip them: where they are on the
    /// captured (primary) screen, if they are visible and captures can see them.
    fn own_rects(&self) -> Vec<(i32, i32, i32, i32)> {
        if !lock(&self.local).chat_en_capturas {
            return Vec::new();
        }
        ["chat", "visor"]
            .iter()
            .filter_map(|label| {
                let w = self.app.get_webview_window(label)?;
                if !w.is_visible().unwrap_or(false) || w.is_minimized().unwrap_or(false) {
                    return None;
                }
                let (p, s) = (w.outer_position().ok()?, w.outer_size().ok()?);
                Some((p.x, p.y, s.width as i32, s.height as i32))
            })
            .collect()
    }

    /// A capture of this screen without her own windows in it (she would read herself).
    pub fn shoot(&self) -> Option<screen::Shot> {
        let mut shot = screen::capture()?;
        for (x, y, w, h) in self.own_rects() {
            shot.mask(x, y, w, h);
        }
        Some(shot)
    }

    /// The chat window in screen captures or not (this computer only).
    pub fn set_chat_capturable(&self, capturable: bool) {
        {
            let mut l = lock(&self.local);
            l.chat_en_capturas = capturable;
            l.save(&self.data);
        }
        for label in ["chat", "visor"] {
            if let Some(h) = self.app.get_webview_window(label).and_then(|w| w.hwnd().ok()) {
                screen::set_capturable(h.0 as isize, capturable);
            }
        }
    }

    fn capture_if_changed(&self) -> Option<String> {
        let shot = self.shoot()?;
        let thumb = shot.thumb();
        if screen::change(&thumb, &lock(&self.eyes).thumb) < CHANGE {
            return None;
        }
        let jpeg = shot.jpeg_b64(SHOT_W);
        let mut e = lock(&self.eyes);
        e.thumb = thumb;
        e.last_look = unix_now();
        jpeg
    }

    #[allow(dead_code)]
    pub fn answer(&self, text: &str) -> Result<(), String> {
        self.answer_with_ghosts(text, &[], None, None)
    }

    fn answer_with_ghosts(&self, text: &str, ghosts: &[String], voz: Option<&Value>, sonido: Option<&Value>) -> Result<(), String> {
        if self.is_brain() && !self.motor.healthy() {
            self.status(&Ui::StWaking.say(&[]));
            if let Err(e) = self.motor.start(Duration::from_secs(240)) {
                return Err(Ui::ErrModelStart.say(&[&e]));
            }
        }
        {
            let mut m = self.mind();
            m.hormones.decay(crate::hormones::TURN_S);
            for g in ghosts {
                m.say(Speaker::Ghost, g, &[]);
                m.hormones.feel(Stimulus::Hesitation);
            }
            // Said out loud: a line of narration before it says so, and whose voice it was.
            if let Some(line) = voz.and_then(|v| crate::lang::heard_by_voice(m.lang(), v, &self.tell_unknown(v))) {
                // …and, against how that voice usually sounds, anything clearly off.
                let line = match sonido.and_then(|s| crate::lang::sounded_unusual(m.lang(), s)) {
                    Some(how) => format!("{line}; {how}"),
                    None => line,
                };
                m.say(Speaker::Narrator, &line, &[]);
            }
            m.say(Speaker::User, text, &[]);
        }
        for g in ghosts {
            self.emit("k:log", json!({ "role": "ghost", "text": g }));
        }
        self.emit("k:log", json!({ "role": "user", "text": text, "voz": voz }));
        self.status(&Ui::StReading.say(&[]));
        let ex = self.mind().extraction(text, &[]);
        let analysis = self.llm.chat_json(&ex.messages, &mind::extraction_schema_with(&ex.sectors), SLOT_ANALYSIS, 0.0, 300).ok();
        if let Some(v) = &analysis {
            let t = mind::tension_of(v);
            self.mind().hormones.feel(Stimulus::Read { tension: t, rise: t, afecto: mind::afecto_of(v) });
        }
        let image = self.current_screen();
        let seen = lock(&self.eyes).seen.clone();
        self.status(&Ui::StReplying.say(&[]));
        let (prepared, temperature, plasticity) = {
            let mut m = self.mind();

            let wt = m.hormones.window_tokens();
            let desc = m.mood();
            let p = m.prepare_reply(wt, &desc, seen.as_deref(), image.as_deref());
            let (t, w) = (m.hormones.temperature(), m.hormones.plasticity());
            m.hormones.feel(Stimulus::Understood(p.understood));
            (p, t, w)
        };
        let reply = self.speak_stream(&prepared, temperature, 500)?;
        // Only the action block she herself wrote in her reply (never inferred from his words).
        let (clean_reply, action) = crate::minecraft_bridge::decided_action_gated(&reply, self.mind().homeostasis.agency_state());
        // Out loud only when answering a message, and once the whole reply is written.
        self.voice_out(&clean_reply);
        crate::minecraft_bridge::broadcast_chat(&clean_reply);
        if let Some(act) = action {
            if crate::minecraft_bridge::last_speaker_is_doctor() {
                crate::minecraft_bridge::broadcast_command(&act);
            }
        }
        let recalled: Vec<u32> = prepared.recalled.iter().map(|r| r.0).collect();
        let events = {
            let mut m = self.mind();
            let ep = m.record_reply(text, &clean_reply, &recalled);
            let ev = analysis.map(|v| m.learn(&v, &ex.numbered, ep, plasticity, Some(text))).unwrap_or_default();
            for e in &ev {
                m.hormones.feel(match e.action {
                    "se cumple" => Stimulus::Fulfilled,
                    "nuevo" | "causa" => Stimulus::NewFact,
                    "refuerza" | "consolidado" => Stimulus::Reinforced,
                    _ => Stimulus::Contradicted,
                });
            }
            ev
        };
        self.persist();
        self.emit("k:learned", json!({ "events": events }));
        lock(&self.eyes).lived += 1;
        self.status("");
        self.emit("k:state", self.state_json());
        Ok(())
    }

    /// The unknown voices of `turns` she was already told about; the others are marked as told
    /// from now on.
    fn tell_unknown(&self, turns: &Value) -> Vec<u64> {
        let mut told = lock(&self.told_unknown);
        let before = told.clone();
        for t in turns.as_array().into_iter().flatten() {
            let id = t["voz"].as_u64().unwrap_or(0);
            if id != 0 && !t["conocida"].as_bool().unwrap_or(false) && !told.contains(&id) {
                told.push(id);
            }
        }
        before
    }

    // ── the open microphone ──

    /// The open microphone sent something: a whole turn (sent as if dictated and sent), or a
    /// piece of a monologue that is still going on.
    pub fn hear_open(self: &Arc<Self>, f: &crate::openmic::Flush) -> Result<(), String> {
        let voz = serde_json::to_value(&f.turns).ok().filter(|v| v.as_array().is_some_and(|a| !a.is_empty()));
        if !f.still_talking {
            let u = crate::sensor::Utterance { said: f.text.clone(), ghosts: Vec::new(), voz, sonido: Some(f.sound_json()) };
            return self.hear_utterance(u);
        }
        match self.role() {
            Role::Brain => {
                self.clone().monologue_here(f.text.clone(), voz, f.sound_json());
                Ok(())
            }
            Role::Terminal(_) => {
                let addr = self.brain_addr().ok_or_else(|| Ui::ErrBrainSilent.say(&[]))?;
                self.net.post(&addr, "/monologo", &json!({ "text": f.text, "voz": voz, "sonido": f.sound_json() }), 10).map(|_| ())
            }
            Role::Alone => Err(Ui::ErrNobodyThinks.say(&[])),
        }
    }

    /// A piece of a monologue, on the brain: it goes into her window (so nothing said is lost
    /// and no single turn outgrows the window), the model reads how charged it is, and she waits,
    /// says one short sentence, or cuts in. While she is still answering something, the piece
    /// is only kept: she reads it with the rest when she answers.
    pub fn monologue_here(self: Arc<Self>, text: String, voz: Option<Value>, sonido: Value) {
        std::thread::spawn(move || {
            {
                let mut m = self.mind();
                if let Some(line) = voz.as_ref().and_then(|v| crate::lang::heard_by_voice(m.lang(), v, &self.tell_unknown(v))) {
                    m.say(Speaker::Narrator, &line, &[]);
                }
                m.say(Speaker::User, &text, &[]);
            }
            self.emit("k:log", json!({ "role": "user", "text": text, "voz": voz, "sigue": true }));
            if self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                return;
            }
            let _free = Free(&self.busy);
            if self.is_brain() && !self.motor.healthy() {
                return;
            }
            let words = sonido["palabras"].as_u64().unwrap_or(0) as usize;
            let read = self.read_monologue(&text, &sonido);
            let decision = read.as_ref().and_then(|v| v["decision"].as_str()).unwrap_or("esperar").to_string();
            self.emit("k:monologue", json!({ "read": read, "decision": decision }));
            if decision == "esperar" {
                self.status(&Ui::StListening.say(&[]));
                self.persist();
                return;
            }
            let v = read.unwrap_or(Value::Null);
            let (emotion, intensity, at_her) = (v["emocion"].as_str().unwrap_or("").to_string(), v["intensidad"].as_f64().unwrap_or(0.0), v["contra_ella"].as_bool().unwrap_or(false));
            let max_tokens = if decision == "breve" { 60 } else { 220 };
            if let Err(e) = self.reply_to_monologue(&text, &decision, words, &emotion, intensity, at_her, max_tokens) {
                self.status("");
                self.emit("k:error", json!({ "text": e }));
            }
        });
    }

    /// The quick read: how charged the piece is and what she should do, by the model.
    fn read_monologue(&self, text: &str, sonido: &Value) -> Option<Value> {
        read_monologue_with(&self.llm, self.mind().lang(), text, sonido)
    }

    /// Her answer to a monologue piece already in her window, led by a line of narration.
    #[allow(clippy::too_many_arguments)]
    fn reply_to_monologue(&self, text: &str, decision: &str, words: usize, emotion: &str, intensity: f64, at_her: bool, max_tokens: u32) -> Result<(), String> {
        let image = self.current_screen();
        let seen = lock(&self.eyes).seen.clone();
        self.status(&Ui::StReplying.say(&[]));
        let (prepared, temperature) = {
            let mut m = self.mind();
            let hint = crate::lang::monologue_hint(m.lang(), decision, words, emotion, intensity, at_her);
            m.say(Speaker::Narrator, &hint, &[]);
            let (wt, desc) = (m.hormones.window_tokens(), m.mood());
            (m.prepare_reply(wt, &desc, seen.as_deref(), image.as_deref()), m.hormones.temperature())
        };
        let reply = self.speak_stream(&prepared, temperature, max_tokens)?;
        let (clean_reply, _) = crate::minecraft_bridge::decided_action_gated(&reply, self.mind().homeostasis.agency_state());
        self.voice_out(&clean_reply);
        let recalled: Vec<u32> = prepared.recalled.iter().map(|r| r.0).collect();
        self.mind().record_reply(text, &clean_reply, &recalled);
        self.persist();
        self.status("");
        self.emit("k:state", self.state_json());
        Ok(())
    }


    fn notice_hesitation(&self, ghosts: &[String]) -> Result<(), String> {
        if self.is_brain() && !self.motor.healthy() {
            self.status(&Ui::StWaking.say(&[]));
            if let Err(e) = self.motor.start(Duration::from_secs(240)) {
                return Err(Ui::ErrModelStart.say(&[&e]));
            }
        }
        let user = self.mind().user.clone();
        for g in ghosts {
            self.mind().say(Speaker::Ghost, g, &[]);
            self.emit("k:log", json!({ "role": "ghost", "text": g }));
        }
        self.mind().hormones.feel(Stimulus::Hesitation);
        let note = crate::lang::silence_note(self.mind().lang(), &user);
        self.mind().say(Speaker::Sense, &note, &[]);
        self.emit("k:log", json!({ "role": "sense", "text": note }));
        self.status(&Ui::StWatching.say(&[]));
        let (prepared, temperature) = {
            let m = self.mind();

            let mut m = m;
            let wt = m.hormones.window_tokens();
            let desc = m.mood();
            let p = m.prepare_reply(wt, &desc, None, None);
            let t = m.hormones.temperature();
            (p, t)
        };
        // A comment of her own carries no action: only the words go out.
        let reply = crate::minecraft_bridge::spoken(&self.speak_stream(&prepared, temperature, 500)?);
        self.voice_out(&reply);
        crate::minecraft_bridge::broadcast_chat(&reply);
        let recalled: Vec<u32> = prepared.recalled.iter().map(|r| r.0).collect();
        {
            let mut m = self.mind();
            let placeholder = crate::lang::silence_placeholder(m.lang());
            m.record_reply(placeholder, &reply, &recalled);
            m.hormones.feel(Stimulus::Acted);
        }
        self.persist();
        lock(&self.eyes).lived += 1;
        self.status("");
        self.emit("k:state", self.state_json());
        Ok(())
    }

    /// Brain: she looks at a screenshot (from whichever screen her body is on).
    fn look(&self, jpeg: &str) -> Result<(), String> {
        let (seen, spoke) = {
            let e = lock(&self.eyes);
            (e.seen.clone(), e.last_spoke.map(|t| unix_now() - t))
        };
        self.status(&Ui::StLooking.say(&[]));
        let (prepared, temperature) = {
            let m = self.mind();

            (m.prepare_look(m.hormones.window_tokens(), &m.mood(), jpeg, seen.as_deref(), spoke), m.hormones.temperature())
        };
        let v = self.llm.chat_json(&prepared.messages, &mind::look_schema(), SLOT_VOICE, temperature.min(0.7), 300)?;
        let ve = v["ve"].as_str().unwrap_or("").trim().to_string();
        {
            let mut e = lock(&self.eyes);
            e.last_look = unix_now();
            if !ve.is_empty() && e.seen.as_deref() != Some(ve.as_str()) {
                e.seen = Some(ve.clone());
                e.lived += 1;
            }
        }
        if !ve.is_empty() {
            let mut m = self.mind();
            m.record_seen(&ve);
            m.hormones.feel(Stimulus::NewFact);
            self.emit("k:log", json!({ "role": "sense", "text": Ui::LogSaw.say(&[&ve]) }));
        }
        let texto = mind::clean_reply(v["texto"].as_str().unwrap_or(""));
        if v["hablar"].as_bool() == Some(true) && !texto.is_empty() {
            if let Some(x) = v["x"].as_f64().filter(|x| (0.0..=1.0).contains(x)) {
                self.body_cmd(json!({ "c": "walk", "x": x }));
            }
            self.speak_line(&texto);
            // What she says on her own is heard too, like her replies.
            self.voice_out(&texto);
            let mut m = self.mind();
            let recalled: Vec<u32> = prepared.recalled.iter().map(|r| r.0).collect();
            let placeholder = crate::lang::look_placeholder(m.lang());
            m.record_reply(placeholder, &texto, &recalled);
            m.hormones.feel(Stimulus::Acted);
        }
        self.persist();
        self.status("");
        self.emit("k:state", self.state_json());
        Ok(())
    }

    /// Brain: looks at a screenshot in the background if she is free. False if busy.
    fn look_async(self: &Arc<Self>, jpeg: String) -> bool {
        if self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return false;
        }
        let core = self.clone();
        std::thread::spawn(move || {
            let _free = Free(&core.busy);
            if let Err(e) = core.look(&jpeg) {
                core.status("");
                core.emit("k:error", json!({ "text": e }));
            }
        });
        true
    }

    /// "Mira ahora": the screen she is on, from any computer.
    pub fn look_now(self: &Arc<Self>) -> Result<(), String> {
        match self.role() {
            Role::Brain => {
                // Still loading, llama-server would answer 503: say so instead.
                if !self.motor.healthy() {
                    return Err(Ui::StWaking.say(&[]));
                }
                let on = lock(&self.body_on).clone();
                let jpeg = if on == self.me() {
                    self.shoot().and_then(|s| s.jpeg_b64(SHOT_W))
                } else {
                    let p = self.net.peer(&on).ok_or_else(|| Ui::ErrScreenSilent.say(&[]))?;
                    self.net.get(&p.addr, "/ojos?todo=1", 15)?["jpeg"].as_str().map(String::from)
                };
                let jpeg = jpeg.ok_or_else(|| Ui::ErrCapture.say(&[]))?;
                if !self.look_async(jpeg) {
                    return Err(Ui::ErrBusy.say(&[]));
                }
                Ok(())
            }
            Role::Terminal(_) => {
                let addr = self.brain_addr().ok_or_else(|| Ui::ErrBrainSilent.say(&[]))?;
                self.net.post(&addr, "/mirar-ahora", &json!({}), 15).map(|_| ())
            }
            Role::Alone => Err(Ui::ErrNobodyThinksShort.say(&[])),
        }
    }

    fn reflect(&self) -> Result<(), String> {
        let Some(ex) = self.mind().reflection() else { return Ok(()) };
        self.status(&Ui::StReflecting.say(&[]));
        let v = self.llm.chat_json(&ex.messages, &mind::reflection_schema(), SLOT_ANALYSIS, 0.3, 300)?;
        let events = {
            let mut m = self.mind();
            let w = m.hormones.plasticity();
            let ev = m.reflect(&v, &ex.numbered, &ex.themes, w);
            for e in &ev {
                // Her personal core or her subsectors changing is not a new thing learned: no hormones for it.
                if !e.action.starts_with("nucleo_") && !e.action.starts_with("tema_") {
                    m.hormones.feel(if e.action == "nuevo" { Stimulus::NewFact } else { Stimulus::Reinforced });
                }
            }
            ev
        };
        self.persist();
        let l = lang::ui();
        for e in &events {
            let Some(verb) = lang::log_verb(e.action) else { continue };
            // The reason a theme was dropped is decided in Spanish; it is shown in the app's language.
            let label = if e.action == "tema_descartado" { lang::theme_rejection(l, &e.label) } else { e.label.clone() };
            self.emit("k:log", json!({ "role": "sense", "text": format!("{}: {label}", verb.text(l)) }));
        }
        self.status("");
        self.emit("k:state", self.state_json());
        Ok(())
    }

    /// First launch (see `Mind::first_run`): the memory is rebuilt from the base seed of the chosen
    /// language with the primary user's name. It is done on the computer that holds the memory.
    pub fn first_run(self: &Arc<Self>, name: &str, language: &str) -> Result<(), String> {
        if let Role::Terminal(_) = self.role() {
            return Err(Ui::ErrFirstRunBrain.say(&[]));
        }
        self.mind().first_run(name, language)?;
        if self.is_brain() {
            self.persist();
        }
        self.emit("k:state", self.state_json());
        Ok(())
    }

    /// Settings changed in the app of any computer.
    pub fn configure(self: &Arc<Self>, v: &Value) -> Result<(), String> {
        if let Role::Terminal(_) = self.role() {
            let addr = self.brain_addr().ok_or("el cerebro no responde")?;
            self.net.post(&addr, "/config", v, 10)?;
            return Ok(());
        }
        let mut reposo_changed = None;
        {
            let mut m = self.mind();
            if let Some(x) = v["vision"].as_bool() {
                m.config.vision = x;
            }
            if let Some(x) = v["wander"].as_bool() {
                m.config.wander = x;
            }
            if let Some(g) = v["min_gap"].as_u64() {
                m.config.min_gap = g.clamp(15, 3600);
            }
            if let Some(r) = v["reposo"].as_bool() {
                if m.config.reposo != r {
                    m.config.reposo = r;
                    reposo_changed = Some(r);
                }
            }
            if let Some(mc) = v["minecraft"].as_bool() {
                m.config.minecraft = mc;
            }
            if let Some(l) = v["language"].as_str().filter(|l| crate::cthg::seed::LANGUAGES.contains(l)) {
                m.config.language = l.to_string();
                lang::set_ui(m.lang());
            }
            if let Some(u) = v["user"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 40) {
                m.user = u.to_string();
            }
        }
        if self.is_brain() {
            self.persist();
        } else {
            let _ = self.mind().save();
        }
        if let Some(r) = reposo_changed {
            self.apply_reposo(r);
        }
        self.emit("k:state", self.state_json());
        Ok(())
    }

    fn apply_reposo(self: &Arc<Self>, reposo: bool) {
        if reposo {
            crate::voice::stop();
            // Resting, she does not listen either.
            if crate::openmic::is_on() {
                crate::openmic::stop();
                self.emit_local("k:openmic", json!({ "on": false, "oyendo": false }));
            }
            if self.is_brain() {
                self.motor.stop();
            }
            lock(&self.body).rest = "Sleep";
            self.emit_local("k:anim", json!({ "name": "Sleep", "loop": true }));
            self.status(&Ui::StResting.say(&[]));
        } else {
            lock(&self.body).rest = "Relax";
            self.emit_local("k:anim", json!({ "name": "Relax", "loop": true }));
            if self.is_brain() {
                self.status(&Ui::StWaking.say(&[]));
                let core = self.clone();
                std::thread::spawn(move || {
                    if let Err(e) = core.motor.start(Duration::from_secs(240)) {
                        core.status("");
                        core.emit("k:error", json!({ "text": Ui::ErrWake.say(&[&e]) }));
                    } else {
                        core.status("");
                        core.emit("k:state", core.state_json());
                    }
                });
            } else {
                self.status("");
            }
        }
    }

    // ── the link ──

    /// Requests from the other computers.
    pub fn handle(self: &Arc<Self>, method: &str, path: &str, from: &str, v: Value) -> Result<Value, (u16, String)> {
        let bad = |e: String| (400u16, e);
        let brain_only = || if self.is_brain() { Ok(()) } else { Err((409u16, "este equipo no es el cerebro".to_string())) };
        match (method, path) {
            ("GET", "/memoria") => Ok(json!({ "files": files_to_json(&self.mind().files()) })),
            ("POST", "/memoria") => {
                if self.is_brain() {
                    return Err((409, "el cerebro no acepta réplicas".into()));
                }
                let files = files_from_json(&v["files"]).map_err(bad)?;
                self.mind().replace(&files).map_err(bad)?;
                self.emit_local("k:state", self.state_json());
                self.emit_local("k:memoria", json!({}));
                Ok(json!({ "ok": true }))
            }
            ("POST", "/fusionar") => {
                brain_only()?;
                let files = files_from_json(&v["files"]).map_err(bad)?;
                let n = self.mind().absorb(&files).map_err(bad)?;
                lock(&self.sent).clear();
                self.persist();
                Ok(json!({ "nuevos": n }))
            }
            ("POST", "/eventos") => {
                for e in v["events"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                    if let (Some(ev), Some(p)) = (e[0].as_str(), e.get(1)) {
                        self.emit_local(ev, p.clone());
                    }
                }
                Ok(json!({ "ok": true }))
            }
            ("POST", "/voz") => {
                let needs = self.speak_here(v["text"].as_str().unwrap_or(""), v["idioma"].as_str().unwrap_or("es"));
                Ok(json!({ "ok": true, "idioma": needs }))
            }
            ("POST", "/cuerpo") => {
                self.apply_body(&v);
                Ok(json!({ "ok": true }))
            }
            ("POST", "/salio") => {
                brain_only()?;
                self.exited();
                Ok(json!({ "ok": true }))
            }
            ("POST", "/traslado") => {
                brain_only()?;
                let to = v["to"].as_str().ok_or_else(|| bad("falta el destino".into()))?;
                self.transfer(to, Side::parse(v["lado"].as_str().unwrap_or("")));
                Ok(json!({ "ok": true }))
            }
            ("POST", "/oir") => {
                brain_only()?;
                let text: String = v["text"].as_str().unwrap_or("").trim().chars().take(4000).collect();
                if text.is_empty() {
                    return Err(bad("mensaje vacío".into()));
                }
                let ghosts: Vec<String> = v["ghosts"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let voz = v.get("voz").filter(|x| x.as_array().is_some_and(|a| !a.is_empty())).cloned();
                let sonido = v.get("sonido").filter(|x| x.is_object()).cloned();
                self.hear_utterance(crate::sensor::Utterance { said: text, ghosts, voz, sonido }).map_err(bad)?;
                Ok(json!({ "ok": true }))
            }
            ("POST", "/monologo") => {
                brain_only()?;
                let text: String = v["text"].as_str().unwrap_or("").trim().chars().take(4000).collect();
                if text.is_empty() {
                    return Err(bad("mensaje vacío".into()));
                }
                self.clone().monologue_here(text, v.get("voz").cloned(), v.get("sonido").cloned().unwrap_or(Value::Null));
                Ok(json!({ "ok": true }))
            }
            ("POST", "/vacilacion") => {
                brain_only()?;
                let ghosts: Vec<String> = v["ghosts"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                if !ghosts.is_empty() && !self.mind().config.reposo && !self.busy.load(Ordering::Acquire) {
                    let core = self.clone();
                    std::thread::spawn(move || {
                        if !core.take_voice(Duration::from_secs(10)) {
                            return;
                        }
                        let _free = Free(&core.busy);
                        let _ = core.notice_hesitation(&ghosts);
                    });
                }
                Ok(json!({ "ok": true }))
            }
            ("POST", "/mirar") => {
                brain_only()?;
                let jpeg = v["jpeg"].as_str().ok_or_else(|| bad("sin imagen".into()))?.to_string();
                if self.look_async(jpeg) { Ok(json!({ "ok": true })) } else { Err((429, "ocupada".into())) }
            }
            ("POST", "/mirar-ahora") => {
                brain_only()?;
                self.look_now().map_err(bad)?;
                Ok(json!({ "ok": true }))
            }
            ("GET", p) if p.starts_with("/ojos") => {
                if !self.here.load(Ordering::Acquire) {
                    return Err((409, "no está en esta pantalla".into()));
                }
                let jpeg = if p.contains("todo=1") { self.shoot().and_then(|s| s.jpeg_b64(SHOT_W)) } else { self.capture_if_changed() };
                Ok(json!({ "jpeg": jpeg }))
            }
            ("POST", "/config") => {
                brain_only()?;
                self.configure(&v).map_err(bad)?;
                Ok(json!({ "ok": true }))
            }
            ("POST", "/propuesta") => {
                brain_only()?;
                let name = v["nombre"].as_str().unwrap_or("otro equipo").to_string();
                *lock(&self.proposal) = Some((from.to_string(), name.clone()));
                if std::env::var("KALTSIT_ACEPTAR_RELEVO").is_ok() {
                    let _ = self.resolve_proposal(true);
                    return Ok(json!({ "ok": true }));
                }
                self.emit_local("k:propuesta", json!({ "id": from, "nombre": name }));
                self.net_changed();
                Ok(json!({ "ok": true }))
            }
            ("POST", "/relevo") => {
                *lock(&self.asked) = None;
                if v["ok"].as_bool() != Some(true) {
                    self.status("");
                    self.emit_local("k:error", json!({ "text": "el cerebro actual no aceptó el relevo" }));
                    self.net_changed();
                    return Ok(json!({ "ok": true }));
                }
                let files = files_from_json(&v["files"]).map_err(bad)?;
                self.mind().replace(&files).map_err(bad)?;
                let body = v["cuerpo"].as_str().map(String::from);
                let core = self.clone();
                std::thread::spawn(move || {
                    let _ = core.become_brain(body);
                });
                Ok(json!({ "ok": true }))
            }
            _ => Err((404, format!("{method} {path}"))),
        }
    }
}

// ── threads ──

/// Sends the brain's events to the other computers, batched every 60 ms.
pub fn spawn_outbox(core: Arc<Core>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(60));
        let batch: Vec<Value> = std::mem::take(&mut *lock(&core.outbox));
        if batch.is_empty() || !core.is_brain() {
            continue;
        }
        let body = json!({ "events": batch });
        let linked: Vec<String> = lock(&core.linked).iter().cloned().collect();
        for id in linked {
            if let Some(p) = core.net.peer(&id) {
                let (net, body) = (core.net.clone(), body.clone());
                std::thread::spawn(move || {
                    let _ = net.post(&p.addr, "/eventos", &body, 5);
                });
            }
        }
    });
}

/// Every 2 s: who is the brain, who joined, who left; she follows the user between screens.
pub fn spawn_roles(core: Arc<Core>) {
    std::thread::spawn(move || {
        // Listen first: if another computer is already thinking, follow it.
        std::thread::sleep(Duration::from_secs(5));
        let mut started = false;
        let mut follow_votes = 0;
        loop {
            let alive = core.net.alive();
            let brain = alive.iter().find(|p| p.status.brain).cloned();
            match core.role() {
                Role::Alone => {
                    // Starting together, only one loads the model: the first in id order among
                    // those that could think (two at once would not fit in the VRAM).
                    let first = !alive.iter().any(|p| p.status.think && p.id.as_str() < core.me());
                    if let Some(b) = &brain {
                        core.become_terminal(&b.id, false);
                    } else if !started && first && lock(&core.local).can_think() {
                        started = true;
                        let _ = core.become_brain(None);
                    }
                }
                Role::Terminal(b) => {
                    if !alive.iter().any(|p| p.id == b) {
                        // The brain is gone (power cut?): the memory stays here, and so does she.
                        *lock(&core.role) = Role::Alone;
                        core.set_here(true);
                        core.place_start();
                        core.emit_local("k:error", json!({ "text": "el cerebro dejó de responder: la memoria sigue aquí. Para que piense en este equipo, «Pensar aquí» en «Red»." }));
                        core.net_changed();
                    } else if let Some(other) = brain.as_ref().filter(|p| p.id != b) {
                        core.become_terminal(&other.id, core.here.load(Ordering::Acquire));
                    }
                }
                Role::Brain => {
                    let since = core.net.status().brain_since;
                    // Two brains (each started alone): the younger merges into the older one.
                    if let Some(older) = alive.iter().find(|p| p.status.brain && (p.status.brain_since, p.id.as_str()) < (since, core.me())) {
                        let files = core.mind().files();
                        if core.net.post(&older.addr, "/fusionar", &json!({ "files": files_to_json(&files) }), 60).is_ok() {
                            core.emit_local("k:log", json!({ "role": "sense", "text": Ui::LogJoins.say(&[&older.name]) }));
                            core.become_terminal(&older.id, false);
                            continue;
                        }
                    }
                    let ids: HashSet<String> = alive.iter().map(|p| p.id.clone()).collect();
                    lock(&core.linked).retain(|id| ids.contains(id));
                    for p in alive.iter().filter(|p| !p.status.brain) {
                        if !lock(&core.linked).contains(&p.id) {
                            core.link(p);
                        }
                    }
                    // Her body on a computer that vanished: she comes back here.
                    let on = lock(&core.body_on).clone();
                    if on != core.me() && !ids.contains(&on) {
                        *lock(&core.body_on) = core.me().to_string();
                        core.set_here(true);
                        core.place_start();
                    }
                    // A move that never finished (the other screen did not answer).
                    let stuck = lock(&core.pending_enter).as_ref().is_some_and(|p| unix_now() - p.2 > 40.0);
                    if stuck {
                        core.exited();
                    }
                    // She goes where the user is working.
                    let wander = core.mind().config.wander;
                    let idle_of = |id: &str| if id == core.me() { local::idle_seconds() } else { alive.iter().find(|p| p.id == id).map(|p| p.idle).unwrap_or(f64::INFINITY) };
                    let mut candidates: Vec<(String, f64)> = alive.iter().map(|p| (p.id.clone(), p.idle)).collect();
                    candidates.push((core.me().to_string(), local::idle_seconds()));
                    let target = candidates.into_iter().filter(|(id, idle)| *id != on && *idle <= ACTIVE_S).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                    if wander && idle_of(&on) >= AWAY_S && target.is_some() {
                        follow_votes += 1;
                    } else {
                        follow_votes = 0;
                    }
                    if follow_votes >= 2 {
                        follow_votes = 0;
                        let (to, _) = target.unwrap();
                        let to_name = if to == core.me() { core.net.name.clone() } else { alive.iter().find(|p| p.id == to).map(|p| p.name.clone()).unwrap_or_default() };
                        // Leave through the side where that computer is, if the user said.
                        let (left, right) = if on == core.me() {
                            let l = lock(&core.local);
                            (l.vecinos.izquierda.clone(), l.vecinos.derecha.clone())
                        } else {
                            alive.iter().find(|p| p.id == on).map(|p| (p.status.left.clone(), p.status.right.clone())).unwrap_or_default()
                        };
                        let side = if left.eq_ignore_ascii_case(&to_name) && !left.is_empty() { Side::Left } else { Side::Right };
                        let _ = right;
                        core.transfer(&to, side);
                    }
                }
            }
            core.sync_links();
            core.emit_local("k:red", core.net_json());
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// Every 3 s on the brain: forgetting, boredom and reflection.
pub fn spawn_mind_clock(core: Arc<Core>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        let role = core.role();
        // Check for wiped ghosts (user typed something in chat, deleted it all and stayed silent)
        let now = crate::clock::unix_now();
        let wiped = {
            let mut s = lock(&core.sensor);
            s.take_wiped_ghosts_if_silent(now, 8.0)
        };
        if !wiped.is_empty() {
            match role {
                Role::Brain => {
                    if !core.mind().config.reposo && !core.busy.load(Ordering::Acquire) {
                        let core2 = core.clone();
                        std::thread::spawn(move || {
                            if !core2.take_voice(Duration::from_secs(10)) {
                                return;
                            }
                            let _free = Free(&core2.busy);
                            if let Err(e) = core2.notice_hesitation(&wiped) {
                                core2.status("");
                                core2.emit("k:error", json!({ "text": e }));
                            }
                        });
                    }
                }
                Role::Terminal(_) => {
                    if let Some(addr) = core.brain_addr() {
                        let net = core.net.clone();
                        std::thread::spawn(move || {
                            let _ = net.post(&addr, "/vacilacion", &json!({ "ghosts": wiped }), 5);
                        });
                    }
                }
                Role::Alone => {}
            }
        }
        if !core.is_brain() {
            continue;
        }
        if core.mind().config.reposo {
            // Resting is sleeping: no boredom and no autonomous life, but her body still calms
            // down (before, her levels froze until she woke).
            core.mind().hormones.decay(3.0);
            continue;
        }
        {
            let mut m = core.mind();
            crate::cthg::learn::fade(&mut m.mem, 0, 3.0);
            let still = {
                let e = lock(&core.eyes);
                unix_now() - e.last_look.max(e.last_spoke.unwrap_or(0.0))
            };
            if still > 300.0 {
                m.hormones.feel(Stimulus::Bored { dt: 3.0, level: ((still - 300.0) / 600.0).min(1.5) as f32 });
            }
            m.hormones.decay(3.0);
        }
        if core.busy.load(Ordering::Acquire) {
            continue;
        }
        let (need, gap) = if std::env::var("KALTSIT_REFLEXION_RAPIDA").is_ok() { (2, 20.0) } else { (6, 600.0) };
        let due = {
            let e = lock(&core.eyes);
            e.lived >= need && unix_now() - e.last_reflect >= gap
        };
        // Not while her own model is still loading (or stopped): it would only answer 503.
        if due && (!core.is_brain() || core.motor.healthy()) && core.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            {
                let mut e = lock(&core.eyes);
                e.lived = 0;
                e.last_reflect = unix_now();
            }
            let _free = Free(&core.busy);
            if let Err(e) = core.reflect() {
                core.status("");
                core.emit("k:error", json!({ "text": e }));
            }
        }
    });
}

/// Every 3 s on the computer her body is on: has this screen changed enough to look again?
pub fn spawn_eyes(core: Arc<Core>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        if !core.here.load(Ordering::Acquire) {
            continue;
        }
        let role = core.role();
        if role == Role::Alone {
            continue;
        }
        let (vision, in_reposo, in_minecraft, min_gap, max_gap, first_launch) = {
            let m = core.mind();
            (m.config.vision, m.config.reposo, m.config.minecraft, m.config.min_gap as f64, m.config.max_gap as f64, m.first_run_pending())
        };
        // Until the first-launch screen is done nothing is lived: she does not look yet.
        if in_reposo || in_minecraft || first_launch || !vision || (role == Role::Brain && core.busy.load(Ordering::Acquire)) {
            continue;
        }
        let Some(shot) = core.shoot() else { continue };
        let thumb = shot.thumb();
        let (since, changed) = {
            let e = lock(&core.eyes);
            (unix_now() - e.last_look, screen::change(&thumb, &e.thumb))
        };
        if !((since >= min_gap && changed >= CHANGE) || since >= max_gap) {
            continue;
        }
        // While llama-server is still loading (it can come up after the app) it answers 503:
        // she waits for it quietly and looks once it is ready.
        if role == Role::Brain && !core.motor.healthy() {
            continue;
        }
        let Some(jpeg) = shot.jpeg_b64(SHOT_W) else { continue };
        let sent = match role {
            Role::Brain => core.look_async(jpeg),
            Role::Terminal(_) => core.brain_addr().is_some_and(|a| core.net.post(&a, "/mirar", &json!({ "jpeg": jpeg }), 15).is_ok()),
            Role::Alone => false,
        };
        let mut e = lock(&core.eyes);
        e.thumb = thumb;
        // A failed or busy look is retried in a while, not every 3 s.
        e.last_look = unix_now() - if sent { 0.0 } else { min_gap * 0.5 };
    });
}

/// 30 fps on every computer: moves the overlay while her body is on this screen; now and then
/// she wanders, sits, dozes off, or walks off an edge into the neighbouring computer.
pub fn spawn_body(core: Arc<Core>) {
    std::thread::spawn(move || {
        let mut seed = unix_now().to_bits() ^ 0x9E37_79B9_7F4A_7C15;
        let mut rand = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 10_000) as f64 / 10_000.0
        };
        let mut last = std::time::Instant::now();
        let mut area = None;
        let mut scale = 1.0;
        let mut frame = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            let dt = last.elapsed().as_secs_f64();
            last = std::time::Instant::now();
            if !core.here.load(Ordering::Acquire) {
                continue;
            }
            if frame % 60 == 0 || area.is_none() {
                area = core.area();
                scale = core.overlay().and_then(|w| w.scale_factor().ok()).unwrap_or(1.0);
            }
            frame = frame.wrapping_add(1);
            let Some((ax, ay, aw, ah, ww, wh)) = area else { continue };
            let y = ay + ah - wh + 4.0 * scale;
            let mut b = lock(&core.body);
            if let Some(t) = b.target {
                // Leaving, she sprints off the edge; entering, she starts outside it.
                let step = SPEED * scale * dt * if b.leaving.is_some() { 1.6 } else { 1.0 };
                let dx = t - b.x;
                let arrived = dx.abs() <= step;
                b.x = if arrived { t } else { b.x + step * dx.signum() };
                let outside = b.leaving.is_some() || b.x < ax || b.x > ax + aw - ww;
                let x = if outside { b.x } else { b.x.clamp(ax, ax + aw - ww) };
                let mut gone = false;
                if arrived {
                    b.target = None;
                    gone = b.leaving.take().is_some();
                }
                let rest = b.rest;
                drop(b);
                if let Some(w) = core.overlay() {
                    let _ = w.set_position(PhysicalPosition::new(x as i32, y as i32));
                }
                if gone {
                    core.left_screen();
                } else if arrived {
                    core.emit_local("k:anim", json!({ "name": rest, "loop": true }));
                }
                continue;
            }
            let now = unix_now();
            if now < b.idle_until {
                continue;
            }
            b.idle_until = now + 25.0 + 45.0 * rand();
            drop(b);
            let (wander, in_reposo, d) = {
                let m = core.mind();
                (m.config.wander, m.config.reposo, m.hormones.get(crate::hormones::H::Dopamina))
            };
            if in_reposo {
                if lock(&core.body).rest != "Sleep" {
                    lock(&core.body).rest = "Sleep";
                    core.emit_local("k:anim", json!({ "name": "Sleep", "loop": true }));
                }
                continue;
            }
            if !wander || core.busy.load(Ordering::Acquire) {
                continue;
            }
            let r = rand();
            let walk = 0.35 + 0.6 * d as f64;
            if r < walk {
                // Sometimes the walk takes her off the edge, into the neighbouring computer.
                let (left, right) = {
                    let l = lock(&core.local);
                    (l.vecinos.izquierda.clone(), l.vecinos.derecha.clone())
                };
                let doors: Vec<(String, Side)> = [(left, Side::Left), (right, Side::Right)].into_iter().filter(|(n, _)| !n.is_empty() && core.net.peer_by_name(n).is_some()).collect();
                if !doors.is_empty() && rand() < 0.2 && core.role() != Role::Alone {
                    let (name, side) = doors[(rand() * doors.len() as f64) as usize % doors.len()].clone();
                    core.request_move(&name, side);
                    continue;
                }
                lock(&core.body).rest = "Relax";
                core.walk_local(0.08 + 0.84 * rand());
            } else if d < 0.25 && r > 0.85 {
                lock(&core.body).rest = "Sleep";
                core.emit_local("k:anim", json!({ "name": "Sleep", "loop": true }));
            } else if r > 0.7 {
                lock(&core.body).rest = "Sit";
                core.emit_local("k:anim", json!({ "name": "Sit", "loop": true }));
            } else {
                lock(&core.body).rest = "Relax";
                core.emit_local("k:anim", json!({ "name": "Relax", "loop": true }));
            }
        }
    });
}

/// The quick read of a monologue piece by the model at `llm`: emotion, intensity, whether it is
/// aimed at her, whether it is venting, and what she should do (`esperar` | `breve` |
/// `interrumpir`).
pub fn read_monologue_with(llm: &Llm, l: crate::lang::Lang, text: &str, sonido: &Value) -> Option<Value> {
    let user = match l {
        crate::lang::Lang::Es => format!("Lo que lleva dicho:\n{text}\n\nCómo suena: {}", crate::lang::monologue_sound(l, sonido)),
        crate::lang::Lang::En => format!("What they have said:\n{text}\n\nHow it sounds: {}", crate::lang::monologue_sound(l, sonido)),
    };
    let messages = vec![Msg::text("system", crate::lang::monologue_reading_prompt(l)), Msg::text("user", user)];
    let schema = json!({
        "type": "object",
        "properties": {
            "emocion": { "type": "string" },
            "intensidad": { "type": "number", "minimum": 0, "maximum": 1 },
            "contra_ella": { "type": "boolean" },
            "desahogo": { "type": "boolean" },
            "decision": { "type": "string", "enum": ["esperar", "breve", "interrumpir"] }
        },
        "required": ["emocion", "intensidad", "contra_ella", "desahogo", "decision"]
    });
    llm.chat_json(&messages, &schema, SLOT_ANALYSIS, 0.2, 120).ok()
}
