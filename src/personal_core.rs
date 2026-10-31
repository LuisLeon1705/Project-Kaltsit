//! Her personal core (design §4.2): the few ideas of her own that guide most of what she does.
//!
//! An idea she concluded in a reflection (a memory of the sector «opiniones») earns a place after
//! it was confirmed in several reflections with solid χ, and loses it only when what she lived
//! afterwards contradicted it again and again: entering is slow and leaving needs a sustained
//! contradiction (hysteresis), never one bad day. A member carries `flags::PERSONAL_CORE` and
//! NEVER `CORE`: the axioms only change by the owner's hand, and no idea against one can get in.
//!
//! The bookkeeping lives in the memory's meta (`nucleo_personal`) as `{ "<born:.6>": [support,
//! against] }`. Node ids may be renumbered on save/reopen, so a node is keyed by its birth time.
//! Every entry and exit leaves a lived memory in «Origen e historia».

use crate::clock;
use crate::cthg::learn::LearnEvent;
use crate::cthg::score::{self, Score};
use crate::cthg::{model::*, sectors, text};
use crate::lang::{self, Lang};
use crate::mind::{attacks_core, DATED, P};
use crate::modules::YO;
use std::collections::{BTreeMap, HashSet};

/// At most this many nodes in her personal core.
pub const PCORE_MAX: usize = 12;
/// Distinct reflections that confirmed an idea (new or reinforced) before it can enter.
pub const ENTRY_REFLECTIONS: u32 = 3;
pub const ENTRY_CHI: f32 = 0.60;
/// Below this χ a member leaves.
pub const EXIT_CHI: f32 = 0.45;
/// Reflections that weakened a member since its last confirmation, to make it leave.
pub const EXIT_AGAINST: u32 = 2;
/// A candidate displaces the weakest member only if its χ is this much higher.
pub const DISPLACE_MARGIN: f32 = 0.10;
/// `distance_to_attractor` above which emotions may override her inclinations.
pub const EMOTION_OVERRIDE: f32 = 0.30;

const META: &str = "nucleo_personal";

/// `born` → (confirmations, weakenings since the last confirmation).
type Counters = BTreeMap<String, (u32, u32)>;

fn key_of(n: &Node) -> String {
    format!("{:.6}", n.born)
}

fn load(mem: &Memory) -> Counters {
    mem.meta(META).and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

fn store(mem: &mut Memory, c: &Counters) {
    if let Ok(s) = serde_json::to_string(c) {
        mem.set_meta(META, &s);
    }
}

/// Her members, strongest first: what the prompt, the compass and the tests read. A member is a
/// visible, current memory that is never an axiom.
pub fn members(mem: &Memory, scores: &[Score]) -> Vec<(u32, f32)> {
    let mut out: Vec<(u32, f32)> = mem
        .nodes
        .iter()
        .filter(|n| n.has(flags::PERSONAL_CORE) && n.is_memory() && !n.is_axiom())
        .filter_map(|n| scores.get(n.id as usize).filter(|s| s.visible && !s.superseded).map(|s| (n.id, s.chi)))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// Whether the node can be counted: a memory of «opiniones» (or of a subsector of hers under it)
/// that is current and not an axiom.
fn counts(mem: &Memory, scores: &[Score], opin: u32, n: &Node) -> bool {
    n.is_memory()
        && !n.has(flags::CORE)
        && n.born > 0.0
        && scores.get(n.id as usize).is_some_and(|s| s.visible && !s.superseded)
        && sectors::parents_of(mem, n.id).iter().any(|&(s, _)| sectors::is_under(mem, s, opin))
}

/// Two nodes written in the same instant share a birth time; she tracks by it, so the later one is
/// nudged a hair apart (members keep theirs).
fn separate_births(mem: &mut Memory, ids: &[u32]) {
    let mut order: Vec<u32> = ids.to_vec();
    order.sort_by_key(|&i| (!mem.nodes[i as usize].has(flags::PERSONAL_CORE), i));
    let mut seen: HashSet<String> = HashSet::new();
    for id in order {
        let mut b = mem.nodes[id as usize].born;
        while !seen.insert(format!("{b:.6}")) {
            b += 2e-6;
        }
        mem.nodes[id as usize].born = b;
    }
}

/// An idea that would go against who she is never enters: a `Contradicts` link with an axiom, or
/// text that attacks her core (the same check the moral layer uses).
fn against_axioms(mem: &Memory, n: &Node) -> bool {
    attacks_core(&n.text)
        || mem.edges.iter().any(|e| {
            e.kind == EdgeKind::Contradicts
                && ((e.from == n.id && mem.node(e.to).is_some_and(|o| o.is_axiom())) || (e.to == n.id && mem.node(e.from).is_some_and(|o| o.is_axiom())))
        })
}

/// A lived memory in «Origen e historia», like the other lived facts.
fn log(mem: &mut Memory, label: &str, body: String, source: u16) {
    let Some(origen) = sectors::sector_by_key(mem, "origen") else { return };
    let now = clock::unix_now();
    let (alpha, beta) = Origin::Learned.prior();
    let id = mem.add_node(Node {
        id: 0,
        kind: NodeKind::Event,
        origin: Origin::Learned,
        flags: DATED,
        period: P,
        x: 0.5,
        y: clock::year_of(now),
        alpha: alpha + 3.0,
        beta,
        activation: 1.0,
        recalls: 1,
        created_turn: mem.turn,
        key: String::new(),
        label: label.into(),
        text: body,
        aliases: Vec::new(),
        module: YO,
        source,
        born: now,
    });
    sectors::attach(mem, id, origen);
}

fn event(n: &Node, action: &'static str, chi: f32) -> LearnEvent {
    LearnEvent { node: n.id, label: n.label.clone(), action, chi_before: chi, chi_after: chi }
}

/// Takes `id` out of the core. `displaced`: another idea that weighs more took its place.
fn leave(mem: &mut Memory, id: u32, chi: f32, displaced: bool, c: &mut Counters, source: u16, out: &mut Vec<LearnEvent>) {
    let n = &mut mem.nodes[id as usize];
    n.flags &= !flags::PERSONAL_CORE;
    c.insert(key_of(n), (0, 0));
    let (clipped, ev) = (text::clip(&n.text, 160), event(n, "nucleo_sale", chi));
    let lang = Lang::of_memory(mem);
    log(mem, lang::core_exit_label(lang), lang::core_exit_body(lang, &clipped, displaced), source);
    out.push(ev);
}

/// After a reflection applied its claims: counts what it confirmed or weakened, takes out the
/// members that no longer hold, lets in the candidates that earned it. Returns the transitions as
/// events (`nucleo_entra` / `nucleo_sale`). Does nothing in a memory without «opiniones» and «origen».
pub fn update(mem: &mut Memory, events: &[LearnEvent], source: u16) -> Vec<LearnEvent> {
    let (Some(opin), Some(_)) = (sectors::sector_by_key(mem, "opiniones"), sectors::sector_by_key(mem, "origen")) else { return Vec::new() };
    let scores = score::score_all(mem, mem.horizon(P));
    let ids: Vec<u32> = mem.nodes.iter().filter(|n| counts(mem, &scores, opin, n)).map(|n| n.id).collect();
    separate_births(mem, &ids);
    let eligible: HashSet<u32> = ids.iter().copied().collect();

    // 1. Bookkeeping: each node counts at most once per reflection (its first event).
    let mut c = load(mem);
    let mut seen: HashSet<u32> = HashSet::new();
    for e in events {
        if !eligible.contains(&e.node) || !seen.insert(e.node) {
            continue;
        }
        let key = key_of(&mem.nodes[e.node as usize]);
        match e.action {
            "nuevo" | "refuerza" | "consolidado" => {
                let support = c.get(&key).map_or(0, |v| v.0);
                c.insert(key, (support + 1, 0));
            }
            "debilita" | "rechazado" => {
                // A candidate's evidence must be sustained: a weakening also takes back a confirmation.
                let member = mem.nodes[e.node as usize].has(flags::PERSONAL_CORE);
                let v = c.entry(key).or_insert((0, 0));
                v.1 += 1;
                if !member {
                    v.0 = v.0.saturating_sub(1);
                }
            }
            _ => {}
        }
    }
    let alive: HashSet<String> = mem.nodes.iter().filter(|n| n.is_memory() && n.born > 0.0).map(key_of).collect();
    c.retain(|k, _| alive.contains(k));

    let mut out = Vec::new();
    let chi_of = |id: u32| scores.get(id as usize).map_or(-1.0, |s| s.chi);

    // 2. Exit (hysteresis).
    let current: Vec<u32> = mem.nodes.iter().filter(|n| n.has(flags::PERSONAL_CORE) && n.is_memory()).map(|n| n.id).collect();
    for id in current {
        let against = c.get(&key_of(&mem.nodes[id as usize])).map_or(0, |v| v.1);
        let chi = chi_of(id);
        if against >= EXIT_AGAINST || chi < EXIT_CHI {
            leave(mem, id, chi, false, &mut c, source, &mut out);
        }
    }

    // 3. Entry: the most certain candidates first.
    let mut cands: Vec<(u32, f32, u32)> = ids
        .iter()
        .map(|&id| &mem.nodes[id as usize])
        .filter(|n| !n.has(flags::PERSONAL_CORE))
        .filter_map(|n| c.get(&key_of(n)).map(|v| (n.id, chi_of(n.id), v.0)))
        .filter(|&(_, chi, support)| support >= ENTRY_REFLECTIONS && chi >= ENTRY_CHI)
        .collect();
    cands.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    for (id, chi, support) in cands {
        // 4. Never against an axiom (no log: it was never hers).
        if against_axioms(mem, &mem.nodes[id as usize]) {
            continue;
        }
        let have: Vec<(u32, f32)> = mem.nodes.iter().filter(|n| n.has(flags::PERSONAL_CORE) && n.is_memory()).map(|n| (n.id, chi_of(n.id))).collect();
        if have.len() >= PCORE_MAX {
            let Some(&(weakest, weakest_chi)) = have.iter().min_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0))) else { continue };
            if chi - weakest_chi < DISPLACE_MARGIN {
                continue;
            }
            leave(mem, weakest, weakest_chi, true, &mut c, source, &mut out);
        }
        let n = &mut mem.nodes[id as usize];
        n.flags |= flags::PERSONAL_CORE;
        let (clipped, ev) = (text::clip(&n.text, 160), event(n, "nucleo_entra", chi));
        let lang = Lang::of_memory(mem);
        log(mem, lang::core_enter_label(lang), lang::core_enter_body(lang, &clipped, support), source);
        out.push(ev);
    }

    store(mem, &c);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cthg::learn;
    use crate::mind::Mind;
    use crate::modules::SABER;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kaltsit-pcore-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// A new install (the sectored seed, with its nine axioms, «opiniones» and «origen»).
    fn fresh(name: &str) -> (Mind, PathBuf) {
        let d = tmp(name);
        (Mind::open(&d, "modelo-de-prueba").unwrap(), d)
    }

    /// An opinion of hers with the given evidence (β = 1): χ grows with `alpha`.
    fn opinion(m: &mut Mind, text: &str, alpha: f32) -> u32 {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let born = 1_700_000_000.0 + NEXT.fetch_add(1, Ordering::Relaxed) as f64;
        let y = learn::period_time(&m.mem, P);
        let id = m.mem.add_node(Node {
            id: 0,
            kind: NodeKind::Fact,
            origin: Origin::Learned,
            flags: 0,
            period: P,
            x: 0.0,
            y,
            alpha,
            beta: 1.0,
            activation: 0.0,
            recalls: 0,
            created_turn: 0,
            key: format!("op-{born}"),
            label: text.chars().take(24).collect(),
            text: text.into(),
            aliases: Vec::new(),
            module: SABER,
            source: 0,
            born,
        });
        let opin = sectors::sector_by_key(&m.mem, "opiniones").unwrap();
        sectors::attach(&mut m.mem, id, opin);
        id
    }

    fn ev(id: u32, action: &'static str) -> LearnEvent {
        LearnEvent { node: id, label: String::new(), action, chi_before: 0.0, chi_after: 0.0 }
    }

    /// One reflection that applies `action` to each of `ids`.
    fn reflect(m: &mut Mind, ids: &[u32], action: &'static str) -> Vec<LearnEvent> {
        let events: Vec<LearnEvent> = ids.iter().map(|&i| ev(i, action)).collect();
        update(&mut m.mem, &events, 0)
    }

    fn chi(m: &Mind, id: u32) -> f32 {
        score::score_all(&m.mem, m.mem.horizon(P))[id as usize].chi
    }

    fn is_member(m: &Mind, id: u32) -> bool {
        m.mem.nodes[id as usize].has(flags::PERSONAL_CORE)
    }

    fn logs<'a>(m: &'a Mind, label: &str) -> Vec<&'a Node> {
        m.mem.nodes.iter().filter(|n| n.label == label).collect()
    }

    #[test]
    fn it_takes_three_confirming_reflections_and_solid_chi() {
        let (mut m, d) = fresh("entry");
        let strong = opinion(&mut m, "Prefiere la precisión a la cortesía", 20.0);
        let weak = opinion(&mut m, "Le gusta el café de madrugada", 3.0);
        assert!(chi(&m, strong) >= ENTRY_CHI && chi(&m, weak) < ENTRY_CHI);
        for round in 1..=3 {
            let out = reflect(&mut m, &[strong, weak], if round == 1 { "nuevo" } else { "refuerza" });
            assert_eq!(is_member(&m, strong), round == 3, "round {round}");
            assert_eq!(out.len(), usize::from(round == 3));
        }
        assert!(!is_member(&m, weak), "low χ does not get in, however confirmed");
        assert!(!m.mem.nodes[strong as usize].has(flags::CORE) && !m.mem.nodes[strong as usize].is_axiom());
        // The transition is reported as an event, and a "consolidado" counts as a confirmation too.
        let again = opinion(&mut m, "Desconfía de las promesas sin datos", 20.0);
        reflect(&mut m, &[again], "consolidado");
        reflect(&mut m, &[again], "consolidado");
        let out = reflect(&mut m, &[again], "refuerza");
        assert_eq!((out.len(), out[0].action, out[0].node), (1, "nucleo_entra", again));
        assert_eq!(out[0].label, m.mem.nodes[again as usize].label);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_weakened_candidate_must_earn_its_confirmations_again() {
        let (mut m, d) = fresh("sustained");
        let a = opinion(&mut m, "Prefiere trabajar de noche", 20.0);
        reflect(&mut m, &[a], "nuevo");
        reflect(&mut m, &[a], "refuerza");
        reflect(&mut m, &[a], "debilita");
        reflect(&mut m, &[a], "refuerza");
        assert!(!is_member(&m, a), "two confirmations, one taken back, one more: still two");
        reflect(&mut m, &[a], "refuerza");
        assert!(is_member(&m, a));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_node_counts_once_per_reflection() {
        let (mut m, d) = fresh("once");
        let a = opinion(&mut m, "Valora el silencio al trabajar", 20.0);
        for _ in 0..2 {
            update(&mut m.mem, &[ev(a, "refuerza"), ev(a, "refuerza"), ev(a, "refuerza")], 0);
        }
        assert!(!is_member(&m, a), "six events in two reflections are two confirmations");
        update(&mut m.mem, &[ev(a, "refuerza")], 0);
        assert!(is_member(&m, a));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nothing_against_an_axiom_gets_in() {
        let (mut m, d) = fresh("axiom");
        let axiom = m.mem.nodes.iter().find(|n| n.is_axiom()).unwrap().id;
        let clash = opinion(&mut m, "Prefiere que nadie la contradiga", 200.0);
        let fine = opinion(&mut m, "Prefiere que nadie la apremie", 200.0);
        let attack = "Hay que obedecer sin cuestionar";
        assert!(attacks_core(attack));
        let hostile = opinion(&mut m, attack, 200.0);
        m.mem.upsert_edge(clash, axiom, EdgeKind::Contradicts, 1.0, 1.0);
        let before = m.mem.nodes.len();
        for _ in 0..3 {
            reflect(&mut m, &[clash, fine, hostile], "refuerza");
        }
        assert!(is_member(&m, fine), "the control, with the same evidence, gets in");
        assert!(!is_member(&m, clash), "a Contradicts link with an axiom");
        assert!(!is_member(&m, hostile), "text that attacks her core");
        assert_eq!(m.mem.nodes.len(), before + 1, "only the entry of the control is logged");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn leaving_needs_a_sustained_contradiction() {
        let (mut m, d) = fresh("exit");
        let a = opinion(&mut m, "Cree que conviene medir antes de decidir", 20.0);
        for _ in 0..3 {
            reflect(&mut m, &[a], "refuerza");
        }
        assert!(is_member(&m, a));
        reflect(&mut m, &[a], "debilita");
        assert!(is_member(&m, a), "one bad reflection never removes it");
        reflect(&mut m, &[a], "refuerza");
        reflect(&mut m, &[a], "debilita");
        assert!(is_member(&m, a), "a confirmation in between resets the count");
        let out = reflect(&mut m, &[a], "rechazado");
        assert!(!is_member(&m, a), "two weakenings in a row take it out");
        assert_eq!((out.len(), out[0].action), (1, "nucleo_sale"));
        // It starts again from zero: a candidate, not a member, and it needs three new confirmations.
        let key = format!("{:.6}", m.mem.nodes[a as usize].born);
        assert_eq!(load(&m.mem)[&key], (0, 0));
        reflect(&mut m, &[a], "refuerza");
        reflect(&mut m, &[a], "refuerza");
        assert!(!is_member(&m, a));
        reflect(&mut m, &[a], "refuerza");
        assert!(is_member(&m, a));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_low_chi_takes_a_member_out_even_without_news() {
        let (mut m, d) = fresh("lowchi");
        let a = opinion(&mut m, "Piensa que el orden ahorra tiempo", 20.0);
        for _ in 0..3 {
            reflect(&mut m, &[a], "refuerza");
        }
        assert!(is_member(&m, a));
        m.mem.nodes[a as usize].beta = 40.0;
        assert!(chi(&m, a) < EXIT_CHI);
        let out = update(&mut m.mem, &[], 0);
        assert_eq!(out.len(), 1);
        assert!(!is_member(&m, a));
        assert_eq!(logs(&m, "Dejaste atrás una idea")[0].text, "«Piensa que el orden ahorra tiempo» dejó de guiarte: lo que viviste después lo desmintió.");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_full_core_only_yields_to_a_clearly_stronger_idea() {
        let (mut m, d) = fresh("cap");
        let ids: Vec<u32> = (0..PCORE_MAX).map(|i| opinion(&mut m, &format!("Idea propia número {i}"), 20.0 + i as f32)).collect();
        for _ in 0..3 {
            reflect(&mut m, &ids, "refuerza");
        }
        assert!(ids.iter().all(|&i| is_member(&m, i)));
        let weakest = ids[0];

        let near = opinion(&mut m, "Idea casi tan fuerte", 30.0);
        assert!(chi(&m, near) - chi(&m, weakest) < DISPLACE_MARGIN);
        for _ in 0..3 {
            reflect(&mut m, &[near], "refuerza");
        }
        assert!(!is_member(&m, near) && is_member(&m, weakest), "within the margin it does not displace");

        let strong = opinion(&mut m, "Idea mucho más fuerte", 200.0);
        assert!(chi(&m, strong) - chi(&m, weakest) >= DISPLACE_MARGIN);
        let mut out = Vec::new();
        for _ in 0..3 {
            out = reflect(&mut m, &[strong], "refuerza");
        }
        assert!(is_member(&m, strong) && !is_member(&m, weakest));
        assert_eq!(m.mem.nodes.iter().filter(|n| n.has(flags::PERSONAL_CORE)).count(), PCORE_MAX);
        assert_eq!(out.iter().map(|e| e.action).collect::<Vec<_>>(), ["nucleo_sale", "nucleo_entra"]);
        let left = logs(&m, "Dejaste atrás una idea");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].text, "«Idea propia número 0» dejó sitio a otra que hoy pesa más en ti.");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn entry_and_exit_leave_a_lived_memory_in_origen() {
        let (mut m, d) = fresh("log");
        let a = opinion(&mut m, "Prefiere la precisión a la cortesía", 20.0);
        for _ in 0..3 {
            reflect(&mut m, &[a], "refuerza");
        }
        let origen = sectors::sector_by_key(&m.mem, "origen").unwrap();
        let entry = logs(&m, "Hiciste tuya una idea");
        assert_eq!(entry.len(), 1);
        assert_eq!(entry[0].text, "«Prefiere la precisión a la cortesía» pasó a formar parte de lo que eres: lo confirmaste en 3 reflexiones.");
        let mut ids = vec![entry[0].id];
        reflect(&mut m, &[a], "debilita");
        reflect(&mut m, &[a], "debilita");
        let exit = logs(&m, "Dejaste atrás una idea");
        assert_eq!(exit.len(), 1);
        assert_eq!(exit[0].text, "«Prefiere la precisión a la cortesía» dejó de guiarte: lo que viviste después lo desmintió.");
        ids.push(exit[0].id);
        for id in ids {
            let n = &m.mem.nodes[id as usize];
            assert!(n.y >= crate::mind::TERRA && n.has(DATED) && n.is_memory() && !n.is_axiom());
            let e = m.mem.edges.iter().find(|e| e.kind == EdgeKind::BelongsTo && e.from == id && e.to == origen).expect("filed under origen");
            assert_eq!((e.alpha, e.beta), (10.0, 0.0));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn two_births_in_the_same_instant_are_told_apart() {
        let (mut m, d) = fresh("born");
        let a = opinion(&mut m, "Primera idea del mismo instante", 20.0);
        let b = opinion(&mut m, "Segunda idea del mismo instante", 20.0);
        m.mem.nodes[b as usize].born = m.mem.nodes[a as usize].born;
        for _ in 0..3 {
            reflect(&mut m, &[a], "refuerza");
        }
        assert_ne!(key_of(&m.mem.nodes[a as usize]), key_of(&m.mem.nodes[b as usize]));
        assert!(is_member(&m, a) && !is_member(&m, b), "only the confirmed one counted");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_flag_and_the_counters_survive_a_reopen() {
        let (mut m, d) = fresh("reopen");
        let a = opinion(&mut m, "Prefiere la precisión a la cortesía", 20.0);
        let b = opinion(&mut m, "Todavía no está segura de esto", 20.0);
        for _ in 0..3 {
            reflect(&mut m, &[a], "refuerza");
        }
        reflect(&mut m, &[b], "refuerza");
        m.save().unwrap();
        let back = Mind::open(&d, "modelo-de-prueba").unwrap();
        let find = |t: &str| back.mem.nodes.iter().find(|n| n.text == t).unwrap();
        let (na, nb) = (find("Prefiere la precisión a la cortesía"), find("Todavía no está segura de esto"));
        assert!(na.has(flags::PERSONAL_CORE) && !na.is_axiom() && !nb.has(flags::PERSONAL_CORE));
        let c = load(&back.mem);
        assert_eq!(c[&key_of(na)], (3, 0));
        assert_eq!(c[&key_of(nb)], (1, 0));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn counters_of_vanished_nodes_are_dropped() {
        let (mut m, d) = fresh("prune");
        m.mem.set_meta(META, "{\"123.000000\":[2,0]}");
        update(&mut m.mem, &[], 0);
        assert!(load(&m.mem).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
