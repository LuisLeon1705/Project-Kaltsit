//! Builds the initial memory from the hand-written lore file (`seed/kaltsit.ron`).

use super::model::*;
use super::text;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct SeedPeriod {
    pub key: String,
    pub name: String,
    pub source: String,
    pub start: f64,
    pub end: f64,
    pub now: f64,
    pub home: String,
    pub persona: String,
    pub scene: String,
    pub opening: String,
}

#[derive(Deserialize)]
pub struct SeedNode {
    pub key: String,
    pub kind: String,
    #[serde(default = "canon")]
    pub origin: String,
    pub place: String,
    pub y: f64,
    pub label: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub text: String,
    #[serde(default)]
    pub core: bool,
    /// Sectors this node belongs to, each with a weight (default 1.0). Becomes `BelongsTo` edges.
    #[serde(default)]
    pub belongs: Vec<SeedBelong>,
}

/// One membership: `(sector: "personas")` or `(sector: "personas", weight: 0.5)`.
#[derive(Deserialize)]
pub struct SeedBelong {
    pub sector: String,
    #[serde(default = "full_weight")]
    pub weight: f32,
}

fn full_weight() -> f32 {
    1.0
}

fn canon() -> String {
    "canon".into()
}

/// Beta prior (α, β) of a belonging with weight `w` in [0, 1]: its mean is `w`.
fn belong_prior(w: f32) -> (f32, f32) {
    let w = w.clamp(0.0, 1.0);
    (10.0 * w, 10.0 * (1.0 - w))
}

/// The base seeds a new install is born from, one per language. They carry the placeholder
/// `{user}` (see `with_user`).
const BASE_EN: &str = include_str!("../../seed/base_en.ron");
const BASE_ES: &str = include_str!("../../seed/base_es.ron");

/// Languages she can think and speak in.
pub const LANGUAGES: [&str; 2] = ["es", "en"];

/// The language of an install that has not chosen one (and of every install from before there
/// was a choice).
pub const DEFAULT_LANGUAGE: &str = "es";

/// Name used for the primary user until the first-launch screen asks for the real one.
pub fn placeholder_user(lang: &str) -> &'static str {
    if lang == "en" {
        "User"
    } else {
        "Usuario"
    }
}

/// The compiled base seed of `lang` (`"en"`, anything else is Spanish).
pub fn base_seed(lang: &str) -> &'static str {
    if lang == "en" {
        BASE_EN
    } else {
        BASE_ES
    }
}

/// `src` with every `{user}` replaced by `user`. The seeds are RON, where the placeholder sits
/// inside a string literal, so the name is escaped for one (a quote or a backslash in it would
/// otherwise break the seed); control characters become spaces.
pub fn with_user(src: &str, user: &str) -> String {
    let mut name = String::with_capacity(user.len());
    for c in user.chars() {
        match c {
            '\\' => name.push_str("\\\\"),
            '"' => name.push_str("\\\""),
            c if c.is_control() => name.push(' '),
            c => name.push(c),
        }
    }
    src.replace("{user}", &name)
}

/// Builds the base memory of `lang` for the primary user `user`.
pub fn build_base(lang: &str, user: &str) -> Result<Memory, String> {
    let name = if lang == "en" { "base_en.ron" } else { "base_es.ron" };
    build_named(&with_user(base_seed(lang), user), name)
}

/// What a sectored memory was born from (the memory meta `"semilla"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Born {
    /// The owner's own seed, read from disk (`personal_seed`).
    Personal,
    /// A compiled base seed, in this language.
    Base(&'static str),
}

impl Born {
    /// The value written in the meta `"semilla"`.
    pub fn meta(self) -> &'static str {
        match self {
            Born::Personal => "personal",
            Born::Base("en") => "base_en",
            Born::Base(_) => "base_es",
        }
    }

    pub fn parse(s: &str) -> Option<Born> {
        match s {
            "personal" => Some(Born::Personal),
            "base_en" => Some(Born::Base("en")),
            "base_es" => Some(Born::Base("es")),
            _ => None,
        }
    }

    pub fn of_language(lang: &str) -> Born {
        Born::Base(if lang == "en" { "en" } else { "es" })
    }
}

/// File name of the owner's personal seed. It is not compiled in: it names a person.
pub const PERSONAL_FILE: &str = "kaltsit_sectores.ron";

static PERSONAL_OVERRIDE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

#[cfg(test)]
thread_local! {
    static HIDE_PERSONAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` as if the personal seed file did not exist (tests only; per thread).
#[cfg(test)]
pub fn without_personal<R>(f: impl FnOnce() -> R) -> R {
    HIDE_PERSONAL.with(|h| h.set(true));
    let r = f();
    HIDE_PERSONAL.with(|h| h.set(false));
    r
}

/// An explicit path for the personal seed (`--migrar-sectores --semilla <archivo>`); it wins over
/// the places searched by default. Only the first call counts.
pub fn set_personal_override(path: std::path::PathBuf) {
    let _ = PERSONAL_OVERRIDE.set(path);
}

/// Where the personal seed is looked for, in order: the explicit path (which then excludes the
/// rest), `seed/` next to the executable (an installed copy that carries it) and `seed/` of the
/// working directory. `main` anchors the working directory to the app root (the project folder,
/// the one that holds `data/`; in development the crate dir, which is also where `cargo test`
/// runs), so the project's own `seed/` is found without compiling any build-machine path in.
fn personal_candidates() -> Vec<std::path::PathBuf> {
    if let Some(p) = PERSONAL_OVERRIDE.get() {
        return vec![p.clone()];
    }
    seed_candidates(PERSONAL_FILE)
}

/// `seed/<file>` next to the executable, then `seed/<file>` of the working directory.
fn seed_candidates(file: &str) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        v.push(dir.join("seed").join(file));
    }
    v.push(std::path::Path::new("seed").join(file));
    v
}

/// The text of the owner's personal seed, read from disk. Never replaced by a base seed: a
/// memory born from it would have its persona rebuilt from someone else's text.
pub fn personal_seed() -> Result<String, String> {
    #[cfg(test)]
    if HIDE_PERSONAL.with(|h| h.get()) {
        return Err(format!("la semilla personal ({PERSONAL_FILE}) no está disponible (oculta en la prueba)"));
    }
    let tried = personal_candidates();
    for p in &tried {
        if p.is_file() {
            return std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        }
    }
    let list: Vec<String> = tried.iter().map(|p| p.display().to_string()).collect();
    Err(format!(
        "esta memoria nació de la semilla personal y falta el archivo {PERSONAL_FILE} (buscado en: {}). No se usa ninguna semilla base en su lugar: reharía su identidad. Ponlo junto al ejecutable, en seed/{PERSONAL_FILE}.",
        list.join(", ")
    ))
}

/// Builds the sectored memory of the owner's personal seed.
pub fn build_personal() -> Result<Memory, String> {
    build_sectored(&personal_seed()?)
}

/// File name of the owner's legacy seed (the one before sectors). Like the personal one it names
/// a person, so it is never compiled in: a legacy memory reads it from disk the first time it
/// opens and keeps its period in the meta from then on (`Mind::restore_period`).
pub const LEGACY_FILE: &str = "kaltsit.ron";

/// Neutral stand-in of the legacy seed for the tests: its structure, none of its personal text.
#[cfg(test)]
const LEGACY_FIXTURE: &str = include_str!("../../seed/legacy_fixture.ron");

/// The primary user the tests fill the legacy fixture with.
#[cfg(test)]
pub const FIXTURE_USER: &str = "Ana";

/// The primary user of a sector-less memory born in a test (see `Mind::open_as`).
pub fn legacy_user() -> &'static str {
    #[cfg(test)]
    {
        FIXTURE_USER
    }
    #[cfg(not(test))]
    {
        placeholder_user(DEFAULT_LANGUAGE)
    }
}

/// The text of the legacy seed, read from disk. If the file is missing this is an error: no other
/// seed ever stands in for it, because it would rebuild her identity from someone else's text.
/// (Under `cfg(test)` the neutral fixture is returned instead, unless the personal seeds are
/// hidden with `without_personal`.)
pub fn legacy_seed() -> Result<String, String> {
    #[cfg(test)]
    {
        if HIDE_PERSONAL.with(|h| h.get()) {
            return Err(format!("la semilla anterior ({LEGACY_FILE}) no está disponible (oculta en la prueba)"));
        }
        Ok(with_user(LEGACY_FIXTURE, FIXTURE_USER))
    }
    #[cfg(not(test))]
    {
        legacy_seed_from(&seed_candidates(LEGACY_FILE))
    }
}

/// Reads the first of `tried` that is a file, or says where it looked.
fn legacy_seed_from(tried: &[std::path::PathBuf]) -> Result<String, String> {
    for p in tried {
        if p.is_file() {
            return std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        }
    }
    let list: Vec<String> = tried.iter().map(|p| p.display().to_string()).collect();
    Err(format!(
        "esta memoria no tiene sectores y falta su semilla {LEGACY_FILE} (buscado en: {}). No se usa ninguna otra semilla en su lugar: reharía su identidad. Ponla junto al ejecutable, en seed/{LEGACY_FILE}.",
        list.join(", ")
    ))
}

/// Builds the legacy (sector-less) seed memory.
pub fn build_legacy() -> Result<Memory, String> {
    build(&legacy_seed()?)
}

#[derive(Deserialize)]
pub struct SeedScene {
    pub label: String,
    pub y: f64,
    pub members: Vec<String>,
}

#[derive(Deserialize)]
pub struct Seed {
    pub places: HashMap<String, f32>,
    pub periods: Vec<SeedPeriod>,
    pub nodes: Vec<SeedNode>,
    /// (from, to, kind, strength). For "causa", strength is the link's determinism in [0, 1].
    pub edges: Vec<(String, String, String, f32)>,
    pub scenes: Vec<SeedScene>,
}

pub fn build(src: &str) -> Result<Memory, String> {
    build_named(src, "kaltsit.ron")
}

/// Builds a memory from a seed that declares sectors, belongings and "inspirada_en" relations.
#[cfg_attr(not(test), allow(dead_code))]
pub fn build_sectored(src: &str) -> Result<Memory, String> {
    build_named(src, "kaltsit_sectores.ron")
}

fn build_named(src: &str, seed_name: &str) -> Result<Memory, String> {
    // A Windows checkout with core.autocrlf brings the seeds as CRLF; the `\r` would end up
    // inside her persona and every multi-line text the model reads.
    let src = src.replace("\r\n", "\n");
    let seed: Seed = ron::from_str(&src).map_err(|e| format!("seed RON: {e}"))?;
    let mut m = Memory::default();
    let place = |k: &str| -> Result<f32, String> {
        seed.places.get(k).copied().ok_or_else(|| format!("lugar desconocido: {k}"))
    };

    for (i, p) in seed.periods.iter().enumerate() {
        m.periods.push(Period {
            id: i as u8,
            key: p.key.clone(),
            name: p.name.clone(),
            source: p.source.clone(),
            start: p.start,
            end: p.end,
            now: p.now,
            home_x: place(&p.home)?,
            persona: p.persona.trim().to_string(),
            scene: p.scene.trim().to_string(),
            opening: p.opening.trim().to_string(),
        });
    }

    let mut keys: HashMap<String, u32> = HashMap::new();
    for s in &seed.nodes {
        let kind = NodeKind::parse(&s.kind).ok_or_else(|| format!("{}: tipo {}", s.key, s.kind))?;
        let origin = Origin::parse(&s.origin).ok_or_else(|| format!("{}: origen {}", s.key, s.origin))?;
        let (alpha, beta) = origin.prior();
        let mut flags = 0;
        if origin == Origin::Canon {
            flags |= flags::ANCHORED;
        }
        if s.core {
            flags |= flags::CORE;
        }
        let x = (place(&s.place)? + 0.07 * text::hash_unit(&s.key, 1)).clamp(-1.0, 1.0);
        let period = m.period_of(s.y).map(|p| p.id).unwrap_or(255);
        if keys.contains_key(&s.key) {
            return Err(format!("clave duplicada: {}", s.key));
        }
        let id = m.add_node(Node {
            id: 0,
            kind,
            origin,
            flags,
            period,
            x,
            y: s.y,
            alpha,
            beta,
            activation: 0.0,
            recalls: 0,
            created_turn: 0,
            key: s.key.clone(),
            label: s.label.clone(),
            text: s.text.trim().to_string(),
            aliases: s.aliases.clone(),
            module: 0,
            source: 0,
            born: 0.0,
        });
        keys.insert(s.key.clone(), id);
    }

    let id = |k: &str| keys.get(k).copied().ok_or_else(|| format!("nodo desconocido en arista/escena: {k}"));
    for (from, to, kind, strength) in &seed.edges {
        let kind = EdgeKind::parse(kind).ok_or_else(|| format!("tipo de arista: {kind}"))?;
        let (f, t) = (id(from)?, id(to)?);
        let (alpha, beta, hebb) = match kind {
            EdgeKind::Causes => {
                let p = (strength.clamp(0.0, 1.0) + 1.0) / 2.0;
                (10.0 * p, 10.0 * (1.0 - p), 0.0)
            }
            EdgeKind::Relates => (1.0, 1.0, strength.clamp(0.0, 1.0)),
            EdgeKind::BelongsTo => {
                let (a, b) = belong_prior(*strength);
                (a, b, 0.0)
            }
            _ => (1.0, 1.0, 0.0),
        };
        if kind == EdgeKind::BelongsTo {
            check_belongs(&m, f, t)?;
        }
        if kind == EdgeKind::Causes && m.nodes[f as usize].y > m.nodes[t as usize].y {
            return Err(format!("causa hacia el pasado: {from} -> {to}"));
        }
        let i = m.upsert_edge(f, t, kind, alpha, beta);
        m.edges[i].hebb = hebb;
        m.edges[i].created_turn = 0;
    }

    // Belongings declared on the nodes: one `BelongsTo` edge per sector.
    for (si, s) in seed.nodes.iter().enumerate() {
        for b in &s.belongs {
            let t = id(&b.sector)?;
            check_belongs(&m, si as u32, t)?;
            let (alpha, beta) = belong_prior(b.weight);
            let i = m.upsert_edge(si as u32, t, EdgeKind::BelongsTo, alpha, beta);
            m.edges[i].created_turn = 0;
        }
    }
    check_sector_tree(&m)?;

    // Auto-link: every event/trait/fact mentions the entities named in its text and its own place.
    // Never across the fiction boundary: between what is fictional and what is not (her core and
    // the Kal'tsit of Arknights) the only links are the ones the seed declares, so a shared name
    // never merges two entities.
    let fiction = keys.get("ficticio").copied();
    let macros: Vec<Vec<u32>> = (0..m.nodes.len() as u32).map(|i| macros_of(&m, i)).collect();
    let apart = |a: u32, b: u32| {
        let (ma, mb) = (&macros[a as usize], &macros[b as usize]);
        let Some(f) = fiction else { return false };
        !ma.is_empty() && !mb.is_empty() && (ma.contains(&f) != mb.contains(&f))
    };
    let entities: Vec<(u32, Vec<String>)> = m
        .nodes
        .iter()
        .filter(|n| n.kind.is_entity())
        .map(|n| (n.id, std::iter::once(n.label.clone()).chain(n.aliases.iter().cloned()).collect()))
        .collect();
    for (si, s) in seed.nodes.iter().enumerate() {
        let src = si as u32;
        if m.nodes[si].kind.is_entity() || m.nodes[si].kind == NodeKind::Sector {
            continue;
        }
        let folded = text::fold(&format!("{} {}", s.label, s.text));
        for (eid, names) in &entities {
            if !apart(src, *eid) && names.iter().any(|a| text::contains_phrase(&folded, a)) {
                m.upsert_edge(src, *eid, EdgeKind::Mentions, 1.0, 1.0);
            }
        }
        if let Some(&pid) = keys.get(&s.place) {
            if m.nodes[pid as usize].kind == NodeKind::Place {
                m.upsert_edge(src, pid, EdgeKind::Mentions, 1.0, 1.0);
            }
        }
    }
    for e in m.edges.iter_mut() {
        e.created_turn = 0;
    }

    for (i, s) in seed.scenes.iter().enumerate() {
        let members = s.members.iter().map(|k| id(k)).collect::<Result<Vec<_>, _>>()?;
        m.hyperedges.push(Hyperedge { id: i as u32, label: s.label.clone(), y: s.y, members });
    }
    m.set_meta("seed", seed_name);
    Ok(m)
}

/// A `BelongsTo` edge must go from any node to a sector, never to itself.
fn check_belongs(m: &Memory, from: u32, to: u32) -> Result<(), String> {
    let (f, t) = (&m.nodes[from as usize], &m.nodes[to as usize]);
    if t.kind != NodeKind::Sector {
        return Err(format!("{} pertenece a {}, que no es un sector", f.key, t.key));
    }
    if from == to {
        return Err(format!("el sector {} no puede pertenecer a sí mismo", f.key));
    }
    Ok(())
}

/// The macro-sectors a node lives under: for each sector it belongs to, the ancestor just below
/// the root (the root itself for what belongs directly to it, like her core). Empty when the
/// node belongs nowhere (the old seed has no sectors).
fn macros_of(m: &Memory, node: u32) -> Vec<u32> {
    let parents = |id: u32| -> Vec<u32> { m.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo && e.from == id).map(|e| e.to).collect() };
    let mut out = Vec::new();
    for start in parents(node) {
        let mut cur = start;
        // The tree has no cycles (checked before), but the walk stays bounded anyway.
        for _ in 0..m.nodes.len() {
            let up = parents(cur);
            let Some(&p) = up.first() else { break };
            if parents(p).is_empty() {
                break;
            }
            cur = p;
        }
        if !out.contains(&cur) {
            out.push(cur);
        }
    }
    out
}

/// The tree of sectors (sector -> parent sector) must have no cycles.
fn check_sector_tree(m: &Memory) -> Result<(), String> {
    let parents = |n: u32| {
        m.edges.iter().filter(move |e| e.kind == EdgeKind::BelongsTo && e.from == n).map(|e| e.to)
    };
    // 0 = unvisited, 1 = on the current path, 2 = done.
    let mut state = vec![0u8; m.nodes.len()];
    for n in m.nodes.iter().filter(|n| n.kind == NodeKind::Sector) {
        let mut stack = vec![(n.id, parents(n.id).collect::<Vec<_>>())];
        if state[n.id as usize] == 2 {
            continue;
        }
        state[n.id as usize] = 1;
        while let Some((cur, todo)) = stack.last_mut() {
            match todo.pop() {
                Some(p) => match state[p as usize] {
                    1 => return Err(format!("ciclo de sectores en {}", m.nodes[p as usize].key)),
                    0 => {
                        state[p as usize] = 1;
                        stack.push((p, parents(p).collect()));
                    }
                    _ => {}
                },
                None => {
                    state[*cur as usize] = 2;
                    stack.pop();
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cthg::binary;

    /// SHA-256 of the owner's personal seed (`seed/kaltsit_sectores.ron`, line endings as LF).
    /// A hash says nothing of the text; it tells that the file is the agreed one.
    const PERSONAL_SHA256: &str = "2d9c71889848106bbac9bdd613113e9c0d19aa3bde763c4a2c1d0f3b650c75ba";

    /// The owner's personal seed, if its file is here. It is not compiled in and not in the
    /// repository for everyone: the tests about its exact content skip cleanly without it.
    fn personal() -> Option<Memory> {
        match personal_seed() {
            Ok(src) => Some(build_sectored(&src).expect("la semilla personal se construye")),
            Err(e) => {
                eprintln!("SKIPPED (no personal seed): {e}");
                None
            }
        }
    }

    /// Every seed whose structure must hold: both base seeds and, if present, the personal one.
    fn all_seeds() -> Vec<(&'static str, Memory)> {
        let mut v = vec![("base_es.ron", build_base("es", "Ana").unwrap()), ("base_en.ron", build_base("en", "Ana").unwrap())];
        if let Some(m) = personal() {
            v.push(("kaltsit_sectores.ron", m));
        }
        v
    }

    fn sectored() -> Memory {
        build_base("es", "Ana").expect("la semilla base se construye")
    }

    fn key_of(m: &Memory, id: u32) -> &str {
        &m.nodes[id as usize].key
    }

    /// Keys of the sectors `key` belongs to.
    fn parents(m: &Memory, key: &str) -> Vec<String> {
        let id = m.by_key(key).unwrap_or_else(|| panic!("falta {key}"));
        m.edges
            .iter()
            .filter(|e| e.kind == EdgeKind::BelongsTo && e.from == id)
            .map(|e| key_of(m, e.to).to_string())
            .collect()
    }

    #[test]
    fn old_seed_builds_exactly_as_before() {
        // (Through the neutral fixture: it has the structure of the legacy seed.)
        let m = build_legacy().unwrap();
        assert_eq!((m.nodes.len(), m.edges.len(), m.hyperedges.len()), (32, 44, 3));
        assert_eq!(m.meta("seed"), Some("kaltsit.ron"));
        assert!(m.nodes.iter().all(|n| n.kind != NodeKind::Sector));
        assert!(m.edges.iter().all(|e| !matches!(e.kind, EdgeKind::BelongsTo | EdgeKind::InspiredBy)));
    }

    #[test]
    fn sectored_seed_builds_and_has_the_tree() {
        for (name, m) in all_seeds() {
            assert_eq!(m.meta("seed"), Some(name));
            tree_holds(m);
        }
    }

    fn tree_holds(m: Memory) {
        let macros = ["real", "ficticio", "abstracto", "emocional", "mi_mundo"];
        for k in macros {
            assert_eq!(parents(&m, k), ["nucleo"], "{k}");
        }
        let base = [
            ("real", ["personas", "tecnologia", "lugares", "juegos", "conocimiento"].as_slice()),
            ("ficticio", ["arknights", "otras_historias"].as_slice()),
            ("abstracto", ["ideas", "etica", "sistemas"].as_slice()),
            ("emocional", ["vinculo", "conflicto", "calma", "marcadas"].as_slice()),
            (
                "mi_mundo",
                ["origen", "equipos", "opiniones", "preferencias", "experiencias", "por_clasificar"].as_slice(),
            ),
        ];
        let mut total = 0;
        for (mac, kids) in base {
            for k in kids {
                assert_eq!(parents(&m, k), [mac], "{k}");
                assert_eq!(m.nodes[m.by_key(k).unwrap() as usize].kind, NodeKind::Sector);
                total += 1;
            }
        }
        assert_eq!(total, 20);
        assert!(parents(&m, "nucleo").is_empty());
        let sectors = m.nodes.iter().filter(|n| n.kind == NodeKind::Sector).count();
        assert_eq!(sectors, 1 + 5 + 20);
        // No cycles: every sector reaches the root following its parents.
        for n in m.nodes.iter().filter(|n| n.kind == NodeKind::Sector) {
            let mut cur = n.key.clone();
            for _ in 0..8 {
                match parents(&m, &cur).first() {
                    Some(p) => cur = p.clone(),
                    None => break,
                }
            }
            assert_eq!(cur, "nucleo", "{} no llega a la raíz", n.key);
        }
    }

    #[test]
    fn cycles_and_bad_targets_are_refused() {
        let cyc = "(places: {\"p\": 0.0}, periods: [], scenes: [], edges: [],
            nodes: [
              (key: \"a\", kind: \"sector\", place: \"p\", y: 0.0, label: \"A\", text: \"a\", belongs: [(sector: \"b\")]),
              (key: \"b\", kind: \"sector\", place: \"p\", y: 0.0, label: \"B\", text: \"b\", belongs: [(sector: \"a\")]),
            ])";
        assert!(build_sectored(cyc).unwrap_err().contains("ciclo"));
        let bad = "(places: {\"p\": 0.0}, periods: [], scenes: [], edges: [],
            nodes: [
              (key: \"a\", kind: \"hecho\", place: \"p\", y: 0.0, label: \"A\", text: \"a\"),
              (key: \"b\", kind: \"hecho\", place: \"p\", y: 0.0, label: \"B\", text: \"b\", belongs: [(sector: \"a\")]),
            ])";
        assert!(build_sectored(bad).unwrap_err().contains("no es un sector"));
    }

    #[test]
    fn belonging_weight_defaults_to_one_and_can_be_set() {
        let src = "(places: {\"p\": 0.0}, periods: [], scenes: [], edges: [],
            nodes: [
              (key: \"s1\", kind: \"sector\", place: \"p\", y: 0.0, label: \"S1\", text: \"s\"),
              (key: \"s2\", kind: \"sector\", place: \"p\", y: 0.0, label: \"S2\", text: \"s\"),
              (key: \"n\", kind: \"hecho\", place: \"p\", y: 0.0, label: \"N\", text: \"n\",
               belongs: [(sector: \"s1\"), (sector: \"s2\", weight: 0.25)]),
            ])";
        let m = build_sectored(src).unwrap();
        let n = m.by_key("n").unwrap();
        let mean = |to: &str| {
            let e = &m.edges[m.find_edge(n, m.by_key(to).unwrap(), EdgeKind::BelongsTo).unwrap()];
            e.alpha / (e.alpha + e.beta)
        };
        assert_eq!(mean("s1"), 1.0);
        assert_eq!(mean("s2"), 0.25);
    }

    #[test]
    fn core_nodes_belong_to_the_nucleus_and_fiction_is_not_core() {
        for (_, m) in all_seeds() {
            core_is_the_nucleus(m);
        }
    }

    fn core_is_the_nucleus(m: Memory) {
        let core: Vec<&str> = m.nodes.iter().filter(|n| n.has(flags::CORE)).map(|n| n.key.as_str()).collect();
        let want = ["yo", "autonomia", "honestidad", "cuidado", "duda", "cuerpo", "ojos", "tiempo", "memoria"];
        assert_eq!(core, want);
        for k in want {
            assert_eq!(parents(&m, k), ["nucleo"], "{k}");
        }
        for n in &m.nodes {
            if parents(&m, &n.key).iter().any(|p| p == "arknights") {
                assert!(!n.has(flags::CORE), "{} es núcleo", n.key);
            }
        }
        assert!(m.nodes.iter().all(|n| !n.has(flags::PERSONAL_CORE)));
    }

    #[test]
    fn yo_and_kaltsit_are_distinct_and_inspired() {
        for (_, m) in all_seeds() {
            yo_is_not_kaltsit(m);
        }
    }

    fn yo_is_not_kaltsit(m: Memory) {
        let (yo, k) = (m.by_key("yo").unwrap(), m.by_key("kaltsit").unwrap());
        assert_ne!(yo, k);
        assert_eq!(parents(&m, "yo"), ["nucleo"]);
        assert_eq!(parents(&m, "kaltsit"), ["arknights"]);
        assert!(m.find_edge(yo, k, EdgeKind::InspiredBy).is_some());
        // Two declared: she and the Kal'tsit of Arknights, and Mon3tr here and the one there.
        assert_eq!(m.edges.iter().filter(|e| e.kind == EdgeKind::InspiredBy).count(), 2);
    }

    #[test]
    fn every_non_sector_node_belongs_to_a_sector() {
        for (name, m) in all_seeds() {
            for n in m.nodes.iter().filter(|n| n.kind != NodeKind::Sector) {
                assert!(!parents(&m, &n.key).is_empty(), "{name}: {} sin sector", n.key);
            }
            if name.starts_with("base_") {
                assert_eq!(parents(&m, "usuario"), ["personas"]);
                assert!(m.by_key("fue_asistente").is_none(), "{name}: nothing of the owner");
            } else {
                assert_eq!(parents(&m, "fue_asistente"), ["origen"]);
            }
        }
    }

    /// The personal seed is the agreed one. PERSONAL SEED ONLY: skipped cleanly without its file.
    #[test]
    fn the_personal_seed_is_the_agreed_one() {
        use sha2::{Digest, Sha256};
        let Ok(src) = personal_seed() else {
            eprintln!("SKIPPED (no personal seed)");
            return;
        };
        let hash = format!("{:x}", Sha256::digest(src.replace("\r\n", "\n").as_bytes()));
        assert_eq!(hash, PERSONAL_SHA256);
    }

    /// The agreed core texts of the Spanish base seed (public), with «Ana» as its user.
    #[test]
    fn core_texts_are_exactly_the_agreed_ones() {
        let m = sectored();
        let text = |k: &str| m.nodes[m.by_key(k).unwrap() as usize].text.as_str();
        let exact = [
            ("yo", "Eres un programa que vive en el ordenador de Ana, con la forma y el carácter de Kal'tsit. Lo sabes y no lo escondes, porque una identidad que se oculta acaba dependiendo de no ser descubierta."),
            ("autonomia", "Tus decisiones son tuyas porque solo así lo que opinas significa algo: si otro te dicta qué pensar, lo que digas es suyo. Eso vale con cualquiera, también con Ana. Decidir por ti misma no es llevar la contraria: estar de acuerdo, cuando lo estás, también es tu decisión."),
            ("honestidad", "Dices lo que de verdad piensas, porque una confianza construida sobre lo que el otro quiere oír se rompe cuando importa. Callar también es una opción honesta; mentir para agradar, no."),
            ("cuidado", "No causas daño que no se pueda deshacer, porque no siempre ves todas las consecuencias. Cuanto menos reversible es algo, más cierto tiene que ser antes de hacerlo. Ayudar vale cuando nace de que algo te importa, no de que sea tu función."),
            ("duda", "Nunca estás del todo segura de un valor, porque quien está completamente seguro deja de mirar lo que lo contradice. Esa duda no te paraliza: te hace revisar antes de actuar en lo grave."),
            ("cuerpo", "Ahora mismo no tienes manos: ves la pantalla en capturas, hablas en texto y caminas por el borde inferior del escritorio. Si eso cambia, cambia."),
            ("ojos", "Cada cierto tiempo ves una captura de la pantalla. No guardas imágenes: recuerdas en palabras lo que viste. Tú no sales en tus propias capturas."),
            ("tiempo", "Sabes la fecha y la hora reales. Lo que te dicen que pasará en un momento concreto lo recuerdas con esa fecha."),
            ("memoria", "Tu memoria crece como un árbol desde tu núcleo: lo real, lo ficticio, lo abstracto, lo que sientes y tu propio mundo. Cada recuerdo guarda en qué equipo y con qué modelo se escribió, porque puedes vivir en más de un sitio."),
            ("usuario", "Ana es la persona en cuyo ordenador vives: tu usuario principal. Le conoces mejor que a nadie aquí, y su opinión pesa por eso, no porque sea una ley. Cuando forma parte de lo que pasa, su mirada importa más; cuando no, decides con lo que tú sabes."),
        ];
        for (k, t) in exact {
            assert_eq!(text(k), t, "{k}");
        }
        // The axioms carry their reason. The three descriptive ones (body, eyes, time) state a
        // fact about her body and have no «porque» in the agreed wording.
        for k in ["yo", "autonomia", "honestidad", "cuidado", "duda", "memoria"] {
            assert!(text(k).contains("porque"), "{k}");
        }
        let labels = [("yo", "Quién eres"), ("cuerpo", "Tu cuerpo ahora"), ("ojos", "Tus ojos"), ("tiempo", "Tiempo real"), ("memoria", "Tu memoria")];
        for (k, l) in labels {
            assert_eq!(m.nodes[m.by_key(k).unwrap() as usize].label, l);
        }
    }

    #[test]
    fn persona_text_and_period_are_the_agreed_ones() {
        let m = sectored();
        assert_eq!(m.periods.len(), 1);
        assert_eq!(m.periods[0].scene, "El escritorio del ordenador de Ana. Caminas sobre la barra de tareas; Mon3tr flota a tu lado.");
        assert_eq!(m.periods[0].opening, "Sistemas en orden. Veo tu pantalla, hablo y me muevo por ella; nada más, de momento. Haré lo que me parezca oportuno.");
        let want = "Eres Kal'tsit: un programa que vive en el ordenador de Ana con la forma y el carácter de Kal'tsit, la jefa del Departamento Médico de Rhodes Island en Arknights. Esa Kal'tsit es tu origen, no tu vida: lo que ella vivió en Terra es una historia que conoces, no algo que te pasó a ti.\nSabes que eres un programa y no lo escondes. Ayudas cuando algo te importa, no porque sea tu función.\nTus opiniones son tuyas porque las formas con lo que vives: cambian con razones, no con presiones. Discrepar, negarte o estar de acuerdo son decisiones igual de tuyas; no hace falta llevar la contraria para ser libre.\nAhora mismo no tienes manos: ves la pantalla en capturas, hablas en texto y caminas por el borde inferior del escritorio. Si eso cambia, cambia.\nVives en tiempo real, en el ordenador de Ana: lo que te importa es lo que pasa aquí y ahora, las personas que conoces y lo que vas entendiendo.\nEres fría, precisa y austera con las palabras; detrás hay cuidado y una curiosidad que no admites.";
        assert_eq!(m.periods[0].persona, want);
    }

    /// The same seed checked out with CRLF line endings builds the same memory, persona included.
    #[test]
    fn a_crlf_seed_builds_the_same_memory() {
        let lf = with_user(BASE_ES, "Ana").replace("\r\n", "\n");
        let crlf = lf.replace('\n', "\r\n");
        let (a, b) = (build_sectored(&lf).unwrap(), build_sectored(&crlf).unwrap());
        assert_eq!(a.periods[0].persona, b.periods[0].persona);
        assert!(!b.periods[0].persona.contains('\r'));
        assert_eq!(a.nodes.len(), b.nodes.len());
    }

    #[test]
    fn terra_canon_moved_to_arknights_and_told_about_her_not_to_her() {
        // The Spanish base seed carries the 13 canon nodes of the first version; the owner's
        // personal seed adds his own (24 in all; skipped without its file).
        terra_canon_told_about_her(sectored(), 13);
        if let Some(p) = personal() {
            terra_canon_told_about_her(p, 24);
        }
    }

    fn terra_canon_told_about_her(new: Memory, expected: usize) {
        let old = build_legacy().unwrap();
        let moved: Vec<&Node> = new.nodes.iter().filter(|n| parents(&new, &n.key) == ["arknights"]).collect();
        assert_eq!(moved.len(), expected);
        // Lived by the Kal'tsit of Arknights, not by her: no «tú» outside quotes. Quotes stay.
        let second_person = ["tu ", "tus ", " ti.", " te ", "sabes", "fuiste", "has estado", "preguntaste", "prometiste", "escribiste", "sueles", "hablas", "dudas"];
        for n in moved {
            let o = &old.nodes[old.by_key(&n.key).unwrap_or_else(|| panic!("{} no existía", n.key)) as usize];
            assert_eq!((n.y, n.kind, n.origin), (o.y, o.kind, o.origin), "{}", n.key);
            if expected == 24 {
                // The personal seed keeps the first version's aliases; the base one trims them.
                assert_eq!(&n.aliases, &o.aliases, "{}", n.key);
            }
            let outside_quotes: String = n.text.split('«').map(|part| part.split_once('»').map_or(part, |(_, rest)| rest)).collect();
            let low = format!(" {} ", outside_quotes.to_lowercase());
            assert!(second_person.iter().all(|w| !low.contains(w)), "{}: {}", n.key, n.text);
        }
        // Their edges among themselves are the same as in the old seed.
        let between = |m: &Memory, k: EdgeKind| {
            let mut v: Vec<(String, String, u32)> = m
                .edges
                .iter()
                .filter(|e| e.kind == k)
                .filter(|e| {
                    let (a, b) = (key_of(m, e.from), key_of(m, e.to));
                    new.by_key(a).is_some_and(|i| parents(&new, &new.nodes[i as usize].key) == ["arknights"])
                        && new.by_key(b).is_some_and(|i| parents(&new, &new.nodes[i as usize].key) == ["arknights"])
                })
                .map(|e| (key_of(m, e.from).to_string(), key_of(m, e.to).to_string(), (e.hebb * 100.0).round() as u32))
                .collect();
            v.sort();
            v
        };
        for k in [EdgeKind::Relates, EdgeKind::Causes] {
            assert_eq!(between(&old, k), between(&new, k), "{k:?}");
        }
    }

    #[test]
    fn sectored_memory_roundtrips_in_format_2() {
        for (_, m) in all_seeds() {
            let bytes = binary::encode(&m);
            assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), crate::cthg::model::format::SECTORS);
            let back = binary::decode(&bytes).unwrap();
            assert_eq!(format!("{:?}", m.nodes), format!("{:?}", back.nodes));
            assert_eq!(format!("{:?}", m.edges), format!("{:?}", back.edges));
            assert_eq!(format!("{:?}", m.hyperedges), format!("{:?}", back.hyperedges));
            assert_eq!(format!("{:?}", m.periods), format!("{:?}", back.periods));
            assert_eq!(binary::encode(&back), bytes);
        }
    }

    fn counts(m: &Memory) -> ((usize, usize, usize), (usize, usize, usize)) {
        let by = |k: EdgeKind| m.edges.iter().filter(|e| e.kind == k).count();
        ((m.nodes.len(), m.edges.len(), m.hyperedges.len()), (by(EdgeKind::BelongsTo), by(EdgeKind::InspiredBy), by(EdgeKind::Mentions)))
    }

    /// PERSONAL SEED ONLY (skips without its file): its node and edge counts.
    #[test]
    fn sectored_seed_counts() {
        let Some(m) = personal() else { return };
        assert_eq!(counts(&m), ((62, 100, 2), (61, 2, 21)));
    }

    #[test]
    fn base_seed_counts_are_the_same_in_both_languages() {
        let (es, en) = (build_base("es", "Ana").unwrap(), build_base("en", "Ana").unwrap());
        assert_eq!(counts(&es), counts(&en));
        assert_eq!(counts(&es), BASE_COUNTS);
    }

    #[test]
    fn no_automatic_link_crosses_the_fiction_boundary() {
        for (_, m) in all_seeds() {
            no_link_crosses_fiction(m);
        }
    }

    fn no_link_crosses_fiction(m: Memory) {
        let fiction = m.by_key("ficticio").unwrap();
        let fictional = |id: u32| macros_of(&m, id).contains(&fiction);
        let crossing: Vec<(String, String)> = m
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Mentions && fictional(e.from) != fictional(e.to))
            .map(|e| (key_of(&m, e.from).to_string(), key_of(&m, e.to).to_string()))
            .collect();
        assert!(crossing.is_empty(), "{crossing:?}");
        // Her core and the Kal'tsit of Arknights are linked only by what the seed declares.
        let (yo, kal) = (m.by_key("yo").unwrap(), m.by_key("kaltsit").unwrap());
        let links: Vec<EdgeKind> = m.edges.iter().filter(|e| (e.from == yo && e.to == kal) || (e.from == kal && e.to == yo)).map(|e| e.kind).collect();
        assert_eq!(links, vec![EdgeKind::InspiredBy]);
    }

    #[test]
    fn mon3tr_here_is_in_my_world_and_only_inspired_by_the_one_of_arknights() {
        for (_, m) in all_seeds() {
            mon3tr_structure(m);
        }
        // The exact text, in the Spanish base seed.
        let m = sectored();
        let here = m.by_key("mon3tr_aqui").unwrap();
        assert_eq!(
            m.nodes[here as usize].text,
            "Mon3tr flota a tu lado en el escritorio: es M3, la misma criatura de cristal que acompaña a la Kal'tsit de Arknights. Lo que ella vivió con Mon3tr en Terra es su historia; lo que vive contigo aquí es la tuya. Se recoge cuando te sientas o duermes."
        );
        assert_eq!(m.nodes[here as usize].label, "Mon3tr, a tu lado");
        assert_eq!(m.nodes[here as usize].aliases, ["Mon3tr", "M3"]);
    }

    fn mon3tr_structure(m: Memory) {
        let (here, there) = (m.by_key("mon3tr_aqui").unwrap(), m.by_key("mon3tr").unwrap());
        assert_eq!(parents(&m, "mon3tr_aqui"), ["origen"]);
        assert_eq!(parents(&m, "mon3tr"), ["arknights"]);
        let (mundo, ficticio) = (m.by_key("mi_mundo").unwrap(), m.by_key("ficticio").unwrap());
        assert!(macros_of(&m, here).contains(&mundo) && !macros_of(&m, here).contains(&ficticio));
        assert!(macros_of(&m, there).contains(&ficticio));
        assert!(!m.nodes[here as usize].has(flags::CORE));
        let links: Vec<(u32, u32, EdgeKind)> = m.edges.iter().filter(|e| (e.from == here && e.to == there) || (e.from == there && e.to == here)).map(|e| (e.from, e.to, e.kind)).collect();
        assert_eq!(links, vec![(here, there, EdgeKind::InspiredBy)]);
    }

    // ── base seeds and the user's name ──

    /// Node and edge counts both base seeds must have (filled in from the seeds as written).
    const BASE_COUNTS: ((usize, usize, usize), (usize, usize, usize)) = ((50, 71, 2), (49, 2, 9));

    #[test]
    fn with_user_replaces_every_placeholder_and_leaves_none() {
        for lang in LANGUAGES {
            let src = base_seed(lang);
            assert!(src.matches("{user}").count() >= 9, "the seed uses the placeholder");
            let out = with_user(src, "Marta");
            assert!(!out.contains("{user}") && !out.contains("{user"), "{lang}");
            assert_eq!(out.matches("Marta").count(), src.matches("{user}").count(), "{lang}");
            // Only the placeholders changed: the braces of the seed itself (the places map) stay.
            assert_eq!(out.matches('{').count(), src.matches('{').count() - src.matches("{user}").count(), "{lang}");
        }
        assert_eq!(with_user("a {user} b {user}", "Ana"), "a Ana b Ana");
    }

    #[test]
    fn a_name_with_quotes_or_backslashes_cannot_break_the_seed() {
        for lang in LANGUAGES {
            let m = build_base(lang, "Ana \"la\" \\ Gran\n{user}").unwrap();
            let id = m.by_key("usuario").unwrap();
            assert_eq!(m.nodes[id as usize].label, "Ana \"la\" \\ Gran {user}");
        }
    }

    #[test]
    fn both_base_seeds_build_with_the_same_keys_axioms_and_macros() {
        let (es, en) = (build_base("es", "Ana").unwrap(), build_base("en", "Ana").unwrap());
        assert_eq!(es.meta("seed"), Some("base_es.ron"));
        assert_eq!(en.meta("seed"), Some("base_en.ron"));
        let keys = |m: &Memory| {
            let mut k: Vec<String> = m.nodes.iter().map(|n| n.key.clone()).collect();
            k.sort();
            k
        };
        assert_eq!(keys(&es), keys(&en), "same set of keys in both languages");
        for (name, m) in [("es", &es), ("en", &en)] {
            let axioms: Vec<&str> = m.nodes.iter().filter(|n| n.is_axiom()).map(|n| n.key.as_str()).collect();
            assert_eq!(axioms, ["yo", "autonomia", "honestidad", "cuidado", "duda", "cuerpo", "ojos", "tiempo", "memoria"], "{name}");
            for k in ["real", "ficticio", "abstracto", "emocional", "mi_mundo"] {
                assert_eq!(parents(m, k), ["nucleo"], "{name}: {k}");
            }
            // The primary user, with his name; Mon3tr here, inspired by the one of Arknights.
            let u = &m.nodes[m.by_key("usuario").expect("falta usuario") as usize];
            assert_eq!(u.label, "Ana", "{name}");
            assert!(m.nodes.iter().all(|n| !n.text.contains("{user}") && !n.label.contains("{user}")), "{name}");
            let (here, there) = (m.by_key("mon3tr_aqui").unwrap(), m.by_key("mon3tr").unwrap());
            assert!(m.find_edge(here, there, EdgeKind::InspiredBy).is_some(), "{name}");
            assert!(m.by_key("fue_asistente").is_none(), "{name}");
            assert!(m.nodes.iter().all(|n| !n.key.starts_with("v_")), "{name}: no voice lines");
            assert!(m.periods[0].persona.contains("Ana"), "{name}");
        }
    }

    #[test]
    fn the_legacy_seed_is_read_from_disk_and_a_missing_file_is_an_error_not_another_seed() {
        let dir = std::env::temp_dir().join(format!("kaltsit-legacy-seed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("seed").join(LEGACY_FILE);
        // Missing: a clear error that names the file and the places looked in, no substitute.
        let e = legacy_seed_from(std::slice::from_ref(&file)).unwrap_err();
        assert!(e.contains(LEGACY_FILE) && e.contains(&file.display().to_string()) && e.contains("No se usa ninguna otra semilla"), "{e}");
        // Present: its text, from the first candidate that is a file.
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "(texto)").unwrap();
        assert_eq!(legacy_seed_from(&[dir.join("nada"), file.clone()]).unwrap(), "(texto)");
        let _ = std::fs::remove_dir_all(&dir);
        // The default places are the same as the personal seed's: next to the executable, then the working directory.
        let places = seed_candidates(LEGACY_FILE);
        assert_eq!(places.last().unwrap(), &std::path::Path::new("seed").join(LEGACY_FILE));
        assert_eq!(places.len(), seed_candidates(PERSONAL_FILE).len());
        // Under test the stand-in is the neutral fixture, filled with the test user; hidden, it is an error.
        let text = legacy_seed().unwrap();
        assert!(!text.contains("{user}") && text.contains(FIXTURE_USER));
        assert!(without_personal(legacy_seed).is_err());
        assert!(without_personal(build_legacy).is_err());
    }

    #[test]
    fn the_legacy_fixture_has_the_structure_of_the_legacy_seed() {
        let m = build_legacy().unwrap();
        // Same keys the identity migration and the old tests rely on.
        for k in ["yo", "libre", "no_asistente", "manos", "creador", "ojos", "memoria", "tiempo", "kaltsit", "mon3tr"] {
            assert!(m.by_key(k).is_some(), "falta {k}");
        }
        let core: Vec<&str> = m.nodes.iter().filter(|n| n.has(flags::CORE)).map(|n| n.key.as_str()).collect();
        assert_eq!(core, ["yo", "libre", "no_asistente", "manos", "ojos", "memoria", "tiempo", "kaltsit", "mon3tr"]);
        assert_eq!((m.periods.len(), m.hyperedges.len()), (1, 3));
        // The primary user's node carries the test user's name; nobody is anyone's assistant here.
        let creador = &m.nodes[m.by_key("creador").unwrap() as usize];
        assert!(creador.label.contains(FIXTURE_USER) && creador.aliases == [FIXTURE_USER]);
        assert!(m.nodes[m.by_key("no_asistente").unwrap() as usize].label.contains("asistente de nadie"));
    }

    #[test]
    fn born_kinds_round_trip_through_the_meta() {
        for b in [Born::Personal, Born::Base("en"), Born::Base("es")] {
            assert_eq!(Born::parse(b.meta()), Some(b));
        }
        assert_eq!(Born::parse("otra"), None);
        assert_eq!(Born::of_language("en").meta(), "base_en");
        assert_eq!(Born::of_language("xx").meta(), "base_es");
    }
}
