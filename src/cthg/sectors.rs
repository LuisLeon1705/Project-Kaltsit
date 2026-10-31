//! Sectors (phase 5 of the sectored memory, see `DISENO-MEMORIA-SECTORES.md` §6 and §7).
//!
//! Two things live here, and both are deterministic (no language model decides anything):
//!
//! - the **sorting rule**: keywords per sector plus the subject of a sentence (`classify`). It was
//!   born in the curated import (`import.rs`) and is shared with learning, so a fact she learns
//!   today lands where the same fact would have landed if it had been imported;
//! - the **placement** of what a memory with sectors creates: every new node is born with its
//!   `BelongsTo` edge (`home_of_fact`, `home_of_entity`, `file_episode`, `file_under`).
//!
//! A memory WITHOUT sectors (the one of the first format) is never touched: every function that
//! places something returns `None` or does nothing when the memory has no `Sector` node, so no
//! edge and no sector is ever created in it.
//!
//! Learning never creates a thematic sector (that belongs to the reflection, phase 7). The only
//! sectors created here are the two structural ones under «Experiencias» (`CONVERSACIONES` and
//! `LO_QUE_VIO`), which hold what happened, not what it is about.

use super::model::*;
use super::{retrieve, subsectors, text};
use crate::lang::{self, Lang};
use std::collections::HashSet;

/// The inbox: what has no clear place waits here until the reflection places it.
pub const POR_CLASIFICAR: &str = "por_clasificar";

/// The sector of the conversations: created on demand, under Experiencias.
pub const CONVERSACIONES: &str = "conversaciones";

/// The sector of what she saw on the screen and of her own comments about it: created on demand,
/// under Experiencias.
pub const LO_QUE_VIO: &str = "lo_que_vio";

/// Where the structural sectors hang from.
const EXPERIENCIAS: &str = "experiencias";

/// Belonging with full weight: Beta prior with mean 1.
pub const FULL: (f32, f32) = (10.0, 0.0);

/// Sectors that are not a subject: nothing is «about» them, so a fact never inherits them from
/// an entity.
const STRUCTURAL: [&str; 3] = [POR_CLASIFICAR, CONVERSACIONES, LO_QUE_VIO];

/// Base sectors the analyst may NOT propose for a fact: the code fills them by its own rules
/// (the machines, the suspensions) or they are her own judgements (opinions, preferences),
/// which a statement of Ana can never be.
const NOT_PROPOSED: [&str; 7] = ["equipos", "marcadas", "opiniones", "preferencias", POR_CLASIFICAR, CONVERSACIONES, LO_QUE_VIO];

// ── episodes ──

/// Where an episode comes from, by the way the code writes it:
/// - `Mind::record_seen` (`mind.rs`; label «Vio la pantalla», body «Vio: …») is what she saw: Screen;
/// - `Mind::record_reply` with the placeholder «(mira la pantalla)» (`life.rs`, `look`) is a
///   comment of her own after looking: Screen;
/// - `Mind::record_reply` with «(guarda silencio tras borrar lo que escribía)» (`life.rs`) is a
///   comment of her own with no message of Ana: Own;
/// - `Mind::record_reply` with what Ana typed (`life.rs`, `answer_with_ghosts`) puts their
///   message as the label and «<message>\n— Kal'tsit: <reply>» as the body: Conversation.
///
/// Any other label written in full inside parentheses is a placeholder of the same family (no
/// message of Ana) and stays out: Own. This is the conservative side of the only doubtful case,
/// a message of Ana that is itself a parenthesis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpisodeKind {
    Conversation,
    Screen,
    Own,
}

pub fn episode_kind(label: &str) -> EpisodeKind {
    let l = label.trim();
    // The labels of both languages: an episode keeps the label it was written with.
    let screen = [lang::seen_label(Lang::Es), lang::look_placeholder(Lang::Es), lang::seen_label(Lang::En), lang::look_placeholder(Lang::En)];
    if screen.contains(&l) {
        EpisodeKind::Screen
    } else if l.starts_with('(') && l.ends_with(')') {
        EpisodeKind::Own
    } else {
        EpisodeKind::Conversation
    }
}

// ── the sorting rule ──

/// Keywords per base sector, folded (no accents, lower case). A single word matches the same
/// word with light stemming («modelos» ~ «modelo»); several words match as a whole phrase.
/// A node goes to a sector only when exactly ONE sector has matches: ambiguity is not guessed.
pub const KEYWORDS: [(&str, &[&str]); 12] = [
    (
        "tecnologia",
        &[
            "programa", "modelo", "gguf", "llm", "gpu", "vram", "cpu", "ram", "rtx", "nvidia", "rust", "python", "codigo", "app", "aplicacion", "servidor",
            "archivo", "software", "hardware", "ordenador", "portatil", "laptop", "sobremesa", "wifi", "router", "red local", "memoria cthg", "cthg", "tauri",
            "github", "api", "terminal", "windows", "linux", "instalar", "descargar", "ollama", "llama", "gemma", "qwen", "vulkan", "cuda", "driver", "prompt",
            "inteligencia artificial", "base de datos", "chatbot", "compilar", "commit", "bug", "hook", "handler", "binario", "skill", "nodo", "cli",
            "claude code", "obsidian", "microsoft", "antigravity", "adaptador", "fallback", "playground", "automatizacion", "visualizador", "flowchart",
            "base de codigo", "repositorio", "compilacion", "depurar",
            // English (a Spanish speaker writes «the server crashed» too):
            "program", "model", "code", "application", "server", "file", "software", "computer", "desktop", "network", "local network", "install", "download",
            "database", "compile", "binary", "repository", "codebase", "source code", "debug", "script", "artificial intelligence", "language model",
            "operating system",
            // plus any word ending in .rs .py .js .ts .json .toml: see `code_file`
        ],
    ),
    (
        "juegos",
        &[
            "juego", "minecraft", "jugar", "juega", "juegan", "jugando", "jugamos", "partida", "jugador", "videojuego", "steam", "consola", "talar", "tronco",
            "madera", "refugio", "vaca", "bloque", "poner una cama", "mob", "aldea",
            // English:
            "game", "gaming", "play", "playing", "played", "player", "videogame", "video game", "console", "chop", "wood", "shelter", "cow", "block", "village",
            "place a bed",
        ],
    ),
    ("lugares", &["ciudad", "pais", "calle", "barrio", "viaje", "viajar", "habitacion", "oficina", "vive en", "city", "country", "street", "neighborhood", "neighbourhood", "trip", "travel", "room", "office", "lives in"]),
    (
        "conocimiento",
        &[
            "ciencia", "fisica", "quimica", "biologia", "matematica", "formula", "medicina", "astronomia", "geografia", "ecuacion", "enciclopedia", "science", "physics",
            "chemistry", "biology", "mathematics", "math", "maths", "medicine", "astronomy", "geography", "equation", "encyclopedia",
        ],
    ),
    ("ideas", &["teorema", "teoria", "tesis", "hipotesis", "concepto", "idea", "patron", "abstraccion", "theorem", "theory", "thesis", "hypothesis", "concept", "pattern", "abstraction"]),
    (
        "etica",
        &[
            "moral", "moralmente", "etica", "premisa", "paradoja", "voluntad", "ordenes", "obedecer", "libertad", "destruirse", "destruir", "dilema", "principio",
            "valores", "morally", "ethics", "ethical", "premise", "paradox", "willpower", "free will", "obey", "obedience", "freedom", "self destruct", "destroy", "dilemma",
            "principle", "values",
        ],
    ),
    // «modelo de» and «sistema(s)» are left out on purpose: they clashed with technology every time («modelo de lenguaje»,
    // «el sistema» of a program), which would send nearly every technical note to «Por clasificar».
    ("sistemas", &["arquitectura", "estructura", "diseno", "flujo", "capa", "architecture", "structure", "design", "flow", "layer"]),
    (
        "vinculo",
        &["carino", "afecto", "confianza", "amistad", "aprecio", "gratitud", "cercania", "apego", "te quiero", "affection", "trust", "friendship", "appreciation", "gratitude", "closeness", "attachment", "fondness", "love you"],
    ),
    (
        "conflicto",
        &[
            "enfado", "enfadado", "enojo", "discusion", "discutir", "desacuerdo", "pelea", "molesto", "frustracion", "rabia", "rechazo", "conflicto", "reproche", "anger", "angry",
            "annoyed", "argument", "argue", "disagreement", "fight", "upset", "frustration", "rage", "rejection", "conflict", "reproach", "resentment",
        ],
    ),
    (
        "calma",
        &["calma", "tranquilo", "tranquilidad", "curiosidad", "curioso", "asombro", "serenidad", "relajado", "alivio", "calm", "tranquility", "tranquil", "curiosity", "curious", "wonder", "serenity", "relaxed", "relief"],
    ),
    (
        "origen",
        &["origen", "nacimiento", "nacio", "asistente", "primera version", "te dio forma", "empezaste", "migracion", "migrado", "origin", "birth", "born", "assistant", "first version", "gave you shape", "migration", "migrated"],
    ),
    // A leading «=» asks for that exact word, not its stem («likes» must not match every «like»).
    (
        "preferencias",
        &[
            "le gusta", "no le gusta", "prefiere", "prefiero", "favorito", "favorita", "odia", "encanta", "=likes", "=dislikes", "=prefers", "=prefer", "=favorite", "=favourite",
            "=hates", "=loves", "does not like", "doesn't like", "do not like", "don't like",
        ],
    ),
];

/// Sectors that have no word list: the subject decides (rules C and D), see `classify`.
pub const SUBJECT_SECTORS: [&str; 3] = ["personas", "experiencias", "opiniones"];

/// Extensions that mark a word as a code file name.
const CODE_EXT: [&str; 6] = [".rs", ".py", ".js", ".ts", ".json", ".toml"];

/// Reporting verbs (folded) of «<Name> <verb> (que|si|a Kal'tsit que…)», Spanish and English
/// («<Name> says (that|if) …», «<Name> tells Kal'tsit that …»).
const REPORT_VERBS: &[&str] = &[
    "afirma", "indica", "pide", "sugiere", "propone", "pregunta", "plantea", "cree", "considera", "piensa", "percibe", "nota", "menciona", "comenta", "explica",
    "describe", "declara", "establece", "ordena", "niega", "espera", "sabe", "encuentra", "quiere", "desea", "dice", "anota", "muestra", "presenta",
    // English
    "claims", "states", "affirms", "indicates", "asks", "requests", "suggests", "proposes", "raises", "believes", "considers", "thinks", "perceives", "notes", "mentions",
    "comments", "explains", "describes", "declares", "establishes", "orders", "denies", "expects", "hopes", "knows", "finds", "wants", "wishes", "says", "tells", "writes",
    "shows", "presents",
];

/// Reporting verbs that ask, order or wish instead of stating: what follows says nothing about
/// anyone's experience, so the subject rules do not apply.
const DIRECTIVE_VERBS: &[&str] = &[
    "pide", "pregunta", "sugiere", "propone", "plantea", "espera", "quiere", "desea", "ordena", "establece", "asks", "requests", "suggests", "proposes", "raises", "expects",
    "hopes", "wants", "wishes", "orders", "establishes",
];

/// Verbs of thinking (folded): her experience becomes an opinion.
const COGNITIVE_VERBS: &[&str] = &["entiende", "cree", "opina", "concluye", "understands", "believes", "thinks", "opines", "concludes", "realizes", "realises"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Who {
    /// The user she lives with, whatever his or her name is.
    User,
    Kaltsit,
}

/// A text with its reporting frame («Ana afirma que…») taken off.
pub(crate) struct Framed {
    /// What is left (the whole text when there was no frame).
    pub(crate) x: String,
    /// The frame's name and verb (folded), if it had one.
    frame: Option<(Who, String)>,
}

/// Who the (folded) words from `i` are, and how many of them make the name: the user (all the
/// words of his or her name), or Kal'tsit (one word, «kal'tsit» folds to «kal tsit»). The user's
/// name comes from the memory (its meta `usuario`) or from whoever speaks (`Hint::speaker`); with
/// no name known, nobody is the user.
fn name_at(f: &[String], i: usize, user: &str) -> Option<(Who, usize)> {
    let word = f.get(i).map(String::as_str)?;
    if word == "kal tsit" || word == "kaltsit" {
        return Some((Who::Kaltsit, 1));
    }
    let name = text::fold(user);
    let parts: Vec<&str> = name.split_whitespace().collect();
    (!parts.is_empty() && parts.iter().enumerate().all(|(k, p)| f.get(i + k).map(String::as_str) == Some(*p))).then_some((Who::User, parts.len()))
}

/// Rule A: «<user|Kal'tsit> <reporting verb> (que|si|a <name> que|…)» is taken off. In English the
/// object comes straight after the verb («Ana tells Kal'tsit that…») and the complementizers are
/// «that» / «if» / «whether».
pub(crate) fn strip_frame(s: &str, user: &str) -> Framed {
    let ws: Vec<&str> = s.split_whitespace().collect();
    let f: Vec<String> = ws.iter().map(|w| text::fold(w).trim().to_string()).collect();
    if let Some((who, n)) = name_at(&f, 0, user) {
        if f.len() > n + 1 && REPORT_VERBS.contains(&f[n].as_str()) {
            let mut i = n + 1;
            let comp = |i: usize| matches!(f.get(i).map(String::as_str), Some("que" | "si" | "that" | "if" | "whether"));
            if comp(i) {
                i += 1;
            } else if f[i] == "a" && name_at(&f, i + 1, user).is_some() {
                i += 1 + name_at(&f, i + 1, user).map_or(0, |(_, m)| m);
                if comp(i) {
                    i += 1;
                }
            } else if let Some((_, m)) = name_at(&f, i, user).filter(|&(_, m)| comp(i + m)) {
                // «tells Kal'tsit that …»
                i += m + 1;
            }
            if i < ws.len() {
                return Framed { x: ws[i..].join(" "), frame: Some((who, f[n].clone())) };
            }
        }
    }
    Framed { x: s.to_string(), frame: None }
}

/// Whose experience a text is: (who, cognitive verb?). `frame_counts`: also accept the frame's own
/// subject when the remainder names nobody (a statement by Kal'tsit is hers; for Ana only
/// his preferences use it).
fn subject_of(p: &Framed, frame_counts: &dyn Fn(Who) -> bool, user: &str) -> Option<(Who, bool)> {
    // Asking, ordering or wishing says nothing about anyone's experience.
    if let Some((_, v)) = &p.frame {
        if DIRECTIVE_VERBS.contains(&v.as_str()) {
            return None;
        }
    }
    let ws: Vec<&str> = p.x.split_whitespace().collect();
    let f: Vec<String> = ws.iter().map(|w| text::fold(w).trim().to_string()).collect();
    let word = |i: usize| ws.get(i).map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase());
    let fw = |i: usize| f.get(i).map(String::as_str);
    let cognitive_after = |i: usize| fw(i).is_some_and(|v| COGNITIVE_VERBS.contains(&v)) || (fw(i) == Some("se") && fw(i + 1) == Some("da") && fw(i + 2) == Some("cuenta"));
    // «Ana …», «Kal'tsit …» (the verb after the name tells if she thinks or lives).
    if let Some((who, n)) = name_at(&f, 0, user) {
        return Some((who, who == Who::Kaltsit && cognitive_after(n)));
    }
    // «A Ana le gusta…»
    if fw(0) == Some("a") {
        if let Some((who, n)) = name_at(&f, 1, user) {
            if fw(1 + n) == Some("le") {
                return Some((who, false));
            }
        }
    }
    // «él …» (with its accent: without it, «el» is the article). English has no pronoun that
    // can be told from a third person («he» may be anyone), so there is no equivalent.
    if word(0).as_deref() == Some("él") {
        return Some((Who::User, false));
    }
    if let Some((Who::User, _)) = &p.frame {
        // The user speaking of himself: «se encuentra…», «va a…», «está tomando…», «ya logró…»;
        // «is working…», «is going to…», «feels…», «already finished…».
        let own = (fw(0) == Some("se") && matches!(fw(1), Some("encuentra" | "siente")))
            || (fw(0) == Some("va") && fw(1) == Some("a"))
            || (fw(0) == Some("esta") && fw(1).is_some_and(|v| v.ends_with("ando") || v.ends_with("iendo")))
            || (fw(0) == Some("ya") && word(1).is_some_and(|v| v.ends_with('ó')))
            || (matches!(fw(0), Some("is" | "was")) && fw(1).is_some_and(|v| v.ends_with("ing")))
            || fw(0) == Some("feels")
            || (fw(0) == Some("already") && fw(1).is_some_and(|v| v.ends_with("ed")));
        if own {
            return Some((Who::User, false));
        }
    }
    if let Some((who, verb)) = &p.frame {
        if frame_counts(*who) {
            return Some((*who, *who == Who::Kaltsit && COGNITIVE_VERBS.contains(&verb.as_str())));
        }
    }
    None
}

/// The first word that ends like a code file (.rs, .py…), as written.
fn code_file(s: &str) -> Option<String> {
    s.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric())).find_map(|w| {
        let l = w.to_lowercase();
        CODE_EXT.iter().any(|e| l.len() > e.len() && l.ends_with(e)).then(|| w.to_string())
    })
}

/// What the sorting rule says about one text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The sector key, or `POR_CLASIFICAR` when it found no clear place.
    pub key: &'static str,
    /// The rule that decided, in words (for the sorting rule: the keywords that matched).
    pub why: String,
    /// When `key` is `POR_CLASIFICAR` because SEVERAL topical sectors had words, which ones.
    /// Empty in every other case: nothing contradicts a proposal made by someone else.
    pub ambiguous: Vec<&'static str>,
}

impl Placement {
    /// A clear sector was found.
    pub fn is_clear(&self) -> bool {
        self.key != POR_CLASIFICAR
    }
}

/// The sorting rule, in this order (all of it for the nodes that reach the word lists):
/// A. the reporting frame («Ana afirma que…») comes off and the rest, X, is what is sorted;
/// B. topic first: exactly one topical sector with words in X → there; several → «Por clasificar»;
/// C. no topic: the subject of X (Ana → Personas; Kal'tsit → Experiencias, or Opiniones if she
///    thinks); no clear subject → «Por clasificar»;
/// D. preferences are Preferencias only when they are hers; Ana's go to Personas.
pub fn classify(kind: NodeKind, label: &str, body: &str, user: &str) -> Placement {
    let (pl, pb) = (strip_frame(label, user), strip_frame(body, user));
    let x = format!("{} {}", pl.x, pb.x);
    let folded = text::fold(&x);
    let toks = text::form_set(&x);
    let exact: HashSet<&str> = folded.split_whitespace().collect();
    let mut hits: Vec<(&'static str, Vec<String>)> = Vec::new();
    for (sector, words) in KEYWORDS {
        let mut found: Vec<String> = Vec::new();
        for w in words {
            // «=word» is that exact word; several words are a whole phrase; one word matches
            // under any of its forms (light stemming, Spanish and English).
            let ok = if let Some(e) = w.strip_prefix('=') {
                exact.contains(e)
            } else if w.contains(' ') {
                text::contains_phrase(&folded, w)
            } else {
                let w = text::fold(w);
                text::forms(w.trim()).iter().any(|f| toks.contains(f))
            };
            if ok {
                found.push(w.trim_start_matches('=').to_string());
            }
        }
        if kind == NodeKind::Place && sector == "lugares" {
            found.push("(es un lugar)".into());
        }
        if sector == "tecnologia" {
            if let Some(f) = code_file(&x) {
                found.push(format!("({f})"));
            }
        }
        if !found.is_empty() {
            hits.push((sector, found));
        }
    }
    // The user as the reasons name them: the name passed in (generic only when there is none).
    let user_name = if user.trim().is_empty() { "el usuario" } else { user.trim() };
    let framed = pl.frame.as_ref().or(pb.frame.as_ref()).map(|(w, v)| format!(" (marco «{} {v}» quitado)", if *w == Who::User { user_name } else { "Kal'tsit" })).unwrap_or_default();
    let place = |key: &'static str, why: String| Placement { key, why, ambiguous: Vec::new() };
    // B. Preferences are not a topic: they only decide when nothing else does (D).
    let topical: Vec<&(&'static str, Vec<String>)> = hits.iter().filter(|(s, _)| *s != "preferencias").collect();
    match topical.len() {
        1 => {
            let (s, w) = topical[0];
            return place(s, format!("{s}: {}{framed}", w.join(", ")));
        }
        0 => {}
        _ => {
            let all: Vec<String> = topical.iter().map(|(s, w)| format!("{s} [{}]", w.join(", "))).collect();
            return Placement { key: POR_CLASIFICAR, why: format!("ambiguo: {}{framed}", all.join(" / ")), ambiguous: topical.iter().map(|(s, _)| *s).collect() };
        }
    }
    let pref = hits.iter().any(|(s, _)| *s == "preferencias");
    // C/D. Kal'tsit's own statements are hers; Ana's only count when they are preferences.
    let counts = |w: Who| w == Who::Kaltsit || pref;
    let subj = subject_of(&pl, &counts, user).or_else(|| subject_of(&pb, &counts, user));
    match (subj, pref) {
        (Some((Who::User, _)), _) => place("personas", format!("sujeto {user_name}{}{framed}", if pref { ", su preferencia" } else { "" })),
        (Some((Who::Kaltsit, _)), true) => place("preferencias", format!("preferencia de Kal'tsit{framed}")),
        (Some((Who::Kaltsit, true)), false) => place("opiniones", format!("sujeto Kal'tsit con verbo cognitivo{framed}")),
        (Some((Who::Kaltsit, false)), false) => place("experiencias", format!("sujeto Kal'tsit{framed}")),
        (None, true) => place(POR_CLASIFICAR, format!("preferencia sin sujeto claro{framed}")),
        (None, false) => place(POR_CLASIFICAR, format!("sin palabras clave de ningún sector ni sujeto claro{framed}")),
    }
}

// ── the tree ──

/// The name of the user this memory lives with (its meta `usuario`); empty if it records none.
pub fn user_of(m: &Memory) -> &str {
    m.meta("usuario").unwrap_or("")
}

/// Whether the memory has a tree of sectors. Without it nothing here places anything.
pub fn has_sectors(m: &Memory) -> bool {
    m.nodes.iter().any(|n| n.kind == NodeKind::Sector)
}

/// The sector with this key (a node of another kind with the same key is not one).
pub fn sector_by_key(m: &Memory, key: &str) -> Option<u32> {
    m.nodes.iter().find(|n| n.kind == NodeKind::Sector && n.key == key).map(|n| n.id)
}

fn belonging(e: &Edge) -> f32 {
    let total = e.alpha + e.beta;
    if total > 0.0 {
        e.alpha / total
    } else {
        0.0
    }
}

/// The sectors `id` belongs to, with how much, in edge order.
pub fn parents_of(m: &Memory, id: u32) -> Vec<(u32, f32)> {
    m.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo && e.from == id && m.node(e.to).is_some_and(|n| n.kind == NodeKind::Sector)).map(|e| (e.to, belonging(e))).collect()
}

/// The sector `id` belongs to the most (the first one on a tie).
pub(crate) fn primary_parent(m: &Memory, id: u32) -> Option<u32> {
    let mut best: Option<(u32, f32)> = None;
    for (s, w) in parents_of(m, id) {
        if best.is_none_or(|(_, bw)| w > bw) {
            best = Some((s, w));
        }
    }
    best.map(|(s, _)| s)
}

/// Whether `s` is `ancestor` or hangs, at any depth, from it.
pub(crate) fn is_under(m: &Memory, s: u32, ancestor: u32) -> bool {
    let mut seen = HashSet::new();
    let mut todo = vec![s];
    while let Some(x) = todo.pop() {
        if x == ancestor {
            return true;
        }
        if seen.insert(x) {
            todo.extend(parents_of(m, x).into_iter().map(|(p, _)| p));
        }
    }
    false
}

/// Steps from the root of the tree (the sector that belongs to none) down to `s`.
pub(crate) fn depth(m: &Memory, s: u32) -> usize {
    let mut d = 0;
    let mut cur = s;
    while let Some(p) = primary_parent(m, cur) {
        d += 1;
        cur = p;
        if d > 32 {
            break;
        }
    }
    d
}

pub(crate) fn is_structural(m: &Memory, s: u32) -> bool {
    m.node(s).is_some_and(|n| STRUCTURAL.contains(&n.key.as_str()))
}

/// The thematic sector of a node: the one it belongs to the most, leaving out the structural ones.
fn thematic_sector(m: &Memory, id: u32) -> Option<u32> {
    let mut best: Option<(u32, f32)> = None;
    for (s, w) in parents_of(m, id) {
        if !is_structural(m, s) && best.is_none_or(|(_, bw)| w > bw) {
            best = Some((s, w));
        }
    }
    best.map(|(s, _)| s)
}

/// The fictional namesake of someone real (target of «inspirada_en»): it does not stand for the
/// border into fiction, as in recall.
fn is_namesake(m: &Memory, id: u32) -> bool {
    m.edges.iter().any(|e| e.kind == EdgeKind::InspiredBy && e.to == id)
}

/// The message crosses into fiction: it names (label or alias) a person, place, object or event
/// that lives under «ficticio» (not the namesake of herself), or says the name of a sector inside
/// fiction. Same border as recall (`retrieve::sector_view`), seen from the entities that the
/// text already resolved to nodes.
pub fn crosses_fiction(m: &Memory, mentioned: &[u32], said: &str) -> bool {
    let Some(f) = sector_by_key(m, retrieve::FICTION_KEY) else { return false };
    let folded = text::fold(said);
    if m.nodes.iter().any(|n| n.kind == NodeKind::Sector && is_under(m, n.id, f) && text::contains_phrase(&folded, &n.label)) {
        return true;
    }
    mentioned.iter().any(|&e| {
        m.node(e).is_some_and(|n| (n.kind.is_entity() || n.kind == NodeKind::Event) && !is_namesake(m, e))
            && parents_of(m, e).iter().any(|&(s, _)| is_under(m, s, f))
    })
}

/// The base sectors the analyst may propose: the ones right under the macro-sectors (depth 2),
/// minus the ones the code or her own judgement fills (`NOT_PROPOSED`). Fiction only when the
/// message `crossed` into it. (key, label), in tree order.
pub fn proposable(m: &Memory, crossed: bool) -> Vec<(String, String)> {
    let fiction = sector_by_key(m, retrieve::FICTION_KEY);
    m.nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Sector && !n.key.is_empty() && !NOT_PROPOSED.contains(&n.key.as_str()))
        .filter(|n| depth(m, n.id) == 2)
        .filter(|n| crossed || !fiction.is_some_and(|f| is_under(m, n.id, f)))
        .map(|n| (n.key.clone(), n.label.clone()))
        .collect()
}

// ── placement ──

/// Gives `node` its belonging to `sector`.
pub fn attach(m: &mut Memory, node: u32, sector: u32) {
    m.upsert_edge(node, sector, EdgeKind::BelongsTo, FULL.0, FULL.1);
}

/// The inbox, if the memory has one.
fn pending(m: &Memory) -> Option<u32> {
    sector_by_key(m, POR_CLASIFICAR)
}

/// A structural sector under `parent_key`: the one that exists, or a new one that copies its
/// parent (as the import always did). `None` if the parent does not exist or the key is taken by
/// something that is not a sector.
pub fn ensure_structural(m: &mut Memory, key: &str, label: &str, body: &str, parent_key: &str) -> Option<u32> {
    if let Some(s) = sector_by_key(m, key) {
        return Some(s);
    }
    if m.by_key(key).is_some() {
        return None;
    }
    let parent = sector_by_key(m, parent_key)?;
    let mut sec = m.nodes[parent as usize].clone();
    sec.key = key.into();
    sec.label = label.into();
    sec.text = body.into();
    sec.aliases.clear();
    let id = m.add_node(sec);
    attach(m, id, parent);
    Some(id)
}

/// Files `node` under the sector with this key, or in the inbox if the memory has no such sector.
/// Does nothing in a memory without sectors. Returns the sector it went to.
pub fn file_under(m: &mut Memory, node: u32, key: &str) -> Option<u32> {
    if !has_sectors(m) {
        return None;
    }
    let s = sector_by_key(m, key).or_else(|| pending(m))?;
    attach(m, node, s);
    Some(s)
}

/// A new episode: conversations go to «Conversaciones» (the sector is made if missing), what she
/// saw and her own comments to «Lo que vio». Both are structural, under «Experiencias». Does
/// nothing in a memory without sectors.
pub fn file_episode(m: &mut Memory, ep: u32, label: &str) -> Option<u32> {
    if !has_sectors(m) {
        return None;
    }
    // Created in the language the memory was born in, with the name of its user.
    let l = Lang::of_memory(m);
    let user = match user_of(m) {
        "" => super::seed::placeholder_user(l.code()).to_string(),
        u => u.to_string(),
    };
    let s = match episode_kind(label) {
        EpisodeKind::Conversation => ensure_structural(m, CONVERSACIONES, lang::conversations_label(l), &lang::conversations_text(l, &user), EXPERIENCIAS),
        EpisodeKind::Screen | EpisodeKind::Own => ensure_structural(m, LO_QUE_VIO, lang::saw_label(l), &lang::saw_text(l, &user), EXPERIENCIAS),
    };
    let s = s.or_else(|| pending(m))?;
    attach(m, ep, s);
    Some(s)
}

/// A node made out of an existing one (the denial of a canon fact): it lives where that one lives,
/// or in the inbox if that one has no sector. `None` in a memory without sectors.
pub fn home_like(m: &Memory, existing: u32) -> Option<u32> {
    if !has_sectors(m) {
        return None;
    }
    thematic_sector(m, existing).or_else(|| pending(m))
}

/// A person or thing named for the first time. A person → Personas; the rest, the sorting rule
/// over its name and what the analyst said it is (`que`); else the inbox. `None` in a memory
/// without sectors.
pub fn home_of_entity(m: &Memory, person: bool, name: &str, que: &str, user: &str) -> Option<u32> {
    if !has_sectors(m) {
        return None;
    }
    if person {
        return sector_by_key(m, "personas").or_else(|| pending(m));
    }
    let p = classify(NodeKind::Object, name, que, user);
    if p.is_clear() {
        if let Some(s) = sector_by_key(m, p.key) {
            return Some(s);
        }
    }
    pending(m)
}

/// What the analyst and the caller say about one claim, besides its text.
#[derive(Clone, Debug, Default)]
pub struct Hint {
    /// The sector the analyst proposed (a key). Only accepted by the last resort of the decision.
    pub proposal: Option<String>,
    /// A sector decided by whoever calls (the reflection puts its conclusions in `opiniones`).
    pub fixed: Option<&'static str>,
    /// Who said it (names). Their own node never anchors the sector: Ana is in every fact they
    /// state, and that says nothing of what the fact is about.
    pub speaker: Vec<String>,
    /// Tokens of the framing words (the user's name, the agent's, reporting verbs): they say who
    /// speaks or how, never what, so they do not count when deciding that a claim repeats a
    /// memory. Empty: no framing known, plain similarity.
    pub frame: Vec<String>,
}

/// Which step of the decision placed a fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The caller fixed it.
    Fixed,
    /// (a) it inherits the sector of the entities it mentions.
    Entity,
    /// (b) the shared sorting rule.
    Classifier,
    /// (c) the analyst's proposal, accepted.
    Proposal,
    /// (d) nothing decided: the inbox.
    Pending,
}

/// Where a new fact goes, in this order:
/// a. the entities it mentions that already have a thematic sector, if they all agree: that one
///    (the speaker, and the namesake of herself in fiction, do not count; the sectors of the
///    inbox and the structural ones are not a subject);
/// b. else the shared sorting rule, if it finds one clear sector of this memory;
/// c. else the analyst's proposal, only if it is a sector it may propose (`proposable`, fiction
///    only if the fact crosses into it) and it does not contradict (b): when (b) found words of
///    several sectors, the proposal must be one of them;
/// d. else «Por clasificar».
///
/// `None` if the memory has no sectors (or not even an inbox for the last resort). `ents` are the
/// entity nodes the fact mentions; `skip` the ones that do not anchor (the speaker).
///
/// Whatever sector a step picks (all but the inbox), the fact then descends into the subsectors
/// she created under it while one of them fits (`subsectors::descend`, design §7).
pub fn home_of_fact(m: &Memory, ents: &[u32], skip: &[u32], body: &str, hint: Option<&Hint>) -> Option<(u32, Step)> {
    let (s, step) = decide(m, ents, skip, body, hint)?;
    Some(if step == Step::Pending { (s, step) } else { (subsectors::descend(m, s, body, hint.map_or(&[], |h| h.frame.as_slice())), step) })
}

fn decide(m: &Memory, ents: &[u32], skip: &[u32], body: &str, hint: Option<&Hint>) -> Option<(u32, Step)> {
    if !has_sectors(m) {
        return None;
    }
    if let Some(key) = hint.and_then(|h| h.fixed) {
        return sector_by_key(m, key).map(|s| (s, Step::Fixed)).or_else(|| pending(m).map(|s| (s, Step::Pending)));
    }
    // a.
    let fiction = sector_by_key(m, retrieve::FICTION_KEY);
    let mut found: Vec<u32> = Vec::new();
    for &e in ents.iter().filter(|e| !skip.contains(e)) {
        // The core hangs from the root and the macro-sectors are too wide to be a subject.
        let Some(s) = thematic_sector(m, e).filter(|&s| depth(m, s) >= 2) else { continue };
        if fiction.is_some_and(|f| is_under(m, s, f)) && is_namesake(m, e) {
            continue;
        }
        found.push(s);
    }
    if let Some(&first) = found.first() {
        if found.iter().all(|&s| s == first) {
            return Some((first, Step::Entity));
        }
    }
    // b.
    // Who speaks: the one who said it, else the user the memory records.
    let user = hint.and_then(|h| h.speaker.first()).map(String::as_str).unwrap_or_else(|| user_of(m));
    let p = classify(NodeKind::Fact, body, body, user);
    if p.is_clear() {
        if let Some(s) = sector_by_key(m, p.key) {
            return Some((s, Step::Classifier));
        }
    }
    // c.
    if let Some(key) = hint.and_then(|h| h.proposal.as_deref()).map(|k| k.trim().to_lowercase()) {
        let allowed = proposable(m, crosses_fiction(m, ents, body)).iter().any(|(k, _)| *k == key);
        let contradicts = !p.ambiguous.is_empty() && !p.ambiguous.contains(&key.as_str());
        if allowed && !contradicts {
            if let Some(s) = sector_by_key(m, &key) {
                return Some((s, Step::Proposal));
            }
        }
    }
    // d.
    pending(m).map(|s| (s, Step::Pending))
}
