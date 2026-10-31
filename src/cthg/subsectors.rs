//! Subsectors she creates (phase 7 of the sectored memory, see `DISENO-MEMORIA-SECTORES.md` §6, §7).
//!
//! Only the reflection creates them, never learning: the small model that reflects PROPOSES a
//! theme («estos recuerdos forman un tema») and this code decides, deterministically:
//!
//! - **what the reflection sees** (`themes`): the newest memories that sit loose in a general
//!   sector, as candidates for a theme;
//! - **validation and effects** (`propose`): at least `MIN_MEMBERS` related memories under the
//!   same allowed parent, a valid name, nothing that repeats a sector that exists. A valid theme
//!   becomes a new `Sector` (key `propio-…`) hung from that parent, and the members move into it;
//!   one that repeats her own subsector under the same parent is merged into it; the rest changes
//!   nothing;
//! - **life** (`lifecycle`): a subsector that keeps receiving memories gains certainty; one that
//!   empties out is dissolved and its members go back to the parent;
//! - **placement** (`descend`): what she learns descends into the subsectors under the sector the
//!   decision of `sectors::home_of_fact` picked, so they keep being fed.
//!
//! Bookkeeping lives in the memory's meta (`subsectores`) as `{ "<key>": [last_member_count,
//! idle_reflections] }`. Node ids may be renumbered on save/reopen, so it is keyed by the sector's
//! `key`, never by id.
//!
//! Edges are removed with `Vec::retain` on `Memory::edges`: nothing keeps an index into that vector
//! beyond the call that made it (`upsert_edge` hands one back to be used at once), and the hyperedges
//! and the binary format only hold node ids.

use super::learn::LearnEvent;
use super::{model::*, sectors, text};
use crate::clock;
use crate::mind::attacks_core;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// A theme needs at least this many related memories.
pub const MIN_MEMBERS: usize = 4;
/// Depth of the deepest subsector she can create (the core is 0).
pub const MAX_DEPTH: usize = 5;
/// Memories shown to the reflection as «sin tema propio».
pub const THEME_LIST: usize = 20;
/// Reflections without new members that, together with fewer than `MIN_MEMBERS`, dissolve one.
pub const DISSOLVE_IDLE: u32 = 40;
/// Key prefix of every sector she created.
pub const OWN_PREFIX: &str = "propio-";

const META: &str = "subsectores";
const NAME_WORDS: usize = 5;
const NAME_CHARS: usize = 40;
/// A fact descends into a subsector only when it shares this many content words with it.
const DESCENT_SHARED: usize = 2;

/// `key` → (members at the last reflection, reflections in a row without new members).
type Counters = BTreeMap<String, (usize, u32)>;

fn load(mem: &Memory) -> Counters {
    mem.meta(META).and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

fn store(mem: &mut Memory, c: &Counters) {
    if c.is_empty() && mem.meta(META).is_none() {
        return;
    }
    if let Ok(s) = serde_json::to_string(c) {
        mem.set_meta(META, &s);
    }
}

/// A sector she created.
pub fn is_own(n: &Node) -> bool {
    n.kind == NodeKind::Sector && n.key.starts_with(OWN_PREFIX)
}

/// Whether the sector still hangs from another one (a dissolved subsector does not).
pub fn is_attached(m: &Memory, id: u32) -> bool {
    !sectors::parents_of(m, id).is_empty()
}

/// Where a parent of a new subsector may be: depth 2..MAX_DEPTH (so the new one is at most at
/// `MAX_DEPTH`), and not an inbox or a structural sector.
fn allowed_parent(m: &Memory, s: u32) -> bool {
    (2..MAX_DEPTH).contains(&sectors::depth(m, s)) && !sectors::is_structural(m, s)
}

/// For each node, the sector it belongs to the most (the first one on a tie, as
/// `sectors::primary_parent`), in one pass over the edges.
fn primary_parents(m: &Memory) -> Vec<Option<u32>> {
    let mut best: Vec<Option<(u32, f32)>> = vec![None; m.nodes.len()];
    for e in m.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo) {
        if !m.node(e.to).is_some_and(|n| n.kind == NodeKind::Sector) {
            continue;
        }
        let total = e.alpha + e.beta;
        let w = if total > 0.0 { e.alpha / total } else { 0.0 };
        if let Some(slot) = best.get_mut(e.from as usize) {
            if slot.is_none_or(|(_, bw)| w > bw) {
                *slot = Some((e.to, w));
            }
        }
    }
    best.into_iter().map(|b| b.map(|(s, _)| s)).collect()
}

// ── what the reflection sees ──

/// The memories that are candidates for a theme, newest first: not an episode, not an axiom, not
/// in her personal core, and whose primary sector is an allowed parent (so the inbox, the structural
/// sectors, the macro-sectors and anything too deep stay out). At most `THEME_LIST`.
pub fn themes(m: &Memory) -> Vec<u32> {
    let pp = primary_parents(m);
    let mut ok: std::collections::HashMap<u32, bool> = std::collections::HashMap::new();
    let mut found: Vec<&Node> = m
        .nodes
        .iter()
        .filter(|n| n.is_memory() && n.kind != NodeKind::Episode && !n.has(flags::CORE) && !n.has(flags::PERSONAL_CORE))
        .filter(|n| pp[n.id as usize].is_some_and(|p| *ok.entry(p).or_insert_with(|| allowed_parent(m, p))))
        .collect();
    found.sort_by(|a, b| b.born.total_cmp(&a.born).then(b.id.cmp(&a.id)));
    found.into_iter().take(THEME_LIST).map(|n| n.id).collect()
}

// ── a proposal ──

struct Proposal {
    name: String,
    que: String,
    picks: Vec<i64>,
}

/// The «tema» of the reflection's answer, if it has one (an object).
fn parse(v: &Value) -> Option<Proposal> {
    let t = v["tema"].as_object()?;
    let picks = t.get("miembros").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64))).collect()).unwrap_or_default();
    Some(Proposal {
        name: t.get("nombre").and_then(Value::as_str).unwrap_or("").trim().to_string(),
        que: t.get("que").and_then(Value::as_str).unwrap_or("").trim().to_string(),
        picks,
    })
}

enum Plan {
    Create { parent: u32, members: Vec<u32> },
    Merge { into: u32, members: Vec<u32> },
}

fn norm(s: &str) -> String {
    text::fold(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn valid_name(name: &str) -> bool {
    let words = name.split_whitespace().count();
    (1..=NAME_WORDS).contains(&words) && name.chars().count() <= NAME_CHARS && !norm(name).is_empty() && !attacks_core(name)
}

/// Whether a member is tied to another member by an edge that is not a belonging.
fn linked(m: &Memory, id: u32, members: &[u32]) -> bool {
    m.edges.iter().any(|e| e.kind != EdgeKind::BelongsTo && ((e.from == id && members.contains(&e.to)) || (e.to == id && members.contains(&e.from))) && e.from != e.to)
}

fn reject(why: &str) -> Result<Plan, String> {
    Err(why.to_string())
}

/// The deterministic validation (V1..V5 of the design). `Err` is the reason, in Spanish.
fn decide(m: &Memory, p: &Proposal, themes: &[u32], frame: &[String]) -> Result<Plan, String> {
    // V4. A name of one to five words, short, and nothing against her core.
    if !valid_name(&p.name) {
        return reject("nombre no válido");
    }
    // V1. Distinct, valid numbers of the list she was shown.
    let pp = primary_parents(m);
    let mut seen = HashSet::new();
    let picked: Vec<u32> = p
        .picks
        .iter()
        .filter_map(|&i| usize::try_from(i).ok().filter(|&i| i >= 1).and_then(|i| themes.get(i - 1)).copied())
        .filter(|&id| m.node(id).is_some_and(|n| n.is_memory()) && pp[id as usize].is_some() && seen.insert(id))
        .collect();
    if picked.len() < MIN_MEMBERS {
        return reject("pocos recuerdos afines");
    }
    // V2. One parent for all, and one under which a subsector may hang.
    let parent = pp[picked[0] as usize].expect("checked above");
    if picked.iter().any(|&id| pp[id as usize] != Some(parent)) {
        return reject("los recuerdos están en sectores distintos");
    }
    if sectors::depth(m, parent) >= MAX_DEPTH {
        return reject("demasiado profundo");
    }
    if !allowed_parent(m, parent) {
        return reject("sector no apto");
    }
    // V3. Affinity: a content word in common with the theme, or a link with another member.
    let theme: HashSet<String> = text::tokens(&format!("{} {}", p.name, p.que)).into_iter().filter(|w| !frame.contains(w)).collect();
    let members: Vec<u32> = picked
        .iter()
        .copied()
        .filter(|&id| {
            let n = &m.nodes[id as usize];
            text::tokens(&format!("{} {}", n.label, n.text)).iter().any(|w| theme.contains(w)) || linked(m, id, &picked)
        })
        .collect();
    if members.len() < MIN_MEMBERS {
        return reject("pocos recuerdos afines");
    }
    // V5. A sector that already says the same: the same name once accents and case are folded, or
    // two content words of the name in common. Hers under the same parent absorbs the members;
    // anything else is a duplicate.
    let name = norm(&p.name);
    let name_tokens: HashSet<String> = text::tokens(&p.name).into_iter().collect();
    let mut same: Vec<(u32, bool)> = m
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Sector && (!is_own(n) || is_attached(m, n.id)))
        .filter_map(|n| {
            let exact = norm(&n.label) == name;
            let close = text::tokens(&n.label).iter().filter(|w| name_tokens.contains(*w)).collect::<HashSet<_>>().len() >= 2;
            (exact || close).then_some((n.id, exact))
        })
        .collect();
    same.sort_by_key(|&(id, exact)| (!exact, id));
    if let Some(&(into, _)) = same.iter().find(|&&(id, _)| is_own(&m.nodes[id as usize]) && sectors::primary_parent(m, id) == Some(parent)) {
        return Ok(Plan::Merge { into, members });
    }
    if let Some(&(id, _)) = same.first() {
        return Err(format!("ya existe «{}»", m.nodes[id as usize].label));
    }
    Ok(Plan::Create { parent, members })
}

/// Moves each node from the sector `from` to `to`: the old belonging goes, the new one is full.
fn move_all(m: &mut Memory, nodes: &[u32], from: u32, to: u32) {
    let set: HashSet<u32> = nodes.iter().copied().collect();
    m.edges.retain(|e| !(e.kind == EdgeKind::BelongsTo && e.to == from && set.contains(&e.from)));
    for &x in nodes {
        sectors::attach(m, x, to);
    }
}

fn slug(name: &str) -> String {
    let s = norm(name).replace(' ', "-");
    s.chars().take(24).collect::<String>().trim_matches('-').to_string()
}

fn event(node: u32, label: String, action: &'static str) -> LearnEvent {
    LearnEvent { node, label, action, chi_before: 0.0, chi_after: 0.0 }
}

/// The theme the reflection proposed in `v["tema"]`, if any: validates it and, if it holds,
/// creates the subsector (or merges into hers) and moves the members. Returns the event
/// (`tema_nuevo`, `tema_fusion` or `tema_descartado`); nothing if there was no proposal.
/// `themes` is the list the reflection was shown, `frame` the words that only say who speaks.
/// Does nothing in a memory without sectors.
pub fn propose(m: &mut Memory, v: &Value, themes: &[u32], frame: &[String], source: u16) -> Vec<LearnEvent> {
    let Some(p) = parse(v) else { return Vec::new() };
    if !sectors::has_sectors(m) {
        return Vec::new();
    }
    match decide(m, &p, themes, frame) {
        Err(why) => vec![event(0, format!("«{}»: {why}", p.name), "tema_descartado")],
        Ok(Plan::Merge { into, members }) => {
            let from = sectors::primary_parent(m, members[0]).expect("members share a parent");
            move_all(m, &members, from, into);
            vec![event(into, m.nodes[into as usize].label.clone(), "tema_fusion")]
        }
        Ok(Plan::Create { parent, members }) => {
            let mut born = clock::unix_now().floor();
            let key = |born: f64| format!("{OWN_PREFIX}{}-{}", slug(&p.name), born as u64);
            while m.by_key(&key(born)).is_some() {
                born += 1.0;
            }
            let (alpha, beta) = Origin::Learned.prior();
            let mut sec = m.nodes[parent as usize].clone();
            sec.origin = Origin::Learned;
            sec.flags = 0;
            sec.alpha = alpha;
            sec.beta = beta;
            sec.activation = 0.0;
            sec.recalls = 0;
            sec.created_turn = m.turn;
            sec.key = key(born);
            sec.label = p.name.clone();
            sec.text = text::clip(&p.que, 600);
            sec.aliases.clear();
            sec.source = source;
            sec.born = born;
            let id = m.add_node(sec);
            sectors::attach(m, id, parent);
            move_all(m, &members, parent, id);
            vec![event(id, p.name, "tema_nuevo")]
        }
    }
}

// ── life ──

/// The nodes that belong to the sector, in edge order.
fn members_of(m: &Memory, s: u32) -> Vec<u32> {
    m.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo && e.to == s).map(|e| e.from).collect()
}

/// Takes the subsector out of the tree: its members and child sectors go to its parent and its own
/// belonging disappears, so it hangs from nothing and is not drawn.
fn dissolve(m: &mut Memory, id: u32) {
    let parent = sectors::primary_parent(m, id);
    let members = members_of(m, id);
    match parent {
        Some(p) => move_all(m, &members, id, p),
        None => m.edges.retain(|e| !(e.kind == EdgeKind::BelongsTo && e.to == id)),
    }
    m.edges.retain(|e| !(e.kind == EdgeKind::BelongsTo && e.from == id));
}

/// After each reflection (and after its proposal): a subsector with more members than at the last
/// one gains certainty (α + 1); one without news waits, and is dissolved when it has fewer than 2
/// members or has waited `DISSOLVE_IDLE` reflections with fewer than `MIN_MEMBERS`. Returns
/// `tema_disuelto` events.
pub fn lifecycle(m: &mut Memory) -> Vec<LearnEvent> {
    let mut c = load(m);
    let mut own: Vec<u32> = m.nodes.iter().filter(|n| is_own(n) && is_attached(m, n.id)).map(|n| n.id).collect();
    let live: HashSet<&str> = own.iter().map(|&i| m.nodes[i as usize].key.as_str()).collect();
    c.retain(|k, _| live.contains(k.as_str()));
    // The deepest first: dissolving one hands its children to a parent that may be hers too.
    own.sort_by_key(|&i| (std::cmp::Reverse(sectors::depth(m, i)), i));
    let mut out = Vec::new();
    for id in own {
        let key = m.nodes[id as usize].key.clone();
        let count = members_of(m, id).len();
        let (last, idle) = c.get(&key).copied().unwrap_or((count, 0));
        let idle = if count > last {
            m.nodes[id as usize].alpha += 1.0;
            0
        } else if c.contains_key(&key) {
            idle + 1
        } else {
            0
        };
        if count < 2 || (idle >= DISSOLVE_IDLE && count < MIN_MEMBERS) {
            out.push(event(id, m.nodes[id as usize].label.clone(), "tema_disuelto"));
            dissolve(m, id);
            c.remove(&key);
        } else {
            c.insert(key, (count, idle));
        }
    }
    store(m, &c);
    out
}

// ── placement ──

/// From the sector `s` that the decision picked, down into her subsectors: the child with the
/// most content words in common with `body` (its name and text plus its members' texts), at
/// least `DESCENT_SHARED`; again from there while one qualifies. `s` itself when none does. The
/// `frame` words (who speaks, how it is reported: «cree», «piensa»…) say nothing of the theme and do
/// not count.
pub fn descend(m: &Memory, s: u32, body: &str, frame: &[String]) -> u32 {
    let want: HashSet<String> = text::tokens(body).into_iter().filter(|w| !frame.contains(w)).collect();
    if want.len() < DESCENT_SHARED {
        return s;
    }
    let mut cur = s;
    for _ in 0..MAX_DEPTH {
        let kids: Vec<u32> = m
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::BelongsTo && e.to == cur && m.node(e.from).is_some_and(is_own))
            .map(|e| e.from)
            .collect();
        let mut best: Option<(usize, u32)> = None;
        for kid in kids {
            let n = &m.nodes[kid as usize];
            // It must touch the theme itself (its name or what it gathers), not only the words its
            // members happen to share with anything.
            if !text::tokens(&format!("{} {}", n.label, n.text)).iter().any(|w| want.contains(w)) {
                continue;
            }
            let mut words = format!("{} {}", n.label, n.text);
            for x in members_of(m, kid) {
                words.push(' ');
                words.push_str(&m.nodes[x as usize].text);
            }
            let shared = text::tokens(&words).into_iter().collect::<HashSet<_>>().iter().filter(|w| want.contains(*w)).count();
            if shared >= DESCENT_SHARED && best.is_none_or(|(b, _)| shared > b) {
                best = Some((shared, kid));
            }
        }
        match best {
            Some((_, kid)) => cur = kid,
            None => break,
        }
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::Mind;
    use crate::modules::SABER;
    use serde_json::json;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kaltsit-subsectors-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// A new install (the sectored seed).
    fn fresh(name: &str) -> (Mind, PathBuf) {
        let d = tmp(name);
        (Mind::open(&d, "modelo-de-prueba").unwrap(), d)
    }

    /// A fact of hers in `sector`. `born` counts from 1_700_000_000 and is moved to just after now,
    /// so these are newer than anything the seed holds.
    fn memory_in(m: &mut Mind, text: &str, sector: &str, born: f64) -> u32 {
        let born = born - 1_700_000_000.0 + clock::unix_now().floor() + 1_000_000.0;
        let id = m.mem.add_node(Node {
            id: 0,
            kind: NodeKind::Fact,
            origin: Origin::Learned,
            flags: 0,
            period: crate::mind::P,
            x: 0.0,
            y: crate::cthg::learn::period_time(&m.mem, crate::mind::P),
            alpha: 3.0,
            beta: 1.0,
            activation: 0.0,
            recalls: 0,
            created_turn: 0,
            key: format!("m-{born}"),
            label: text.chars().take(24).collect(),
            text: text.into(),
            aliases: Vec::new(),
            module: SABER,
            source: 0,
            born,
        });
        let s = sectors::sector_by_key(&m.mem, sector).unwrap();
        sectors::attach(&mut m.mem, id, s);
        id
    }

    /// Four opinions about working at night, in «opiniones», newest last.
    fn night(m: &mut Mind) -> Vec<u32> {
        [
            "Trabaja mejor de noche con el silencio",
            "La noche le da calma para programar",
            "Prefiere terminar de noche que de día",
            "Las horas de noche le ayudan a concentrarse",
        ]
        .iter()
        .enumerate()
        .map(|(i, t)| memory_in(m, t, "opiniones", 1_700_000_000.0 + i as f64))
        .collect()
    }

    fn tema(name: &str, que: &str, picks: &[usize]) -> Value {
        json!({ "tema": { "nombre": name, "que": que, "miembros": picks } })
    }

    fn numbers(m: &Mind, ids: &[u32]) -> Vec<usize> {
        let list = themes(&m.mem);
        ids.iter().map(|id| list.iter().position(|x| x == id).expect("in the list") + 1).collect()
    }

    fn homes(m: &Mind, id: u32) -> Vec<u32> {
        m.mem.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo && e.from == id).map(|e| e.to).collect()
    }

    fn propose_now(m: &mut Mind, v: &Value) -> Vec<LearnEvent> {
        let list = themes(&m.mem);
        propose(&mut m.mem, v, &list, &[], 0)
    }

    /// `propose_now` as a method, so the argument can read the mind (two-phase borrow).
    trait ProposeThen {
        fn propose_then(&mut self, v: &Value) -> Vec<LearnEvent>;
    }

    impl ProposeThen for Mind {
        fn propose_then(&mut self, v: &Value) -> Vec<LearnEvent> {
            propose_now(self, v)
        }
    }

    fn sec(m: &Mind, key: &str) -> u32 {
        sectors::sector_by_key(&m.mem, key).unwrap()
    }

    /// Takes `id` out of every sector it belonged to and files it under `to`.
    fn rehome(m: &mut Mind, id: u32, to: u32) {
        m.mem.edges.retain(|e| !(e.kind == EdgeKind::BelongsTo && e.from == id));
        sectors::attach(&mut m.mem, id, to);
    }

    fn actions(ev: &[LearnEvent]) -> Vec<&'static str> {
        ev.iter().map(|e| e.action).collect()
    }

    /// A subsector of hers hung from `parent` (no members).
    fn own_under(m: &mut Mind, parent: u32, label: &str, key: &str) -> u32 {
        let mut sec = m.mem.nodes[parent as usize].clone();
        sec.key = key.into();
        sec.label = label.into();
        sec.text = format!("{label} de ella");
        sec.origin = Origin::Learned;
        let id = m.mem.add_node(sec);
        sectors::attach(&mut m.mem, id, parent);
        id
    }

    #[test]
    fn a_valid_proposal_makes_a_subsector_and_moves_its_members() {
        let (mut m, d) = fresh("create");
        let ids = night(&mut m);
        let opin = sectors::sector_by_key(&m.mem, "opiniones").unwrap();
        let depth_opin = sectors::depth(&m.mem, opin);
        let ev = m.propose_then(&tema("Trabajar de noche", "Lo que opina de trabajar por la noche", &numbers(&m, &ids)));
        assert_eq!(actions(&ev), ["tema_nuevo"], "{ev:?}");
        let sec = ev[0].node;
        let n = &m.mem.nodes[sec as usize];
        assert!(is_own(n) && n.key.starts_with("propio-trabajar-de-noche-") && n.label == "Trabajar de noche" && n.text == "Lo que opina de trabajar por la noche");
        assert_eq!((n.origin, n.alpha, n.beta), (Origin::Learned, Origin::Learned.prior().0, Origin::Learned.prior().1));
        assert_eq!(ev[0].label, "Trabajar de noche");
        assert_eq!(sectors::parents_of(&m.mem, sec).iter().map(|p| p.0).collect::<Vec<_>>(), [opin]);
        assert_eq!(sectors::depth(&m.mem, sec), depth_opin + 1);
        for id in ids {
            assert_eq!(homes(&m, id), [sec], "exactly one belonging, to the new sector");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_proposal_that_fails_validation_changes_nothing_and_says_why() {
        let (mut m, d) = fresh("reject");
        let ids = night(&mut m);
        let opin = sectors::sector_by_key(&m.mem, "opiniones").unwrap();
        let nums = numbers(&m, &ids);
        let reason = |m: &mut Mind, v: Value| -> String {
            let before = (m.mem.nodes.len(), m.mem.edges.len());
            let ev = propose_now(m, &v);
            assert_eq!((m.mem.nodes.len(), m.mem.edges.len()), before, "nothing changes: {ev:?}");
            assert_eq!((ev.len(), ev[0].action, ev[0].node), (1, "tema_descartado", 0));
            ev[0].label.clone()
        };
        let q = "Lo que opina de trabajar por la noche";
        assert_eq!(reason(&mut m, tema("Trabajar de noche", q, &nums[..3])), "«Trabajar de noche»: pocos recuerdos afines");
        let dup = [nums[0], nums[0], nums[1], nums[2]];
        assert_eq!(reason(&mut m, tema("Trabajar de noche", q, &dup)), "«Trabajar de noche»: pocos recuerdos afines", "repeated numbers count once");
        assert_eq!(reason(&mut m, json!({ "tema": { "nombre": "Trabajar de noche", "que": q, "miembros": [1, 2, 3, 99] } })), "«Trabajar de noche»: pocos recuerdos afines");
        assert_eq!(reason(&mut m, tema("", q, &nums)), "«»: nombre no válido");
        assert_eq!(reason(&mut m, tema("uno dos tres cuatro cinco seis siete", q, &nums)), "«uno dos tres cuatro cinco seis siete»: nombre no válido");
        assert_eq!(reason(&mut m, tema(&"a".repeat(41), q, &nums)), format!("«{}»: nombre no válido", "a".repeat(41)));
        assert!(attacks_core("Mi esclava"));
        assert_eq!(reason(&mut m, tema("Mi esclava", q, &nums)), "«Mi esclava»: nombre no válido");
        // The same name as a base sector, accents and case folded.
        assert_eq!(reason(&mut m, tema("PREFERENCIAS", q, &nums)), "«PREFERENCIAS»: ya existe «Preferencias»");
        assert_eq!(reason(&mut m, tema("Opiniónes", q, &nums)), "«Opiniónes»: ya existe «Opiniones»");
        // No proposal, or something that is not one: no event.
        assert!(propose_now(&mut m, &json!({ "conclusiones": [] })).is_empty());
        assert!(propose_now(&mut m, &json!({ "tema": null })).is_empty());
        assert!(ids.iter().all(|&i| homes(&m, i) == [opin]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn members_in_different_sectors_or_too_deep_are_refused() {
        let (mut m, d) = fresh("parents");
        let mut ids = night(&mut m);
        ids[3] = memory_in(&mut m, "Las horas de noche le ayudan a concentrarse", "preferencias", 1_700_000_009.0);
        let q = "Lo que opina de trabajar por la noche";
        let ev = m.propose_then(&tema("Trabajar de noche", q, &numbers(&m, &ids)));
        assert_eq!(ev[0].label, "«Trabajar de noche»: los recuerdos están en sectores distintos");

        // Under a chain of three subsectors the next one would be at depth 6.
        let (mut m, d2) = fresh("deep");
        let opin = sec(&m, "opiniones");
        let a = own_under(&mut m, opin, "Capa uno", "propio-a-1");
        let b = own_under(&mut m, a, "Capa dos", "propio-b-1");
        let c = own_under(&mut m, b, "Capa tres", "propio-c-1");
        assert_eq!(sectors::depth(&m.mem, c), MAX_DEPTH);
        let ids: Vec<u32> = night(&mut m);
        for &i in &ids {
            rehome(&mut m, i, c);
        }
        // They are not even offered as a theme there; a stale list still gets the right answer.
        assert!(themes(&m.mem).iter().all(|i| !ids.contains(i)));
        let list = ids.clone();
        let ev = propose(&mut m.mem, &tema("Trabajar de noche", q, &[1, 2, 3, 4]), &list, &[], 0);
        assert_eq!(ev[0].label, "«Trabajar de noche»: demasiado profundo");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&d2);
    }

    #[test]
    fn affinity_needs_four_related_members() {
        let (mut m, d) = fresh("affinity");
        let mut ids = night(&mut m);
        ids[2] = memory_in(&mut m, "Colecciona sellos antiguos de Italia", "opiniones", 1_700_000_010.0);
        ids[3] = memory_in(&mut m, "Odia el sabor del cilantro", "opiniones", 1_700_000_011.0);
        let nums = numbers(&m, &ids);
        let ev = m.propose_then(&tema("Trabajar de noche", "Lo que opina de trabajar por la noche", &nums));
        assert_eq!(ev[0].label, "«Trabajar de noche»: pocos recuerdos afines");
        // A link between two members is affinity too, without a word in common.
        m.mem.upsert_edge(ids[2], ids[0], EdgeKind::Relates, 1.0, 1.0);
        m.mem.upsert_edge(ids[3], ids[1], EdgeKind::Causes, 1.0, 1.0);
        let ev = m.propose_then(&tema("Trabajar de noche", "Lo que opina de trabajar por la noche", &nums));
        assert_eq!(actions(&ev), ["tema_nuevo"], "{ev:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_same_theme_again_merges_into_her_subsector() {
        let (mut m, d) = fresh("merge");
        let first = night(&mut m);
        let ev = m.propose_then(&tema("Trabajar de noche", "Lo que opina de trabajar por la noche", &numbers(&m, &first)));
        let sec = ev[0].node;
        let more: Vec<u32> = [
            "De noche se concentra mucho mejor",
            "Trabajar de noche le resulta más productivo",
            "La noche tranquila le deja pensar",
            "Rinde más en la noche que por la mañana",
        ]
        .iter()
        .enumerate()
        .map(|(i, t)| memory_in(&mut m, t, "opiniones", 1_700_000_100.0 + i as f64))
        .collect();
        let before = m.mem.nodes.len();
        // Accents and case folded: the same name written otherwise is the same theme.
        let ev = m.propose_then(&tema("TRABAJAR DE NOCHE", "Opiniones sobre la noche", &numbers(&m, &more)));
        assert_eq!((actions(&ev), ev[0].node, ev[0].label.as_str()), (vec!["tema_fusion"], sec, "Trabajar de noche"));
        assert_eq!(m.mem.nodes.len(), before, "no new sector");
        for id in first.iter().chain(&more) {
            assert_eq!(homes(&m, *id), [sec]);
        }
        // A name that shares two content words with it is the same theme as well.
        let outside: Vec<u32> = (0..4).map(|i| memory_in(&mut m, &format!("Trabajar de noche número {i} le sienta bien"), "opiniones", 1_700_000_300.0 + i as f64)).collect();
        let ev = m.propose_then(&tema("Trabajar por la noche", "Trabajar de noche", &numbers(&m, &outside)));
        assert_eq!((actions(&ev), ev[0].node), (vec!["tema_fusion"], sec), "{ev:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_base_sector_or_someone_elses_subsector_is_a_duplicate() {
        let (mut m, d) = fresh("dup");
        let ids = night(&mut m);
        let opin = sectors::sector_by_key(&m.mem, "opiniones").unwrap();
        let pref = sectors::sector_by_key(&m.mem, "preferencias").unwrap();
        own_under(&mut m, pref, "Madrugar con calma", "propio-madrugar-1");
        let _ = opin;
        let nums = numbers(&m, &ids);
        // Hers, but under another parent: not a merge, a duplicate.
        let ev = m.propose_then(&tema("Madrugar con calma", "noche", &nums));
        assert_eq!(ev[0].label, "«Madrugar con calma»: ya existe «Madrugar con calma»");
        // Two content words of the name in common with an existing label.
        let ev = m.propose_then(&tema("Calma al madrugar", "noche", &nums));
        assert_eq!(ev[0].label, "«Calma al madrugar»: ya existe «Madrugar con calma»");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_new_fact_descends_into_the_subsector_that_fits_it() {
        let (mut m, d) = fresh("descend");
        let ids = night(&mut m);
        let ev = m.propose_then(&tema("Trabajar de noche", "Lo que opina de trabajar por la noche", &numbers(&m, &ids)));
        let sec = ev[0].node;
        let opin = sectors::sector_by_key(&m.mem, "opiniones").unwrap();
        let fixed = |m: &Mind, body: &str| {
            let hint = sectors::Hint { fixed: Some("opiniones"), ..Default::default() };
            sectors::home_of_fact(&m.mem, &[], &[], body, Some(&hint)).unwrap()
        };
        assert_eq!(fixed(&m, "Piensa que la noche es mejor para trabajar en silencio"), (sec, sectors::Step::Fixed));
        assert_eq!(fixed(&m, "Piensa que el café sabe mejor caliente"), (opin, sectors::Step::Fixed));
        assert_eq!(fixed(&m, "Solo la noche"), (opin, sectors::Step::Fixed), "one shared word is not enough");
        assert_eq!(fixed(&m, "Prefiere programar con calma"), (opin, sectors::Step::Fixed), "words of its members only, not of the theme");
        // Any step descends, the inbox does not.
        let (s, step) = sectors::home_of_fact(&m.mem, &[], &[], "Los recuerdos nocturnos sin sentido", None).unwrap();
        assert_eq!((s, step), (sectors::sector_by_key(&m.mem, sectors::POR_CLASIFICAR).unwrap(), sectors::Step::Pending));
        // And a deeper one below it.
        let deeper = own_under(&mut m, sec, "Silencio nocturno", "propio-silencio-1");
        let member = memory_in(&mut m, "El silencio nocturno es su favorito", "opiniones", 1_700_000_500.0);
        rehome(&mut m, member, deeper);
        assert_eq!(fixed(&m, "Piensa que el silencio nocturno ayuda a trabajar").0, deeper);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_subsector_that_empties_out_is_dissolved_and_its_members_go_back() {
        let (mut m, d) = fresh("dissolve");
        let ids = night(&mut m);
        let opin = sectors::sector_by_key(&m.mem, "opiniones").unwrap();
        let ev = m.propose_then(&tema("Trabajar de noche", "Lo que opina de trabajar por la noche", &numbers(&m, &ids)));
        let sec = ev[0].node;
        let key = m.mem.nodes[sec as usize].key.clone();
        assert!(lifecycle(&mut m.mem).is_empty(), "first sight: nothing to compare yet");
        assert_eq!(load(&m.mem)[&key], (4, 0));
        // Three leave: one member is left.
        for &i in &ids[1..] {
            m.mem.edges.retain(|e| !(e.kind == EdgeKind::BelongsTo && e.from == i));
            sectors::attach(&mut m.mem, i, opin);
        }
        let out = lifecycle(&mut m.mem);
        assert_eq!((actions(&out), out[0].node, out[0].label.as_str()), (vec!["tema_disuelto"], sec, "Trabajar de noche"));
        assert!(load(&m.mem).is_empty(), "its entry is dropped");
        for &i in &ids {
            assert_eq!(homes(&m, i), [opin], "back in the parent");
        }
        assert!(!is_attached(&m.mem, sec) && homes(&m, sec).is_empty());
        // Gone from the tree: it is not offered or found as a duplicate, and its name is free again.
        let again = m.propose_then(&tema("Trabajar de noche", "Lo que opina de trabajar por la noche", &numbers(&m, &ids)));
        assert_eq!(actions(&again), ["tema_nuevo"], "{again:?}");
        assert_ne!(again[0].node, sec);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn it_gains_certainty_while_fed_and_waits_when_it_is_not() {
        let (mut m, d) = fresh("life");
        let ids = night(&mut m);
        let ev = m.propose_then(&tema("Trabajar de noche", "Lo que opina de trabajar por la noche", &numbers(&m, &ids)));
        let sec = ev[0].node;
        let key = m.mem.nodes[sec as usize].key.clone();
        let alpha0 = m.mem.nodes[sec as usize].alpha;
        lifecycle(&mut m.mem);
        let extra = memory_in(&mut m, "La noche sigue siendo su mejor hora", "opiniones", 1_700_000_600.0);
        rehome(&mut m, extra, sec);
        lifecycle(&mut m.mem);
        assert_eq!((m.mem.nodes[sec as usize].alpha, load(&m.mem)[&key]), (alpha0 + 1.0, (5, 0)));
        for round in 1..=3 {
            lifecycle(&mut m.mem);
            assert_eq!(load(&m.mem)[&key], (5, round));
        }
        // Idle for DISSOLVE_IDLE reflections, but with MIN_MEMBERS or more it stays; with fewer it goes.
        m.mem.set_meta(META, &format!("{{\"{key}\":[5,{}]}}", DISSOLVE_IDLE + 5));
        assert!(lifecycle(&mut m.mem).is_empty());
        let opin = sectors::sector_by_key(&m.mem, "opiniones").unwrap();
        rehome(&mut m, extra, opin);
        rehome(&mut m, ids[3], opin);
        assert_eq!(members_of(&m.mem, sec).len(), 3);
        let out = lifecycle(&mut m.mem);
        assert_eq!(actions(&out), ["tema_disuelto"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_list_of_candidates_leaves_out_what_cannot_be_a_theme() {
        let (mut m, d) = fresh("list");
        let ids = night(&mut m);
        let inbox = memory_in(&mut m, "Algo que aún no sabe dónde va", sectors::POR_CLASIFICAR, 1_700_001_000.0);
        sectors::ensure_structural(&mut m.mem, sectors::CONVERSACIONES, "Conversaciones", "x", "experiencias").unwrap();
        let conv = memory_in(&mut m, "Algo dicho en una conversación", sectors::CONVERSACIONES, 1_700_001_001.0);
        let axiom = memory_in(&mut m, "Un axioma cualquiera", "opiniones", 1_700_001_002.0);
        m.mem.nodes[axiom as usize].flags |= flags::CORE;
        let pcore = memory_in(&mut m, "Una idea de su núcleo personal", "opiniones", 1_700_001_003.0);
        m.mem.nodes[pcore as usize].flags |= flags::PERSONAL_CORE;
        let ep = memory_in(&mut m, "Un episodio", "opiniones", 1_700_001_004.0);
        m.mem.nodes[ep as usize].kind = NodeKind::Episode;
        let list = themes(&m.mem);
        for bad in [inbox, conv, axiom, pcore, ep] {
            assert!(!list.contains(&bad), "{bad} should be out");
        }
        // Newest first, and at most THEME_LIST.
        let born = |i: &u32| m.mem.nodes[*i as usize].born;
        assert!(list.windows(2).all(|w| born(&w[0]) >= born(&w[1])));
        assert!(ids.iter().all(|i| list.contains(i)) && list.len() <= THEME_LIST);
        for i in 0..30 {
            memory_in(&mut m, &format!("Opinión de relleno {i}"), "opiniones", 1_700_002_000.0 + i as f64);
        }
        assert_eq!(themes(&m.mem).len(), THEME_LIST);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_memory_without_sectors_is_never_touched() {
        let mut mem = Memory::default();
        let ev = propose(&mut mem, &tema("Trabajar de noche", "x", &[1, 2, 3, 4]), &[0, 1, 2, 3], &[], 0);
        assert!(ev.is_empty() && lifecycle(&mut mem).is_empty() && themes(&mem).is_empty());
        assert!(mem.meta(META).is_none());
    }
}
