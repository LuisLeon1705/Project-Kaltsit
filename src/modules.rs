//! Portable memory modules (`.cthm`).
//!
//! Her memory is split by what it is about, one file per module in `data/memoria/`:
//! - `yo`: her persona and base history (who she is, what she knows of Terra);
//! - `personas`: the real people she knows;
//! - `cosas`: objects, files, programs, projects, places, events and what she lived;
//! - `saber`: what she concluded on her own.
//!
//! Any other `.cthm` dropped in that folder is mounted as an extra module (a memory brought from
//! another computer, a knowledge pack...). Modules are made to travel between machines and
//! models with as few cuts as possible:
//! - nodes are addressed by stable keys, never by position, and edges point to keys, so an edge
//!   to a module that is not here stays pending (and is saved again) until that module returns;
//! - the same key coming from two modules is merged, keeping the version with more evidence;
//! - only text and numbers are stored, nothing that depends on the model reading them;
//! - every node names its origin (MAC, computer and model) through the module's sources table.
//!
//! Format, little-endian: "CTHM", u16 version, then name, title, saved time, sources, nodes,
//! edges, scenes, meta and chat, and a CRC32 of everything before it.

use crate::cthg::model::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"CTHM";
/// Newest format this build reads and writes. A file is written in the oldest format that can
/// hold its content (see `needed_format`), so a memory without sectors stays readable by old apps.
const VERSION: u16 = format::CURRENT;

pub const YO: u8 = 0;
pub const PERSONAS: u8 = 1;
pub const COSAS: u8 = 2;
pub const SABER: u8 = 3;
/// The four modules every memory has, in this order.
pub const CORE: [(&str, &str); 4] = [
    ("yo", "Persona e historia base"),
    ("personas", "Personas"),
    ("cosas", "Cosas"),
    ("saber", "Conocimiento propio"),
];

pub fn module_by_name(name: &str) -> Option<u8> {
    CORE.iter().position(|(n, _)| *n == name).map(|i| i as u8)
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub title: String,
}

/// An edge whose other end lives in a module that is not loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub module: u8,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub alpha: f32,
    pub beta: f32,
    pub hebb: f32,
}

pub struct Loaded {
    pub mem: Memory,
    pub modules: Vec<Module>,
    pub pending: Vec<Pending>,
    /// Keys found in more than one module and merged.
    pub merged: usize,
}

// ── encoding ──

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.b.get(self.i..self.i + n).ok_or("módulo truncado")?;
        self.i += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn str(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "texto no UTF-8".into())
    }
    fn count(&mut self) -> Result<usize, String> {
        let n = self.u32()? as usize;
        // Every record takes at least a byte: a bigger count means a corrupt file.
        if n > self.b.len() {
            return Err("recuento imposible".into());
        }
        Ok(n)
    }
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// A module as it is on disk: everything by key.
#[derive(Default, Debug)]
pub struct Raw {
    pub name: String,
    pub title: String,
    pub saved: f64,
    pub sources: Vec<Source>,
    /// (node with `source` relative to `sources`, edges by key below)
    pub nodes: Vec<Node>,
    pub edges: Vec<(String, String, EdgeKind, f32, f32, f32, u32)>,
    pub scenes: Vec<(String, f64, Vec<String>)>,
    pub meta: Vec<(String, String)>,
    pub chat: Vec<(Role, f64, String, Vec<String>)>,
}

/// Oldest format that can hold everything in `raw`: 1 unless it uses sectors, sector edges or
/// the personal-core flag. An app that only knows format 1 then refuses the file instead of
/// misreading it.
pub fn needed_format(raw: &Raw) -> u16 {
    let nodes = raw.nodes.iter().map(|n| format::node_needs(n.kind, n.flags));
    let edges = raw.edges.iter().map(|e| e.2.min_format());
    nodes.chain(edges).fold(format::LEGACY, u16::max)
}

pub fn encode(raw: &Raw) -> Vec<u8> {
    let mut w = W::default();
    w.0.extend_from_slice(MAGIC);
    w.u16(needed_format(raw));
    w.str(&raw.name);
    w.str(&raw.title);
    w.f64(raw.saved);
    w.u32(raw.sources.len() as u32);
    for s in &raw.sources {
        w.str(&s.mac);
        w.str(&s.host);
        w.str(&s.model);
    }
    w.u32(raw.nodes.len() as u32);
    for n in &raw.nodes {
        w.str(&n.key);
        w.u8(n.kind.code());
        w.u8(n.origin.code());
        w.u8(n.flags);
        w.f32(n.x);
        w.f64(n.y);
        w.f32(n.alpha);
        w.f32(n.beta);
        w.f32(n.activation);
        w.u32(n.recalls);
        w.f64(n.born);
        w.u16(n.source);
        w.str(&n.label);
        w.str(&n.text);
        w.u32(n.aliases.len() as u32);
        for a in &n.aliases {
            w.str(a);
        }
    }
    w.u32(raw.edges.len() as u32);
    for (f, t, k, a, b, h, turn) in &raw.edges {
        w.str(f);
        w.str(t);
        w.u8(k.code());
        w.f32(*a);
        w.f32(*b);
        w.f32(*h);
        w.u32(*turn);
    }
    w.u32(raw.scenes.len() as u32);
    for (label, y, members) in &raw.scenes {
        w.str(label);
        w.f64(*y);
        w.u32(members.len() as u32);
        for m in members {
            w.str(m);
        }
    }
    w.u32(raw.meta.len() as u32);
    for (k, v) in &raw.meta {
        w.str(k);
        w.str(v);
    }
    w.u32(raw.chat.len() as u32);
    for (role, at, text, recalled) in &raw.chat {
        w.u8(role.code());
        w.f64(*at);
        w.str(text);
        w.u32(recalled.len() as u32);
        for k in recalled {
            w.str(k);
        }
    }
    let crc = crc32(&w.0);
    w.u32(crc);
    w.0
}

pub fn decode(b: &[u8]) -> Result<Raw, String> {
    if b.len() < 10 || &b[..4] != MAGIC {
        return Err("no es un módulo de memoria (.cthm)".into());
    }
    let (body, tail) = b.split_at(b.len() - 4);
    if crc32(body) != u32::from_le_bytes(tail.try_into().unwrap()) {
        return Err("el módulo está dañado (CRC)".into());
    }
    let mut r = Rd { b: body, i: 4 };
    let version = r.u16()?;
    debug_assert_eq!(VERSION, format::CURRENT);
    format::check_version(version)?;
    let mut raw = Raw { name: r.str()?, title: r.str()?, saved: r.f64()?, ..Raw::default() };
    for _ in 0..r.count()? {
        raw.sources.push(Source { mac: r.str()?, host: r.str()?, model: r.str()? });
    }
    for _ in 0..r.count()? {
        let key = r.str()?;
        let code = r.u8()?;
        let kind = NodeKind::from_code(code).ok_or_else(|| format!("tipo de nodo desconocido ({code}): memoria de una app más nueva"))?;
        let origin = Origin::from_code(r.u8()?).ok_or("origen desconocido")?;
        let flags = r.u8()?;
        format::check_node(version, kind, flags)?;
        let mut n = Node {
            id: 0,
            kind,
            origin,
            flags,
            period: 0,
            x: r.f32()?,
            y: r.f64()?,
            alpha: r.f32()?,
            beta: r.f32()?,
            activation: r.f32()?,
            recalls: r.u32()?,
            created_turn: 0,
            key,
            label: String::new(),
            text: String::new(),
            aliases: vec![],
            module: 0,
            source: 0,
            born: 0.0,
        };
        n.born = r.f64()?;
        n.source = r.u16()?;
        n.label = r.str()?;
        n.text = r.str()?;
        for _ in 0..r.count()? {
            n.aliases.push(r.str()?);
        }
        raw.nodes.push(n);
    }
    for _ in 0..r.count()? {
        let (f, t) = (r.str()?, r.str()?);
        let code = r.u8()?;
        let k = EdgeKind::from_code(code).ok_or_else(|| format!("tipo de arista desconocido ({code}): memoria de una app más nueva"))?;
        format::check_edge(version, k)?;
        raw.edges.push((f, t, k, r.f32()?, r.f32()?, r.f32()?, r.u32()?));
    }
    for _ in 0..r.count()? {
        let (label, y) = (r.str()?, r.f64()?);
        let mut members = Vec::new();
        for _ in 0..r.count()? {
            members.push(r.str()?);
        }
        raw.scenes.push((label, y, members));
    }
    for _ in 0..r.count()? {
        raw.meta.push((r.str()?, r.str()?));
    }
    for _ in 0..r.count()? {
        let role = Role::from_code(r.u8()?);
        let (at, text) = (r.f64()?, r.str()?);
        let mut rec = Vec::new();
        for _ in 0..r.count()? {
            rec.push(r.str()?);
        }
        raw.chat.push((role, at, text, rec));
    }
    Ok(raw)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    if path.exists() {
        let _ = std::fs::copy(path, path.with_extension("cthm.bak"));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

// ── the whole memory ⇄ module files ──

pub fn dir_of(data: &Path) -> PathBuf {
    data.join("memoria")
}

pub fn history_file(data: &Path) -> PathBuf {
    data.join("historial.cthm")
}

/// A memory as files: (path relative to the data folder, bytes). What is saved, replicated to
/// the other computers and merged.
pub type Files = Vec<(String, Vec<u8>)>;

const HISTORY: &str = "historial.cthm";

/// Every module file in `data/memoria` plus the chat history, as they are on disk.
pub fn read_all(data: &Path) -> Result<Files, String> {
    let mut out = Files::new();
    let dir = dir_of(data);
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "cthm")).collect())
        .unwrap_or_default();
    names.sort();
    for f in names {
        let name = format!("memoria/{}", f.file_name().unwrap_or_default().to_string_lossy());
        out.push((name, std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?));
    }
    let hist = history_file(data);
    if hist.exists() {
        out.push((HISTORY.into(), std::fs::read(&hist).map_err(|e| e.to_string())?));
    }
    Ok(out)
}

/// Writes files received from another computer (only names we know how to place).
pub fn write_files(data: &Path, files: &Files) -> Result<(), String> {
    std::fs::create_dir_all(dir_of(data)).map_err(|e| e.to_string())?;
    for (name, bytes) in files {
        let ok = name == HISTORY || (name.starts_with("memoria/") && name.ends_with(".cthm") && !name[8..].contains(['/', '\\']) && !name.contains(".."));
        if !ok {
            return Err(format!("nombre de archivo no válido: {name}"));
        }
        decode(bytes).map_err(|e| format!("{name}: {e}"))?;
        write_atomic(&data.join(name), bytes)?;
    }
    Ok(())
}

/// Mounts every module in `data/memoria` (core ones first) and the chat history. `None` when
/// there is no memory there yet.
pub fn load(data: &Path) -> Result<Option<Loaded>, String> {
    let files = read_all(data)?;
    if !files.iter().any(|(n, _)| n.starts_with("memoria/")) {
        return Ok(None);
    }
    from_files(&files).map(Some)
}

/// Builds one memory out of module files. Several files of the same module (this computer's and
/// another's) merge: nodes by key keeping the richer evidence, edges once, chat lines once.
pub fn from_files(files: &Files) -> Result<Loaded, String> {
    let mut raws = Vec::new();
    let mut chats = Vec::new();
    for (name, bytes) in files {
        let raw = decode(bytes).map_err(|e| format!("{name}: {e}"))?;
        if raw.name == "historial" {
            chats.push(raw);
        } else {
            raws.push(raw);
        }
    }
    // Core modules keep their fixed index; the rest follow in order of appearance.
    let mut modules: Vec<Module> = CORE.iter().map(|(n, t)| Module { name: n.to_string(), title: t.to_string() }).collect();
    let mut order: Vec<(u8, usize, Raw)> = Vec::new();
    for (i, raw) in raws.into_iter().enumerate() {
        let idx = match module_by_name(&raw.name) {
            Some(i) => i,
            None => match modules.iter().position(|m| m.name == raw.name) {
                Some(i) => i as u8,
                None => {
                    modules.push(Module { name: raw.name.clone(), title: raw.title.clone() });
                    (modules.len() - 1) as u8
                }
            },
        };
        order.push((idx, i, raw));
    }
    // Stable: for the same module, the first file given (this computer's) is read first.
    order.sort_by_key(|(idx, i, _)| (*idx, *i));

    let mut mem = Memory::default();
    let mut by_key: HashMap<String, u32> = HashMap::new();
    let mut merged = 0;
    let mut edges = Vec::new();
    let mut scenes = Vec::new();
    for (idx, _, raw) in order {
        let map: Vec<u16> = raw.sources.iter().map(|s| mem.source_id(s)).collect();
        for mut n in raw.nodes {
            n.module = idx;
            n.source = map.get(n.source as usize).copied().unwrap_or(0);
            match by_key.get(&n.key).copied() {
                Some(id) => {
                    merged += 1;
                    let old = &mut mem.nodes[id as usize];
                    for a in &n.aliases {
                        if !old.aliases.contains(a) {
                            old.aliases.push(a.clone());
                        }
                    }
                    old.activation = old.activation.max(n.activation);
                    if n.alpha + n.beta > old.alpha + old.beta {
                        let (id, module, aliases, act) = (old.id, old.module, std::mem::take(&mut old.aliases), old.activation);
                        *old = n;
                        old.id = id;
                        old.module = module;
                        old.aliases = aliases;
                        old.activation = act;
                    }
                }
                None => {
                    let key = n.key.clone();
                    let id = mem.add_node(n);
                    if !key.is_empty() {
                        by_key.insert(key, id);
                    }
                }
            }
        }
        for e in raw.edges {
            edges.push((idx, e));
        }
        scenes.extend(raw.scenes);
        // The first file that defines a meta key wins (yo comes first, this computer's first).
        for (k, v) in raw.meta {
            if mem.meta(&k).is_none() {
                mem.set_meta(&k, &v);
            }
        }
    }
    let mut pending = Vec::new();
    for (module, (f, t, kind, alpha, beta, hebb, turn)) in edges {
        match (by_key.get(&f), by_key.get(&t)) {
            (Some(&a), Some(&b)) => {
                let i = mem.upsert_edge(a, b, kind, alpha, beta);
                mem.edges[i].hebb = mem.edges[i].hebb.max(hebb);
                mem.edges[i].created_turn = mem.edges[i].created_turn.max(turn);
            }
            _ => {
                let p = Pending { module, from: f, to: t, kind, alpha, beta, hebb };
                if !pending.contains(&p) {
                    pending.push(p);
                }
            }
        }
    }
    let mut seen_scenes = std::collections::HashSet::new();
    for (label, y, members) in scenes {
        if !seen_scenes.insert((label.clone(), members.clone())) {
            continue;
        }
        let members: Vec<u32> = members.iter().filter_map(|k| by_key.get(k).copied()).collect();
        if !members.is_empty() {
            let id = mem.hyperedges.len() as u32;
            mem.hyperedges.push(Hyperedge { id, label, y, members });
        }
    }
    // Chat: both histories, each line once, in the order it was said.
    let mut lines: Vec<(Role, f64, String, Vec<String>)> = Vec::new();
    let mut seen_lines = std::collections::HashSet::new();
    for raw in chats {
        for (role, at, text, rec) in raw.chat {
            if seen_lines.insert((role.code(), (at * 1000.0) as i64, text.clone())) {
                lines.push((role, at, text, rec));
            }
        }
    }
    lines.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    for (role, at, text, rec) in lines {
        let recalled = rec.iter().filter_map(|k| by_key.get(k).copied()).collect();
        mem.chat.push(ChatMessage { period: 0, role, turn: 0, text, recalled, at });
    }
    mem.turn = mem.meta("turno").and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(Loaded { mem, modules, pending, merged })
}

/// One module's file content, from the in-memory graph.
pub fn raw_of(mem: &Memory, idx: u8, m: &Module, pending: &[Pending], now: f64) -> Raw {
    let mut raw = Raw { name: m.name.clone(), title: m.title.clone(), saved: now, ..Raw::default() };
    let mut local: HashMap<u16, u16> = HashMap::new();
    for n in mem.nodes.iter().filter(|n| n.module == idx) {
        let mut c = n.clone();
        let next = local.len() as u16;
        c.source = *local.entry(n.source).or_insert_with(|| {
            raw.sources.push(mem.sources.get(n.source as usize).cloned().unwrap_or_default());
            next
        });
        raw.nodes.push(c);
    }
    let key = |id: u32| mem.nodes[id as usize].key.clone();
    for e in mem.edges.iter().filter(|e| mem.nodes[e.from as usize].module == idx) {
        raw.edges.push((key(e.from), key(e.to), e.kind, e.alpha, e.beta, e.hebb, e.created_turn));
    }
    for p in pending.iter().filter(|p| p.module == idx) {
        raw.edges.push((p.from.clone(), p.to.clone(), p.kind, p.alpha, p.beta, p.hebb, 1));
    }
    for h in mem.hyperedges.iter().filter(|h| h.members.first().is_some_and(|&f| mem.nodes[f as usize].module == idx)) {
        raw.scenes.push((h.label.clone(), h.y, h.members.iter().map(|&i| key(i)).collect()));
    }
    if idx == YO {
        raw.meta = mem.meta.clone();
    }
    raw
}

/// The whole memory as files, ready to save or to send.
pub fn encode_all(mem: &Memory, modules: &[Module], pending: &[Pending], now: f64) -> Files {
    let mut out = Files::new();
    for (i, m) in modules.iter().enumerate() {
        out.push((format!("memoria/{}.cthm", m.name), encode(&raw_of(mem, i as u8, m, pending, now))));
    }
    let key = |id: u32| mem.nodes.get(id as usize).map(|n| n.key.clone()).unwrap_or_default();
    let hist = Raw {
        name: "historial".into(),
        title: "Historial del chat".into(),
        saved: now,
        chat: mem.chat.iter().map(|c| (c.role, c.at, c.text.clone(), c.recalled.iter().map(|&i| key(i)).collect())).collect(),
        ..Raw::default()
    };
    out.push((HISTORY.into(), encode(&hist)));
    out
}

pub fn save(data: &Path, mem: &Memory, modules: &[Module], pending: &[Pending], now: f64) -> Result<Files, String> {
    let files = encode_all(mem, modules, pending, now);
    std::fs::create_dir_all(dir_of(data)).map_err(|e| e.to_string())?;
    for (name, bytes) in &files {
        write_atomic(&data.join(name), bytes)?;
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(key: &str, module: u8, alpha: f32) -> Node {
        Node {
            id: 0,
            kind: NodeKind::Fact,
            origin: Origin::Learned,
            flags: 0,
            period: 0,
            x: 0.1,
            y: 2026.8,
            alpha,
            beta: 1.0,
            activation: 0.5,
            recalls: 2,
            created_turn: 0,
            key: key.into(),
            label: key.to_uppercase(),
            text: format!("texto de {key}"),
            aliases: vec![key.into()],
            module,
            source: 0,
            born: 1_790_000_000.0,
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kaltsit-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn sample() -> (Memory, Vec<Module>) {
        let mut m = Memory::default();
        let pc = m.source_id(&Source { mac: "AA-BB-CC-DD-EE-01".into(), host: "PC-TEST".into(), model: "gemma-4-12B".into() });
        let seed = m.source_id(&Source { mac: "AA-BB-CC-DD-EE-01".into(), host: "PC-TEST".into(), model: "semilla".into() });
        let mut a = node("yo-1", YO, 3.0);
        a.source = seed;
        let mut b = node("ana", PERSONAS, 2.0);
        b.source = pc;
        let c = node("rust", COSAS, 2.0);
        let (a, b, c) = (m.add_node(a), m.add_node(b), m.add_node(c));
        m.upsert_edge(b, c, EdgeKind::Relates, 1.0, 1.0);
        m.upsert_edge(a, b, EdgeKind::Mentions, 1.0, 1.0);
        m.set_meta("usuario", "Ana");
        m.chat.push(ChatMessage { period: 0, role: Role::User, turn: 0, text: "hola".into(), recalled: vec![c], at: 1_790_000_100.0 });
        let modules = CORE.iter().map(|(n, t)| Module { name: n.to_string(), title: t.to_string() }).collect();
        (m, modules)
    }

    #[test]
    fn modules_roundtrip_with_keys_origins_and_chat() {
        let (m, modules) = sample();
        let d = tmp("roundtrip");
        save(&d, &m, &modules, &[], 1.0).unwrap();
        for name in ["yo", "personas", "cosas", "saber"] {
            assert!(dir_of(&d).join(format!("{name}.cthm")).exists(), "{name}");
        }
        let l = load(&d).unwrap().unwrap();
        assert_eq!(l.mem.nodes.len(), 3);
        assert_eq!(l.mem.edges.len(), 2);
        assert!(l.pending.is_empty());
        let ana = l.mem.by_key("ana").unwrap() as usize;
        assert_eq!(l.mem.nodes[ana].module, PERSONAS);
        let src = &l.mem.sources[l.mem.nodes[ana].source as usize];
        assert_eq!((src.mac.as_str(), src.host.as_str(), src.model.as_str()), ("AA-BB-CC-DD-EE-01", "PC-TEST", "gemma-4-12B"));
        let yo = l.mem.by_key("yo-1").unwrap() as usize;
        assert_eq!(l.mem.sources[l.mem.nodes[yo].source as usize].model, "semilla");
        assert_eq!(l.mem.meta("usuario"), Some("Ana"));
        assert_eq!(l.mem.chat[0].text, "hola");
        assert_eq!(l.mem.chat[0].recalled, vec![l.mem.by_key("rust").unwrap()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_module_leaves_its_edges_pending_not_lost() {
        let (m, modules) = sample();
        let d = tmp("pending");
        save(&d, &m, &modules, &[], 1.0).unwrap();
        // Take "cosas" to another computer: personas→cosas has nowhere to land.
        let away = d.join("cosas.away");
        std::fs::rename(dir_of(&d).join("cosas.cthm"), &away).unwrap();
        let l = load(&d).unwrap().unwrap();
        assert_eq!(l.pending.len(), 1);
        assert_eq!((l.pending[0].from.as_str(), l.pending[0].to.as_str()), ("ana", "rust"));
        // Saving without it keeps the pending edge; when cosas comes back, the edge is whole.
        save(&d, &l.mem, &l.modules, &l.pending, 2.0).unwrap();
        std::fs::rename(&away, dir_of(&d).join("cosas.cthm")).unwrap();
        let back = load(&d).unwrap().unwrap();
        assert!(back.pending.is_empty());
        let (ana, rust) = (back.mem.by_key("ana").unwrap(), back.mem.by_key("rust").unwrap());
        assert!(back.mem.find_edge(ana, rust, EdgeKind::Relates).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn modules_from_another_computer_merge_and_keep_their_origin() {
        let (m, modules) = sample();
        let d = tmp("merge");
        save(&d, &m, &modules, &[], 1.0).unwrap();
        // A module written on another PC by another model: one known key with more evidence, one new.
        let mut other = Memory::default();
        let src = other.source_id(&Source { mac: "11-22-33-44-55-66".into(), host: "PORTATIL".into(), model: "qwen3-8b".into() });
        // Written as module 0 of that file: on this computer it is an extra module.
        let mut richer = node("ana", 0, 9.0);
        richer.source = src;
        richer.aliases = vec!["Anita".into()];
        let mut fresh = node("eva", 0, 2.0);
        fresh.source = src;
        other.add_node(richer);
        other.add_node(fresh);
        let extra = Module { name: "portatil".into(), title: "Memoria del portátil".into() };
        let raw = raw_of(&other, 0, &extra, &[], 3.0);
        std::fs::write(dir_of(&d).join("portatil.cthm"), encode(&raw)).unwrap();

        let l = load(&d).unwrap().unwrap();
        assert_eq!(l.merged, 1);
        assert_eq!(l.modules.last().unwrap().name, "portatil");
        let ana = &l.mem.nodes[l.mem.by_key("ana").unwrap() as usize];
        assert_eq!(ana.alpha, 9.0, "the richer version wins");
        assert_eq!(ana.module, PERSONAS, "but it stays where it lived");
        assert!(ana.aliases.contains(&"ana".to_string()) && ana.aliases.contains(&"Anita".to_string()));
        assert_eq!(l.mem.sources[ana.source as usize].host, "PORTATIL");
        let ana = &l.mem.nodes[l.mem.by_key("ana").unwrap() as usize];
        assert_eq!(l.mem.sources[ana.source as usize].model, "qwen3-8b");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn two_diverged_memories_merge_into_one() {
        // Two computers start from the same memory and each lives something on its own.
        let (m, modules) = sample();
        let base = encode_all(&m, &modules, &[], 1.0);
        let mut a = from_files(&base).unwrap();
        let mut b = from_files(&base).unwrap();
        let na = a.mem.add_node(node("visto-en-desktop", COSAS, 2.0));
        a.mem.chat.push(ChatMessage { period: 0, role: Role::User, turn: 0, text: "desde el sobremesa".into(), recalled: vec![na], at: 1_790_000_200.0 });
        b.mem.add_node(node("visto-en-laptop", COSAS, 2.0));
        b.mem.chat.push(ChatMessage { period: 0, role: Role::User, turn: 0, text: "desde la laptop".into(), recalled: vec![], at: 1_790_000_150.0 });
        let ana = b.mem.by_key("ana").unwrap() as usize;
        b.mem.nodes[ana].alpha = 7.0;
        let mut files = encode_all(&a.mem, &a.modules, &[], 2.0);
        files.extend(encode_all(&b.mem, &b.modules, &[], 3.0));
        let m = from_files(&files).unwrap();
        assert!(m.mem.by_key("visto-en-desktop").is_some() && m.mem.by_key("visto-en-laptop").is_some());
        assert_eq!(m.mem.nodes[m.mem.by_key("ana").unwrap() as usize].alpha, 7.0, "the richer evidence wins");
        let texts: Vec<&str> = m.mem.chat.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["hola", "desde la laptop", "desde el sobremesa"], "each line once, in time order");
        assert_eq!(m.mem.edges.len(), 2, "shared edges are not doubled");
        assert_eq!(m.mem.nodes.len(), 5);
    }

    #[test]
    fn received_files_are_checked_before_writing() {
        let d = tmp("write");
        let (m, modules) = sample();
        let files = encode_all(&m, &modules, &[], 1.0);
        write_files(&d, &files).unwrap();
        assert_eq!(read_all(&d).unwrap().len(), files.len());
        assert!(write_files(&d, &vec![("../fuera.cthm".into(), files[0].1.clone())]).is_err(), "no escaping the folder");
        assert!(write_files(&d, &vec![("memoria/x.cthm".into(), b"basura".to_vec())]).is_err(), "no garbage");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn damaged_modules_are_refused() {
        let (m, modules) = sample();
        let mut b = encode(&raw_of(&m, PERSONAS, &modules[1], &[], 1.0));
        let n = b.len();
        b[n / 2] ^= 0x5A;
        assert!(decode(&b).unwrap_err().contains("CRC"));
        assert!(decode(b"nada").is_err());
    }

    // ── format 2: sectors ──

    fn sector(key: &str, label: &str) -> Node {
        let mut n = node(key, YO, 1.0);
        n.kind = NodeKind::Sector;
        n.origin = Origin::Canon;
        n.label = label.into();
        n.text = format!("descripción de {label}");
        n.aliases.clear();
        n
    }

    /// Rewrites a file's version (bytes 4..6) and fixes the CRC, to forge what other builds write.
    fn forge(bytes: &[u8], version: u16) -> Vec<u8> {
        let mut b = bytes[..bytes.len() - 4].to_vec();
        b[4..6].copy_from_slice(&version.to_le_bytes());
        let crc = crc32(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b
    }

    /// A raw module with one node (key "k", empty names): the node's kind byte is at offset 35.
    fn tiny(kind: NodeKind, flags: u8) -> Vec<u8> {
        let mut n = node("k", YO, 1.0);
        n.kind = kind;
        n.flags = flags;
        encode(&Raw { nodes: vec![n], ..Raw::default() })
    }

    fn with_byte(bytes: &[u8], at: usize, v: u8) -> Vec<u8> {
        let mut b = bytes[..bytes.len() - 4].to_vec();
        b[at] = v;
        let crc = crc32(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b
    }

    fn sectored() -> Raw {
        let mut hijo = node("hijo", YO, 2.0);
        hijo.flags = flags::PERSONAL_CORE | flags::CORE;
        let mut raw = Raw { name: "yo".into(), title: "Persona".into(), saved: 5.0, ..Raw::default() };
        raw.sources.push(Source { mac: "m".into(), host: "h".into(), model: "x".into() });
        raw.nodes = vec![sector("s-casa", "Casa"), sector("s-trabajo", "Trabajo"), hijo, node("kaltsit-aqui", YO, 3.0), node("kaltsit-arknights", YO, 3.0)];
        raw.edges = vec![
            ("hijo".into(), "s-casa".into(), EdgeKind::BelongsTo, 7.0, 1.0, 0.0, 3),
            ("hijo".into(), "s-trabajo".into(), EdgeKind::BelongsTo, 2.0, 5.0, 0.0, 3),
            ("kaltsit-aqui".into(), "kaltsit-arknights".into(), EdgeKind::InspiredBy, 4.0, 1.0, 0.25, 9),
        ];
        raw
    }

    #[test]
    fn sectors_belonging_inspiration_and_personal_core_roundtrip() {
        let raw = sectored();
        let bytes = encode(&raw);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), format::SECTORS, "new types carry the new version");
        let back = decode(&bytes).unwrap();
        assert_eq!(format!("{raw:?}"), format!("{back:?}"));
        assert_eq!(back.nodes[0].kind, NodeKind::Sector);
        assert_eq!(back.nodes[0].label, "Casa");
        assert!(back.nodes[2].has(flags::PERSONAL_CORE) && back.nodes[2].has(flags::CORE));
        assert_eq!(back.edges[2].2, EdgeKind::InspiredBy);
    }

    #[test]
    fn a_node_can_belong_to_two_sectors_with_both_weights() {
        let mut m = Memory::default();
        let a = m.add_node(sector("s-casa", "Casa"));
        let b = m.add_node(sector("s-trabajo", "Trabajo"));
        let c = m.add_node(node("hijo", COSAS, 2.0));
        m.upsert_edge(c, a, EdgeKind::BelongsTo, 7.0, 1.0);
        m.upsert_edge(c, b, EdgeKind::BelongsTo, 2.0, 5.0);
        m.nodes[c as usize].flags |= flags::PERSONAL_CORE;
        let modules: Vec<Module> = CORE.iter().map(|(n, t)| Module { name: n.to_string(), title: t.to_string() }).collect();
        let l = from_files(&encode_all(&m, &modules, &[], 1.0)).unwrap();
        let hijo = l.mem.by_key("hijo").unwrap();
        let belongs: Vec<_> = l.mem.edges.iter().filter(|e| e.from == hijo && e.kind == EdgeKind::BelongsTo).collect();
        assert_eq!(belongs.len(), 2);
        let w = |key: &str| {
            let s = l.mem.by_key(key).unwrap();
            let e = belongs.iter().find(|e| e.to == s).unwrap();
            (e.alpha, e.beta)
        };
        assert_eq!(w("s-casa"), (7.0, 1.0));
        assert_eq!(w("s-trabajo"), (2.0, 5.0));
        assert!(l.mem.nodes[hijo as usize].has(flags::PERSONAL_CORE));
        assert_eq!(l.mem.nodes[l.mem.by_key("s-casa").unwrap() as usize].kind, NodeKind::Sector);
    }

    /// Hand-written format-1 file, laid out as the first build of the format did.
    fn legacy_file() -> Vec<u8> {
        let mut w = W::default();
        w.0.extend_from_slice(MAGIC);
        w.u16(1);
        w.str("cosas");
        w.str("Cosas");
        w.f64(42.0);
        w.u32(0); // sources
        w.u32(1); // nodes
        w.str("rust");
        w.u8(5); // Fact
        w.u8(3); // Learned
        w.u8(flags::CONSOLIDATED | flags::SCHEDULED | (1 << 5)); // bit 5: DATED in mind.rs
        w.f32(0.1);
        w.f64(2026.8);
        w.f32(2.0);
        w.f32(1.0);
        w.f32(0.5);
        w.u32(2);
        w.f64(1_790_000_000.0);
        w.u16(0);
        w.str("RUST");
        w.str("texto de rust");
        w.u32(1);
        w.str("rust");
        w.u32(1); // edges
        w.str("rust");
        w.str("rust");
        w.u8(2); // Relates
        w.f32(1.0);
        w.f32(1.0);
        w.f32(0.5);
        w.u32(4);
        w.u32(0); // scenes
        w.u32(1); // meta
        w.str("usuario");
        w.str("Ana");
        w.u32(0); // chat
        let crc = crc32(&w.0);
        w.u32(crc);
        w.0
    }

    #[test]
    fn a_format_1_file_still_reads_the_same_and_is_written_back_as_format_1() {
        let old = legacy_file();
        let raw = decode(&old).unwrap();
        assert_eq!((raw.name.as_str(), raw.saved), ("cosas", 42.0));
        let n = &raw.nodes[0];
        assert_eq!((n.kind, n.origin, n.key.as_str(), n.label.as_str()), (NodeKind::Fact, Origin::Learned, "rust", "RUST"));
        assert_eq!(n.flags, flags::CONSOLIDATED | flags::SCHEDULED | (1 << 5));
        assert_eq!((n.alpha, n.beta, n.recalls, n.aliases.clone()), (2.0, 1.0, 2, vec!["rust".to_string()]));
        assert_eq!(raw.edges[0].2, EdgeKind::Relates);
        assert_eq!(raw.meta, vec![("usuario".to_string(), "Ana".to_string())]);
        assert_eq!(encode(&raw), old, "no new type: same bytes, same version, an old app can still open it");
    }

    #[test]
    fn unknown_versions_and_types_are_refused_with_a_clear_error() {
        let ok = encode(&sectored());
        let newer = forge(&ok, format::CURRENT + 1);
        assert!(decode(&newer).unwrap_err().contains("más nueva"));
        assert!(decode(&forge(&ok, 0)).unwrap_err().contains("inválida"));
        // A sector inside a file that says it is format 1 is not quietly turned into anything.
        let lie = forge(&ok, format::LEGACY);
        let e = decode(&lie).unwrap_err();
        assert!(e.contains("sector") || e.contains("pertenece") || e.contains("formato 1"), "{e}");
        // A node-kind code nobody knows (here 9) is an error, not a fact.
        let tiny_ok = tiny(NodeKind::Fact, 0);
        assert_eq!(tiny_ok[35], NodeKind::Fact.code());
        let e = decode(&with_byte(&tiny_ok, 35, 9)).unwrap_err();
        assert!(e.contains("tipo de nodo desconocido"), "{e}");
        // Same for flag bit 7, and for the personal-core bit in a format-1 file.
        assert!(decode(&with_byte(&tiny_ok, 37, 0x80)).unwrap_err().contains("marcas"));
        let pc = tiny(NodeKind::Fact, flags::PERSONAL_CORE);
        assert_eq!(u16::from_le_bytes([pc[4], pc[5]]), format::SECTORS);
        assert!(decode(&forge(&pc, format::LEGACY)).unwrap_err().contains("formato 1"));
        // And an edge-kind code nobody knows (8).
        let mut raw = sectored();
        raw.edges.truncate(1);
        raw.edges[0].2 = EdgeKind::Relates;
        let relates = encode(&raw);
        // Tail: edge kind | f32 f32 f32 u32 | scenes, meta, chat counts (12) | crc (4)
        let kind_at = relates.len() - 4 - 12 - 16 - 1;
        assert_eq!(relates[kind_at], EdgeKind::Relates.code());
        assert!(decode(&with_byte(&relates, kind_at, 8)).unwrap_err().contains("tipo de arista desconocido"));
    }

    #[test]
    fn a_module_without_sectors_is_written_as_format_1() {
        let (m, modules) = sample();
        for (_, bytes) in encode_all(&m, &modules, &[], 1.0) {
            assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), format::LEGACY);
        }
    }
}
