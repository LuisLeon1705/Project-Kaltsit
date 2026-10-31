//! Format of the first version (one .cthg file). Kept to read and fold in an old memory;
//! the memory now lives in portable modules (`modules.rs`).
#![cfg_attr(not(test), allow(dead_code))]
//! `.cthg` binary format, little endian.
//!
//! ```text
//! "CTHG" | u16 version | u16 reserved | u32 section_count
//! section* : [u8;4] tag | u32 payload_len | payload
//! u32 crc32 (IEEE) of every byte before it
//! ```
//! Sections: META, PERI, NODE, EDGE, HYPE, CHAT. Strings are u32 length + UTF-8.
//! Saves are atomic: write `.tmp`, keep the previous file as `.bak`, rename.

use super::model::*;
use std::fs;
use std::io;
use std::path::Path;

pub const MAGIC: &[u8; 4] = b"CTHG";
/// Oldest format that can hold everything in `m`: 1 unless it uses sectors, sector edges or the
/// personal-core flag (then 2). `format::CURRENT` is the newest this build reads.
pub fn needed_format(m: &Memory) -> u16 {
    let nodes = m.nodes.iter().map(|n| format::node_needs(n.kind, n.flags));
    let edges = m.edges.iter().map(|e| e.kind.min_format());
    nodes.chain(edges).fold(format::LEGACY, u16::max)
}

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

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
    fn ids(&mut self, ids: &[u32]) {
        self.u32(ids.len() as u32);
        for &i in ids {
            self.u32(i);
        }
    }
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("cthg: {msg}"))
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        if self.p + n > self.b.len() {
            return Err(bad("archivo truncado"));
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn str(&mut self) -> io::Result<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| bad("utf-8 inválido"))
    }
    fn ids(&mut self) -> io::Result<Vec<u32>> {
        let n = self.u32()? as usize;
        (0..n).map(|_| self.u32()).collect()
    }
    fn done(&self) -> bool {
        self.p >= self.b.len()
    }
}

pub fn encode(m: &Memory) -> Vec<u8> {
    let mut sections: Vec<([u8; 4], W)> = Vec::new();

    let mut s = W::default();
    s.u32(m.turn);
    s.u32(m.meta.len() as u32);
    for (k, v) in &m.meta {
        s.str(k);
        s.str(v);
    }
    sections.push((*b"META", s));

    let mut s = W::default();
    s.u32(m.periods.len() as u32);
    for p in &m.periods {
        s.u8(p.id);
        s.str(&p.key);
        s.str(&p.name);
        s.str(&p.source);
        s.f64(p.start);
        s.f64(p.end);
        s.f64(p.now);
        s.f32(p.home_x);
        s.str(&p.persona);
        s.str(&p.scene);
        s.str(&p.opening);
    }
    sections.push((*b"PERI", s));

    let mut s = W::default();
    s.u32(m.nodes.len() as u32);
    for n in &m.nodes {
        s.u32(n.id);
        s.u8(n.kind.code());
        s.u8(n.origin.code());
        s.u8(n.flags);
        s.u8(n.period);
        s.f32(n.x);
        s.f64(n.y);
        s.f32(n.alpha);
        s.f32(n.beta);
        s.f32(n.activation);
        s.u32(n.recalls);
        s.u32(n.created_turn);
        s.str(&n.key);
        s.str(&n.label);
        s.str(&n.text);
        s.u16(n.aliases.len() as u16);
        for a in &n.aliases {
            s.str(a);
        }
    }
    sections.push((*b"NODE", s));

    let mut s = W::default();
    s.u32(m.edges.len() as u32);
    for e in &m.edges {
        s.u32(e.from);
        s.u32(e.to);
        s.u8(e.kind.code());
        s.f32(e.alpha);
        s.f32(e.beta);
        s.f32(e.hebb);
        s.u32(e.created_turn);
    }
    sections.push((*b"EDGE", s));

    let mut s = W::default();
    s.u32(m.hyperedges.len() as u32);
    for h in &m.hyperedges {
        s.u32(h.id);
        s.str(&h.label);
        s.f64(h.y);
        s.ids(&h.members);
    }
    sections.push((*b"HYPE", s));

    let mut s = W::default();
    s.u32(m.chat.len() as u32);
    for c in &m.chat {
        s.u8(c.period);
        s.u8(c.role.code());
        s.u32(c.turn);
        s.str(&c.text);
        s.ids(&c.recalled);
    }
    sections.push((*b"CHAT", s));

    let mut out = W::default();
    out.0.extend_from_slice(MAGIC);
    out.u16(needed_format(m));
    out.u16(0);
    out.u32(sections.len() as u32);
    for (tag, body) in &sections {
        out.0.extend_from_slice(tag);
        out.u32(body.0.len() as u32);
        out.0.extend_from_slice(&body.0);
    }
    let crc = crc32(&out.0);
    out.u32(crc);
    out.0
}

pub fn decode(bytes: &[u8]) -> io::Result<Memory> {
    if bytes.len() < 16 {
        return Err(bad("archivo demasiado corto"));
    }
    let (body, tail) = bytes.split_at(bytes.len() - 4);
    let stored = u32::from_le_bytes(tail.try_into().unwrap());
    if crc32(body) != stored {
        return Err(bad("CRC32 no coincide (archivo corrupto)"));
    }
    let mut r = R { b: body, p: 0 };
    if r.take(4)? != MAGIC {
        return Err(bad("no es un archivo CTHG"));
    }
    let version = r.u16()?;
    format::check_version(version).map_err(|e| bad(&e))?;
    r.u16()?;
    let count = r.u32()?;
    let mut m = Memory::default();
    for _ in 0..count {
        let tag: [u8; 4] = r.take(4)?.try_into().unwrap();
        let len = r.u32()? as usize;
        let mut s = R { b: r.take(len)?, p: 0 };
        match &tag {
            b"META" => {
                m.turn = s.u32()?;
                for _ in 0..s.u32()? {
                    let k = s.str()?;
                    let v = s.str()?;
                    m.meta.push((k, v));
                }
            }
            b"PERI" => {
                for _ in 0..s.u32()? {
                    m.periods.push(Period {
                        id: s.u8()?,
                        key: s.str()?,
                        name: s.str()?,
                        source: s.str()?,
                        start: s.f64()?,
                        end: s.f64()?,
                        now: s.f64()?,
                        home_x: s.f32()?,
                        persona: s.str()?,
                        scene: s.str()?,
                        opening: s.str()?,
                    });
                }
            }
            b"NODE" => {
                for _ in 0..s.u32()? {
                    let id = s.u32()?;
                    let code = s.u8()?;
                    let kind = NodeKind::from_code(code).ok_or_else(|| bad(&format!("tipo de nodo desconocido ({code})")))?;
                    let origin = Origin::from_code(s.u8()?).ok_or_else(|| bad("origen"))?;
                    let flags = s.u8()?;
                    format::check_node(version, kind, flags).map_err(|e| bad(&e))?;
                    let period = s.u8()?;
                    let x = s.f32()?;
                    let y = s.f64()?;
                    let alpha = s.f32()?;
                    let beta = s.f32()?;
                    let activation = s.f32()?;
                    let recalls = s.u32()?;
                    let created_turn = s.u32()?;
                    let key = s.str()?;
                    let label = s.str()?;
                    let text = s.str()?;
                    let mut aliases = Vec::new();
                    for _ in 0..s.u16()? {
                        aliases.push(s.str()?);
                    }
                    if id as usize != m.nodes.len() {
                        return Err(bad("ids de nodo no consecutivos"));
                    }
                    m.nodes.push(Node {
                        id,
                        kind,
                        origin,
                        flags,
                        period,
                        x,
                        y,
                        alpha,
                        beta,
                        activation,
                        recalls,
                        created_turn,
                        key,
                        label,
                        text,
                        aliases,
                        module: 0,
                        source: 0,
                        born: 0.0,
                    });
                }
            }
            b"EDGE" => {
                for _ in 0..s.u32()? {
                    let (from, to) = (s.u32()?, s.u32()?);
                    let code = s.u8()?;
                    let kind = EdgeKind::from_code(code).ok_or_else(|| bad(&format!("tipo de arista desconocido ({code})")))?;
                    format::check_edge(version, kind).map_err(|e| bad(&e))?;
                    m.edges.push(Edge { from, to, kind, alpha: s.f32()?, beta: s.f32()?, hebb: s.f32()?, created_turn: s.u32()? });
                }
            }
            b"HYPE" => {
                for _ in 0..s.u32()? {
                    m.hyperedges.push(Hyperedge {
                        id: s.u32()?,
                        label: s.str()?,
                        y: s.f64()?,
                        members: s.ids()?,
                    });
                }
            }
            b"CHAT" => {
                for _ in 0..s.u32()? {
                    let period = s.u8()?;
                    let role = Role::from_code(s.u8()?);
                    m.chat.push(ChatMessage {
                        period,
                        role,
                        turn: s.u32()?,
                        text: s.str()?,
                        recalled: s.ids()?,
                        at: 0.0,
                    });
                }
            }
            _ => {} // unknown section from a newer writer: skip
        }
        if !s.done() {
            return Err(bad(&format!("sección {} con bytes sobrantes", String::from_utf8_lossy(&tag))));
        }
    }
    let n = m.nodes.len() as u32;
    if m.edges.iter().any(|e| e.from >= n || e.to >= n)
        || m.hyperedges.iter().any(|h| h.members.iter().any(|&i| i >= n))
    {
        return Err(bad("arista apunta a un nodo inexistente"));
    }
    Ok(m)
}

pub fn save(m: &Memory, path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("cthg.tmp");
    fs::write(&tmp, encode(m))?;
    if path.exists() {
        let _ = fs::copy(path, path.with_extension("cthg.bak"));
    }
    fs::rename(&tmp, path)
}

pub fn load(path: &Path) -> io::Result<Memory> {
    decode(&fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(key: &str, kind: NodeKind, flags: u8) -> Node {
        Node {
            id: 0,
            kind,
            origin: Origin::Learned,
            flags,
            period: 0,
            x: 0.1,
            y: 2026.8,
            alpha: 2.0,
            beta: 1.0,
            activation: 0.5,
            recalls: 2,
            created_turn: 1,
            key: key.into(),
            label: key.to_uppercase(),
            text: format!("texto de {key}"),
            aliases: vec![key.into()],
            module: 0,
            source: 0,
            born: 0.0,
        }
    }

    fn with_version(bytes: &[u8], version: u16) -> Vec<u8> {
        let mut b = bytes[..bytes.len() - 4].to_vec();
        b[4..6].copy_from_slice(&version.to_le_bytes());
        let crc = crc32(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b
    }

    #[test]
    fn sectors_roundtrip_and_old_memories_stay_format_1() {
        let mut m = Memory::default();
        let legacy = encode(&m);
        assert_eq!(u16::from_le_bytes([legacy[4], legacy[5]]), format::LEGACY);
        let s1 = m.add_node(node("s-casa", NodeKind::Sector, 0));
        let s2 = m.add_node(node("s-trabajo", NodeKind::Sector, 0));
        let c = m.add_node(node("hijo", NodeKind::Fact, flags::PERSONAL_CORE));
        m.upsert_edge(c, s1, EdgeKind::BelongsTo, 7.0, 1.0);
        m.upsert_edge(c, s2, EdgeKind::BelongsTo, 2.0, 5.0);
        m.upsert_edge(s1, s2, EdgeKind::InspiredBy, 1.0, 1.0);
        let bytes = encode(&m);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), format::SECTORS);
        let back = decode(&bytes).unwrap();
        assert_eq!(format!("{:?}", m.nodes), format!("{:?}", back.nodes));
        assert_eq!(back.edges.len(), 3);
        assert_eq!((back.edges[1].kind, back.edges[1].alpha, back.edges[1].beta), (EdgeKind::BelongsTo, 2.0, 5.0));
        assert!(back.nodes[2].has(flags::PERSONAL_CORE));
        assert_eq!(decode(&legacy).unwrap().nodes.len(), 0);
    }

    #[test]
    fn unknown_versions_and_types_are_refused() {
        let mut m = Memory::default();
        m.add_node(node("s", NodeKind::Sector, 0));
        let bytes = encode(&m);
        assert!(decode(&with_version(&bytes, format::CURRENT + 1)).unwrap_err().to_string().contains("más nueva"));
        let e = decode(&with_version(&bytes, format::LEGACY)).unwrap_err().to_string();
        assert!(e.contains("sector"), "{e}");
    }
}
