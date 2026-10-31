//! Self-learning. Three plasticity rules, one per axis:
//!
//! - Y (time) is immutable and forward-only: every turn is written *after* the period's present
//!   and nothing already written can be moved or erased. Canon cannot be overturned.
//! - X (space/association) is plastic: entities that appear together get Hebbian links,
//!   activation decays every turn and, while the autonomous clock runs, with time (forgetting
//!   by inattention: the less she understands a memory, the faster it fades); recall restores it.
//! - Z (causality, χ) moves only through evidence: claims that agree add α, denials add β,
//!   claims against anchored canon are pushed out of the system (toward −1).

use super::model::*;
use super::score::{self, Score};
use super::sectors;
use super::text;

const ACTIVATION_DECAY: f32 = 0.92;
const HEBB_RATE: f32 = 0.15;
const HEBB_DECAY: f32 = 0.995;
/// Her life runs on the real calendar: a step of about three seconds.
const TURN_STEP: f64 = 1e-7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    Affirms,
    Denies,
}

/// A statement extracted from the user's message. `supports` / `contradicts` are node ids.
#[derive(Clone, Debug)]
pub struct Claim {
    pub text: String,
    pub entities: Vec<String>,
    pub stance: Stance,
    pub cause: Option<String>,
    pub supports: Option<u32>,
    pub contradicts: Option<u32>,
    pub supersedes: Option<u32>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct LearnEvent {
    pub node: u32,
    pub label: String,
    /// "nuevo", "refuerza", "debilita", "rechazado", "consolidado", "causa", "se cumple"
    pub action: &'static str,
    pub chi_before: f32,
    pub chi_after: f32,
}

/// Next instant on the period's timeline (strictly after everything already written).
pub fn period_time(m: &Memory, period: u8) -> f64 {
    m.horizon(period) + TURN_STEP
}

/// A possible future (canon after the chronicle's start) that this claim describes.
fn find_potential(m: &Memory, toks: &[String], min: f32) -> Option<u32> {
    m.nodes
        .iter()
        .filter(|n| n.has(flags::POTENTIAL) && matches!(n.kind, NodeKind::Event | NodeKind::Fact))
        .map(|n| (n.id, text::similarity(toks, &text::tokens(&format!("{} {}", n.label, n.text)))))
        .filter(|&(_, j)| j >= min)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(id, _)| id)
}

fn place_x(m: &Memory, ids: &[u32], period: u8, salt: &str) -> f32 {
    let xs: Vec<f32> = ids.iter().filter_map(|&i| m.node(i)).map(|n| n.x).collect();
    let base = if xs.is_empty() {
        // Where she is right now, if she moved on her own; otherwise the period's home.
        m.meta("vital.x").and_then(|s| s.parse().ok()).unwrap_or(m.periods[period as usize].home_x)
    } else {
        xs.iter().sum::<f32>() / xs.len() as f32
    };
    (base + 0.06 * text::hash_unit(salt, 7)).clamp(-1.0, 1.0)
}

fn new_node(m: &Memory, kind: NodeKind, origin: Origin, period: u8, x: f32, y: f64, label: &str, body: &str) -> Node {
    let (alpha, beta) = origin.prior();
    Node {
        id: 0,
        kind,
        origin,
        flags: 0,
        period,
        x,
        y,
        alpha,
        beta,
        activation: 1.0,
        recalls: 0,
        created_turn: m.turn,
        key: String::new(),
        label: text::clip(label, 64),
        text: text::clip(body, 600),
        aliases: vec![],
        module: 0,
        source: 0,
        born: 0.0,
    }
}

fn hebbian(m: &mut Memory, a: u32, b: u32) {
    if a == b {
        return;
    }
    let (a, b) = if a < b { (a, b) } else { (b, a) };
    let i = match m.find_edge(a, b, EdgeKind::Relates) {
        Some(i) => i,
        None => m.upsert_edge(a, b, EdgeKind::Relates, 1.0, 1.0),
    };
    let e = &mut m.edges[i];
    e.hebb += HEBB_RATE * (1.0 - e.hebb);
}

/// Writes the turn as an episode node and applies the X-axis rules. Returns the episode id.
pub fn record_turn(m: &mut Memory, period: u8, user: &str, reply: &str, recalled: &[u32]) -> u32 {
    let body = if reply.is_empty() { user.to_string() } else { format!("{user}
— Kal'tsit: {reply}") };
    record_episode(m, period, user, &body, &format!("{user}
{reply}"), &[], recalled)
}

/// An episode in the present of `period`: a turn, a move she made alone, a sentence left
/// half-said. `about` is scanned for entities; `places` are linked as where it happened.
pub fn record_episode(m: &mut Memory, period: u8, label: &str, body: &str, about: &str, places: &[u32], recalled: &[u32]) -> u32 {
    m.turn += 1;
    let scores = score::score_all(m, m.horizon(period));

    for n in m.nodes.iter_mut() {
        n.activation *= ACTIVATION_DECAY;
    }
    for e in m.edges.iter_mut() {
        if e.kind == EdgeKind::Relates && e.created_turn > 0 {
            e.hebb *= HEBB_DECAY;
        }
    }

    let mut entities = super::retrieve::mentioned_entities(m, &scores, about);
    entities.extend_from_slice(places);
    entities.sort_unstable();
    entities.dedup();

    let y = period_time(m, period);
    let x = if places.is_empty() { place_x(m, &entities, period, label) } else { place_x(m, places, period, label) };
    let ep = new_node(m, NodeKind::Episode, Origin::Chat, period, x, y, label, body);
    let ep = m.add_node(ep);
    // Conversations to «Conversaciones», what she saw and her own comments to «Lo que vio»
    // (both made on demand under «Experiencias»), so no episode is left without a sector.
    sectors::file_episode(m, ep, label);

    let prev = m
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Episode && n.period == period && n.id != ep)
        .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
        .map(|n| n.id);
    if let Some(prev) = prev {
        m.upsert_edge(prev, ep, EdgeKind::Precedes, 1.0, 1.0);
    }
    for &id in &entities {
        m.upsert_edge(ep, id, EdgeKind::Mentions, 1.0, 1.0);
    }
    // Structure is not something that was recalled: a sector gets no link and no activation.
    let recalled: Vec<u32> = recalled.iter().copied().filter(|&id| is_memory(m, id)).collect();
    for &id in recalled.iter().take(4) {
        m.upsert_edge(ep, id, EdgeKind::Mentions, 1.0, 1.0);
    }
    for (i, &a) in entities.iter().enumerate() {
        for &b in &entities[i + 1..] {
            hebbian(m, a, b);
        }
    }
    for &id in entities.iter() {
        if let Some(n) = m.node_mut(id) {
            n.activation = 1.0;
            n.recalls += 1;
        }
    }
    for &id in recalled.iter() {
        if let Some(n) = m.node_mut(id) {
            n.activation = (n.activation + 0.3).min(1.0);
            n.recalls += 1;
        }
    }
    ep
}

/// Forgetting by inattention, `dt` seconds of the autonomous clock. Activation fades with a
/// time constant from 2 min (a memory she finds no sense in) to 20 min (one she fully
/// understands); the identity core never fades. Recall and reinforcement bring it back to 1.
pub fn fade(m: &mut Memory, period: u8, dt: f64) {
    let scores = score::score_all(m, m.horizon(period));
    for n in m.nodes.iter_mut() {
        if n.has(flags::CORE) || n.activation <= 1e-3 {
            continue;
        }
        let u = scores.get(n.id as usize).filter(|s| s.visible).map(|s| (s.chi + 1.0) / 2.0).unwrap_or(0.0);
        let tau = 120.0 + 1080.0 * u * u;
        n.activation *= (-(dt as f32) / tau).exp();
    }
}

/// Whether `id` is a memory that learning may reinforce, weaken or lean on. A sector is
/// structure: it is never any of those, whatever the analyst numbered.
fn is_memory(m: &Memory, id: u32) -> bool {
    m.node(id).is_some_and(|n| n.is_memory())
}

/// The memory a claim repeats: the most alike one reaching `min`, judged without the `frame`
/// words (see `text::bare_similarity`).
fn find_similar(m: &Memory, scores: &[Score], toks: &[String], min: f32, frame: &[String]) -> Option<u32> {
    m.nodes
        .iter()
        // Nodes created after `scores` was computed have no score yet: skip them.
        .filter(|n| scores.get(n.id as usize).is_some_and(|s| s.visible))
        .filter(|n| n.is_memory())
        .filter(|n| matches!(n.kind, NodeKind::Fact | NodeKind::Event | NodeKind::Trait))
        .map(|n| (n.id, text::bare_similarity(toks, &text::tokens(&format!("{} {}", n.label, n.text)), frame)))
        .filter(|&(_, j)| j >= min)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(id, _)| id)
}

fn resolve_entities(m: &Memory, scores: &[Score], names: &[String]) -> Vec<u32> {
    let mut out = Vec::new();
    for name in names {
        let folded = text::fold(name);
        if let Some(n) = m.nodes.iter().find(|n| {
            scores.get(n.id as usize).is_some_and(|s| s.visible)
                && n.kind.is_entity()
                && std::iter::once(&n.label).chain(n.aliases.iter()).any(|a| {
                    text::contains_phrase(&folded, a) || text::contains_phrase(&text::fold(a), name)
                })
        }) {
            if !out.contains(&n.id) {
                out.push(n.id);
            }
        }
    }
    out
}

fn rejected_by_canon(m: &Memory, scores: &[Score], id: u32) -> bool {
    m.edges.iter().any(|e| {
        e.kind == EdgeKind::Contradicts
            && e.from == id
            && scores.get(e.to as usize).is_some_and(|s| s.visible)
            && m.nodes[e.to as usize].anchored()
    })
}

fn consolidate(m: &mut Memory, id: u32) -> bool {
    let n = &mut m.nodes[id as usize];
    if n.origin == Origin::Learned && !n.has(flags::CONSOLIDATED) && n.evidence() >= 3.0 && score::chance(n) >= 0.6 {
        n.flags |= flags::CONSOLIDATED;
        return true;
    }
    false
}

/// Applies the Z-axis rules for the claims extracted from one user message.
#[cfg_attr(not(test), allow(dead_code))]
pub fn apply_claims(m: &mut Memory, period: u8, episode: u32, claims: &[Claim]) -> Vec<LearnEvent> {
    apply_claims_with(m, period, episode, claims, 1.0)
}

/// Same, with `plasticity`: how much one piece of evidence weighs (dopamine gates it). Canon
/// rejections are not hormonal and keep their fixed weight.
pub fn apply_claims_with(m: &mut Memory, period: u8, episode: u32, claims: &[Claim], plasticity: f32) -> Vec<LearnEvent> {
    apply_claims_placed(m, period, episode, claims, plasticity, &[])
}

/// Same, and in a memory with sectors every node it creates is born with its belonging
/// (`sectors::home_of_fact`). `hints[i]` is what the analyst and the caller say about `claims[i]`
/// (a claim without a hint is decided by the entities and the sorting rule alone). A claim that
/// reinforces or weakens a memory that already exists creates nothing and moves nothing: the
/// memory keeps the sector it had. In a memory without sectors this is exactly `apply_claims_with`.
pub fn apply_claims_placed(m: &mut Memory, period: u8, episode: u32, claims: &[Claim], plasticity: f32, hints: &[sectors::Hint]) -> Vec<LearnEvent> {
    let w = plasticity;
    let now = m.horizon(period);
    let before = score::score_all(m, now);
    let mut touched: Vec<(u32, &'static str)> = Vec::new();

    for (ci, c) in claims.iter().enumerate() {
        let toks = text::tokens(&c.text);
        if toks.len() < 2 {
            continue;
        }
        // A possible future only exists once it happens in the story: then it is written now.
        if c.stance == Stance::Affirms {
            if let Some(id) = find_potential(m, &toks, 0.55) {
                let y = period_time(m, period);
                let n = &mut m.nodes[id as usize];
                n.flags &= !flags::POTENTIAL;
                n.y = y;
                n.period = period;
                n.activation = 1.0;
                m.upsert_edge(episode, id, EdgeKind::Mentions, 1.0, 1.0);
                touched.push((id, "se cumple"));
                continue;
            }
        }
        let frame: &[String] = hints.get(ci).map_or(&[], |h| h.frame.as_slice());
        let scores = score::score_all(m, m.horizon(period));
        // The extractor may say which memory the claim restates; otherwise match lexically.
        let restated = c.supports.filter(|&s| {
            scores.get(s as usize).is_some_and(|x| x.visible)
                && is_memory(m, s)
                && matches!(m.nodes[s as usize].kind, NodeKind::Fact | NodeKind::Event | NodeKind::Trait)
        });
        if let Some(id) = restated.or_else(|| find_similar(m, &scores, &toks, 0.5, frame)) {
            let anchored = m.nodes[id as usize].anchored();
            let rejected = rejected_by_canon(m, &scores, id);
            let n = &mut m.nodes[id as usize];
            n.activation = 1.0;
            match (c.stance, anchored) {
                (Stance::Affirms, true) => touched.push((id, "refuerza")),
                (Stance::Affirms, false) if rejected => {
                    n.beta += w;
                    touched.push((id, "rechazado"));
                }
                (Stance::Affirms, false) => {
                    n.alpha += w;
                    touched.push((id, "refuerza"));
                }
                (Stance::Denies, false) => {
                    n.beta += w;
                    touched.push((id, "debilita"));
                }
                (Stance::Denies, true) => {
                    // Canon is Y-locked: the denial itself becomes a memory that lives outside the system.
                    let x = m.nodes[id as usize].x;
                    let y = period_time(m, period);
                    let label = crate::lang::denial_label(crate::lang::Lang::of_memory(m), &m.nodes[id as usize].label);
                    let mut f = new_node(m, NodeKind::Fact, Origin::Learned, period, x, y, &label, &c.text);
                    f.beta += 2.0;
                    let f = m.add_node(f);
                    // The denial lives where the canon it denies lives.
                    if let Some(s) = sectors::home_like(m, id) {
                        sectors::attach(m, f, s);
                    }
                    m.upsert_edge(f, id, EdgeKind::Contradicts, 1.0, 1.0);
                    m.upsert_edge(episode, f, EdgeKind::Mentions, 1.0, 1.0);
                    touched.push((f, "rechazado"));
                }
            }
            continue;
        }

        let ents = resolve_entities(m, &scores, &c.entities);
        let x = place_x(m, &ents, period, &c.text);
        let y = period_time(m, period);
        let mut f = new_node(m, NodeKind::Fact, Origin::Learned, period, x, y, &c.text, &c.text);
        f.alpha += w;
        let fid = m.add_node(f);
        // Born with its sector (only in a memory that has them): the entities it mentions, the
        // sorting rule, the analyst's proposal, or the inbox. The cause below lives with it.
        let hint = hints.get(ci);
        let skip = hint.map(|h| resolve_entities(m, &scores, &h.speaker)).unwrap_or_default();
        let home = sectors::home_of_fact(m, &ents, &skip, &c.text, hint).map(|(s, _)| s);
        if let Some(s) = home {
            sectors::attach(m, fid, s);
        }
        m.upsert_edge(episode, fid, EdgeKind::Mentions, 1.0, 1.0);
        for &e in &ents {
            m.upsert_edge(fid, e, EdgeKind::Mentions, 1.0, 1.0);
        }
        let mut action = "nuevo";

        if let Some(s) = c.supports.filter(|&s| scores.get(s as usize).is_some_and(|x| x.visible) && is_memory(m, s)) {
            m.upsert_edge(s, fid, EdgeKind::Causes, 3.0, 1.0);
        }
        if let Some(t) = c.contradicts.filter(|&t| scores.get(t as usize).is_some_and(|x| x.visible) && is_memory(m, t)) {
            m.upsert_edge(fid, t, EdgeKind::Contradicts, 1.0, 1.0);
            if m.nodes[t as usize].anchored() {
                m.nodes[fid as usize].beta += 2.0;
                action = "rechazado";
            } else {
                m.nodes[t as usize].beta += w;
                touched.push((t, "debilita"));
            }
        }
        if let Some(old) = c.supersedes.filter(|&o| scores.get(o as usize).is_some_and(|x| x.visible) && is_memory(m, o)) {
            if supersede(m, fid, old).is_ok() {
                touched.push((old, "sustituido"));
            }
        }
        if let Some(cause) = c.cause.as_deref().filter(|s| text::tokens(s).len() >= 2) {
            let ctoks = text::tokens(cause);
            let cid = match find_similar(m, &scores, &ctoks, 0.35, frame) {
                Some(id) => id,
                None => {
                    let y = period_time(m, period);
                    let mut cn = new_node(m, NodeKind::Fact, Origin::Learned, period, x, y - TURN_STEP * 0.5, cause, cause);
                    cn.alpha += w;
                    let id = m.add_node(cn);
                    if let Some(s) = home {
                        sectors::attach(m, id, s);
                    }
                    m.upsert_edge(episode, id, EdgeKind::Mentions, 1.0, 1.0);
                    touched.push((id, "nuevo"));
                    id
                }
            };
            let i = m.upsert_edge(cid, fid, EdgeKind::Causes, 1.0, 1.0);
            m.edges[i].alpha += w;
            touched.push((cid, "causa"));
        }
        touched.push((fid, action));
    }

    let mut events = Vec::new();
    let after = score::score_all(m, m.horizon(period));
    for (id, action) in touched {
        let action = if consolidate(m, id) { "consolidado" } else { action };
        let n = &m.nodes[id as usize];
        events.push(LearnEvent {
            node: id,
            label: n.label.clone(),
            action,
            chi_before: before.get(id as usize).map(|s| s.chi).unwrap_or(0.0),
            chi_after: after[id as usize].chi,
        });
    }
    events
}

/// Adds a `Supersedes` edge `new` -> `old`.
///
/// Fails if:
/// - either id does not exist, or both are the same;
/// - the old node is ANCHORED (cannot be overwritten by conversation);
/// - the new or old node is POTENTIAL (future plan);
/// - a cycle would be created across the supersession chain.
pub fn supersede(m: &mut Memory, new: u32, old: u32) -> Result<usize, String> {
    if new == old {
        return Err(format!("#{new} y #{old} son el mismo nodo: un nodo no puede sustituirse a sí mismo"));
    }
    if m.node(new).is_none() {
        return Err(format!("no existe el nodo #{new}"));
    }
    if m.node(old).is_none() {
        return Err(format!("no existe el nodo #{old}"));
    }
    if m.nodes[old as usize].anchored() {
        return Err(format!("#{old} está anclado: solo canon puede modificarlo"));
    }
    if m.nodes[new as usize].has(flags::POTENTIAL) {
        return Err(format!("#{new} es un plan: no es visible"));
    }
    if m.nodes[old as usize].has(flags::POTENTIAL) {
        return Err(format!("#{old} es un plan: no es visible"));
    }
    // Reject any cycle: if new is reachable from old following Supersedes edges
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(old);
    visited.insert(old);
    while let Some(curr) = queue.pop_front() {
        for e in &m.edges {
            if e.from == curr && e.kind == EdgeKind::Supersedes {
                if e.to == new {
                    return Err(format!("ya existe una cadena de sustitución de #{old} a #{new}: crearía un ciclo"));
                }
                if visited.insert(e.to) {
                    queue.push_back(e.to);
                }
            }
        }
    }
    let idx = m.upsert_edge(new, old, EdgeKind::Supersedes, 1.0, 1.0);
    Ok(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Memory {
        let mut m = Memory::default();
        m.periods.push(Period {
            id: 0,
            key: "p".into(),
            name: "p".into(),
            source: String::new(),
            start: 1082.0,
            end: 1093.0,
            now: 1092.9,
            home_x: 0.0,
            persona: String::new(),
            scene: String::new(),
            opening: String::new(),
        });
        let mut texas = new_node(&m, NodeKind::Person, Origin::Canon, 255, -0.8, 1082.0, "Texas", "Cellinia Texas");
        texas.flags = flags::ANCHORED;
        m.add_node(texas);
        let mut duel = new_node(&m, NodeKind::Event, Origin::Canon, 255, -0.8, 1092.3, "Duelo en la finca en llamas", "Texas hiere a Lappland en el duelo de la finca en llamas");
        duel.flags = flags::ANCHORED;
        m.add_node(duel);
        m
    }

    #[test]
    fn time_moves_forward_and_learning_needs_evidence() {
        let mut m = mem();
        let e1 = record_turn(&mut m, 0, "Hola Texas", "…", &[]);
        let e2 = record_turn(&mut m, 0, "Otra vez", "…", &[]);
        assert!(m.nodes[e2 as usize].y > m.nodes[e1 as usize].y);
        assert!(m.nodes[e1 as usize].y > 1092.9);

        let claim = Claim {
            text: "Lappland guarda una uva seca en el bolsillo".into(),
            entities: vec![],
            stance: Stance::Affirms,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: None,
        };
        let ev = apply_claims(&mut m, 0, e2, &[claim.clone()]);
        let fact = ev.iter().find(|e| e.action == "nuevo").unwrap().node;
        let chi0 = score::score_all(&m, m.horizon(0))[fact as usize].chi;
        assert!(score::score_all(&m, m.horizon(0))[fact as usize].visible, "chat writes are in her present");
        for _ in 0..4 {
            apply_claims(&mut m, 0, e2, &[claim.clone()]);
        }
        let chi1 = score::score_all(&m, m.horizon(0))[fact as usize].chi;
        assert!(chi1 > chi0);
        assert!(m.nodes[fact as usize].has(flags::CONSOLIDATED));
    }

    #[test]
    fn unattended_memories_fade_faster_the_less_she_understands_them() {
        let mut m = mem();
        let ep = record_turn(&mut m, 0, "eso", "…", &[]);
        let claim = Claim {
            text: "Lappland guarda una uva seca en el bolsillo".into(),
            entities: vec![],
            stance: Stance::Affirms,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: None,
        };
        let fact = apply_claims(&mut m, 0, ep, &[claim])[0].node;
        let chi = score::score_all(&m, m.horizon(0))[fact as usize].chi;
        assert!((chi - 0.08).abs() < 0.01, "a new fact is born barely understood: {chi}");
        m.nodes[1].activation = 1.0;
        m.nodes[fact as usize].activation = 1.0;
        fade(&mut m, 0, 600.0);
        let weak = m.nodes[fact as usize].activation;
        let canon = m.nodes[1].activation;
        assert!(weak < 0.3 && canon > 0.55, "weak {weak} canon {canon}");
    }

    #[test]
    fn engaged_she_is_surer_of_what_she_hears() {
        let claim = Claim {
            text: "Lappland guarda una uva seca en el bolsillo".into(),
            entities: vec![],
            stance: Stance::Affirms,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: None,
        };
        let chi = |w: f32| {
            let mut m = mem();
            let ep = record_turn(&mut m, 0, "eso", "…", &[]);
            let f = apply_claims_with(&mut m, 0, ep, &[claim.clone()], w)[0].node;
            score::score_all(&m, m.horizon(0))[f as usize].chi
        };
        assert!(chi(1.5) > chi(1.0) && chi(1.0) > chi(0.6), "{} {} {}", chi(1.5), chi(1.0), chi(0.6));
    }

    #[test]
    fn claim_with_new_cause_builds_causal_edge() {
        let mut m = mem();
        let ep = record_turn(&mut m, 0, "…", "…", &[]);
        let c = Claim {
            text: "Lappland odia la lluvia de Brunello".into(),
            entities: vec!["Texas".into()],
            stance: Stance::Affirms,
            cause: Some("la lluvia apagó su primer incendio".into()),
            supports: None,
            contradicts: None,
            supersedes: None,
        };
        let ev = apply_claims(&mut m, 0, ep, &[c]);
        let fact = ev.iter().find(|e| e.action == "nuevo" && e.label.contains("odia")).unwrap().node;
        assert!(m.edges.iter().any(|e| e.kind == EdgeKind::Causes && e.to == fact));
        let s = score::score_all(&m, m.horizon(0));
        assert!(s.iter().all(|x| x.visible || x.chi == -1.0));
    }

    #[test]
    fn denying_canon_creates_out_of_system_memory() {
        let mut m = mem();
        let ep = record_turn(&mut m, 0, "eso no pasó", "…", &[]);
        let deny = Claim {
            text: "El duelo en la finca en llamas nunca ocurrió".into(),
            entities: vec![],
            stance: Stance::Denies,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: None,
        };
        let ev = apply_claims(&mut m, 0, ep, &[deny]);
        assert_eq!(ev[0].action, "rechazado");
        let s = score::score_all(&m, m.horizon(0));
        assert!(s[ev[0].node as usize].visible);
        assert!(s[ev[0].node as usize].chi < -0.5);
        assert!(s[1].chi > 0.9, "canon untouched");
    }

    #[test]
    fn supersede_sinks_the_old_rule() {
        let mut m = mem();
        let ep = record_turn(&mut m, 0, "eso", ".", &[]);
        let c1 = Claim {
            text: "Los mensajes se leen en texto plano".into(),
            entities: vec![],
            stance: Stance::Affirms,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: None,
        };
        let ev1 = apply_claims(&mut m, 0, ep, &[c1]);
        let old = ev1[0].node;
        assert!(score::score_all(&m, m.horizon(0))[old as usize].chi > 0.0);

        let c2 = Claim {
            text: "Los mensajes se procesan en bloques estructurados".into(),
            entities: vec![],
            stance: Stance::Affirms,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: Some(old),
        };
        let ev2 = apply_claims(&mut m, 0, ep, &[c2]);
        let new = ev2.iter().find(|e| e.action == "nuevo").unwrap().node;
        let s = score::score_all(&m, m.horizon(0));
        assert!(s[old as usize].superseded, "old node must be marked superseded");
        assert!(s[old as usize].chi < 0.0, "old chi should sink: {}", s[old as usize].chi);
        assert!(s[new as usize].chi > 0.0, "new node should have positive chi");
    }

    #[test]
    fn supersede_rejects_invalid_cases() {
        let mut m = mem();
        let ep = record_turn(&mut m, 0, "test", ".", &[]);
        let r1 = apply_claims(&mut m, 0, ep, &[Claim {
            text: "Primera regla sobre manzanas frescas".into(),
            entities: vec![],
            stance: Stance::Affirms,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: None,
        }])[0].node;
        let r2 = apply_claims(&mut m, 0, ep, &[Claim {
            text: "Segunda regla sobre naranjas maduras".into(),
            entities: vec![],
            stance: Stance::Affirms,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: None,
        }])[0].node;

        // 1. Same ID
        assert!(supersede(&mut m, r1, r1).unwrap_err().contains("mismo nodo"));

        // 2. Non-existent ID
        assert!(supersede(&mut m, 999, r1).unwrap_err().contains("no existe"));
        assert!(supersede(&mut m, r1, 999).unwrap_err().contains("no existe"));

        // 3. Old is ANCHORED
        assert!(supersede(&mut m, r1, 1).unwrap_err().contains("anclado"));

        // 4. Cycle rejection: r1 supersedes r2, then r2 supersedes r1 fails
        supersede(&mut m, r1, r2).expect("valid supersede");
        assert!(supersede(&mut m, r2, r1).unwrap_err().contains("ciclo"));

        // 5. Cycle of 3: r2 -> r3, then r3 -> r1 fails
        let r3 = apply_claims(&mut m, 0, ep, &[Claim {
            text: "Tercera regla".into(),
            entities: vec![],
            stance: Stance::Affirms,
            cause: None,
            supports: None,
            contradicts: None,
            supersedes: None,
        }])[0].node;
        supersede(&mut m, r2, r3).expect("valid supersede 2 -> 3");
        assert!(supersede(&mut m, r3, r1).unwrap_err().contains("ciclo"));
    }
}
