//! Recall: lexical match + spreading activation through the hypergraph, gated by period and χ.

use super::model::*;
use super::score::Score;
use super::text;

/// Below this relevance nothing comes back.
pub const MIN_RELEVANCE: f32 = 0.05;
/// Nor below this fraction of the best match of the same query.

#[derive(Clone, Debug)]
pub struct Recall {
    pub id: u32,
    pub relevance: f32,
}

/// Entity nodes literally named in `s` (label or alias), visible in this view.
pub fn mentioned_entities(m: &Memory, scores: &[Score], s: &str) -> Vec<u32> {
    let folded = text::fold(s);
    m.nodes
        .iter()
        .filter(|n| scores[n.id as usize].visible && !scores[n.id as usize].superseded && (n.kind.is_entity() || n.kind == NodeKind::Trait))
        .filter(|n| {
            std::iter::once(&n.label)
                .chain(n.aliases.iter())
                .any(|a| text::contains_phrase(&folded, a))
        })
        .map(|n| n.id)
        .collect()
}

/// First stage of recall, pure CPU: how strongly the text names each node (2 for a literal
/// label or alias, plus word overlap). Used live on the half-typed draft, without the model.
pub fn lexical_seeds(m: &Memory, scores: &[Score], query: &str, min_chi: f32) -> Vec<f32> {
    let q = text::tokens(query);
    let folded = text::fold(query);
    let mut rel = vec![0.0f32; m.nodes.len()];
    for node in &m.nodes {
        let i = node.id as usize;
        let s = scores[i];
        // A sector is structure, never a memory: a message cannot «name» it.
        if node.kind == NodeKind::Sector || !s.visible || s.superseded || s.chi < min_chi {
            continue;
        }
        let mut r = 0.0;
        if std::iter::once(&node.label)
            .chain(node.aliases.iter())
            .any(|a| text::contains_phrase(&folded, a))
        {
            r += 2.0;
        }
        if !q.is_empty() {
            let doc = text::tokens(&format!("{} {}", node.label, node.text));
            let hits = q.iter().filter(|t| doc.contains(t)).count() as f32;
            r += hits / (doc.len() as f32).sqrt().max(1.0) * 1.5;
        }
        rel[i] = r;
    }
    rel
}

/// `min_chi` filters what can come to mind: the chat uses −0.25 (no out-of-system memories),
/// the claim extractor uses −1 so it can recognise a rumor it is being told again.
pub fn recall(m: &Memory, scores: &[Score], query: &str, limit: usize, min_chi: f32) -> Vec<Recall> {
    recall_in(m, scores, query, limit, min_chi, None)
}

/// `recall`, seen through the tree of sectors when the memory has one: the push for being near
/// what the message names reorders the candidates (it never adds any), and what lives in
/// fiction stays out unless the message crosses into it. A `Sector` node never comes back.
pub fn recall_in(m: &Memory, scores: &[Score], query: &str, limit: usize, min_chi: f32, view: Option<&SectorView>) -> Vec<Recall> {
    let mut rel = lexical_seeds(m, scores, query, min_chi);

    // One hop of spreading activation from the seeds, weighted by edge strength.
    let seeds = rel.clone();
    for e in &m.edges {
        let w = match e.kind {
            EdgeKind::Causes => 0.45 * e.determinism(),
            // A1: Nothing records real associations yet: the old ones came from co-injection (every
            // injected pair linked), a clique that pulled itself back on any match. Inert until
            // something learns associations that mean something.
            EdgeKind::Relates => 0.0,
            EdgeKind::Contradicts | EdgeKind::Supersedes => 0.3,
            EdgeKind::Mentions => 0.2,
            EdgeKind::Precedes => 0.15,
            // Structure, not association: belonging and inspiration never carry activation (the
            // tree of sectors acts apart, as proximity in `sector_view`). If they did, a sector
            // would light up its whole membership and a fictional namesake would leak into the real one.
            EdgeKind::BelongsTo | EdgeKind::InspiredBy => 0.0,
        };
        let (f, t) = (e.from as usize, e.to as usize);
        if seeds[f] > 0.0 && scores[t].visible && !scores[t].superseded && scores[t].chi >= min_chi {
            rel[t] += seeds[f] * w;
        }
        if seeds[t] > 0.0 && scores[f].visible && !scores[f].superseded && scores[f].chi >= min_chi {
            rel[f] += seeds[t] * w * 0.7;
        }
    }
    for h in &m.hyperedges {
        let peak = h.members.iter().map(|&i| seeds[i as usize]).fold(0.0f32, f32::max);
        if peak > 0.0 {
            for &i in &h.members {
                if scores[i as usize].visible && !scores[i as usize].superseded && scores[i as usize].chi >= min_chi {
                    rel[i as usize] += peak * 0.15;
                }
            }
        }
    }

    let mut out: Vec<Recall> = m
        .nodes
        .iter()
        .filter(|node| rel[node.id as usize] > MIN_RELEVANCE && node.kind != NodeKind::Place && !scores[node.id as usize].superseded)
        .filter(|node| node.kind != NodeKind::Sector && view.is_none_or(|v| v.allows(node.id)))
        .map(|node| {
            let s = scores[node.id as usize];
            let recency = 0.3 * node.activation;
            let trust = 0.6 + 0.4 * s.chi.max(0.0);
            let relevance = (rel[node.id as usize] + recency) * trust;
            // A moderate push for the sectors near what is named: ×1 far away, ×1.25 in the same one.
            let relevance = match view {
                Some(v) => relevance * (1.0 + SECTOR_PUSH * v.prox[node.id as usize]),
                None => relevance,
            };
            Recall { id: node.id, relevance }
        })
        .collect();
    out.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    out
}

/// Decay of the proximity per step in the tree of sectors (the same λ as the owner's 3D engine).
pub const SECTOR_LAMBDA: f32 = 0.8;
/// How much being near the named sectors lifts a memory in the order: relevance × (1 + this × proximity).
pub const SECTOR_PUSH: f32 = 0.25;
/// Lexical strength from which a fictional node counts as named for crossing the border: the 2.0
/// that `lexical_seeds` gives a literal label or alias (weaker overlaps stay below it in practice).
pub const FICTION_NAMED: f32 = 2.0;
/// Key of the macro-sector whose members are fiction.
pub const FICTION_KEY: &str = "ficticio";

/// What the tree of sectors says about one message. Only exists when the memory has sectors:
/// without them (the memory of the first format) recall works exactly as it always did.
pub struct SectorView {
    /// Per node: proximity in the tree to the sectors of what the message names, in [0, 1].
    pub prox: Vec<f32>,
    /// Per node: it lives under the macro-sector «ficticio».
    pub fiction: Vec<bool>,
    /// The message crosses the border into fiction (names something that lives there).
    pub crossed: bool,
}

impl SectorView {
    /// Fiction only comes to mind when the message crosses into it.
    pub fn allows(&self, id: u32) -> bool {
        self.crossed || !self.fiction[id as usize]
    }
}

/// The tree of sectors for a message, or `None` if the memory has no sectors.
///
/// - `named`: lexical strength of each node for the message (`lexical_seeds`). The sectors of the
///   nodes at or above `anchor` are the sources; proximity falls as λ^d with the steps `d` in the
///   tree (same sector 1, parent or child λ, …) times how much the node belongs to its sector.
///   The root and the paths through it do not count: another macro-sector gets nothing.
/// - `border` / `border_text`: the same for the words that may cross into fiction (his message,
///   not what she saw). It crosses if a person, place, object or event under «ficticio» is
///   literally named (label or alias: `FICTION_NAMED`; a word in common with its text is not
///   enough: a long message about her shares words with half the lore), not its traits or facts,
///   whose aliases are everyday words, nor the namesake of herself; or if the message says the
///   name of a sector inside fiction («Arknights»).
///   Being inspired by something fictional (`InspiredBy`) is not enough: that edge is not
///   followed at all.
pub fn sector_view(m: &Memory, named: &[f32], border: &[f32], border_text: &str, anchor: f32) -> Option<SectorView> {
    if !m.nodes.iter().any(|n| n.kind == NodeKind::Sector) {
        return None;
    }
    let n = m.nodes.len();
    let is_sector = |i: usize| m.nodes[i].kind == NodeKind::Sector;
    // up[x]: sectors x belongs to, with how much; down[s]: sectors that hang from s.
    let mut up: Vec<Vec<(u32, f32)>> = vec![Vec::new(); n];
    let mut down: Vec<Vec<u32>> = vec![Vec::new(); n];
    for e in &m.edges {
        let (f, t) = (e.from as usize, e.to as usize);
        if e.kind != EdgeKind::BelongsTo || !is_sector(t) {
            continue;
        }
        let total = e.alpha + e.beta;
        up[f].push((e.to, if total > 0.0 { e.alpha / total } else { 0.0 }));
        if is_sector(f) {
            down[t].push(e.from);
        }
    }
    let is_root = |s: u32| up[s as usize].is_empty();
    // The fictional namesake of someone real (target of «inspirada_en»).
    let mut namesake = vec![false; n];
    for e in m.edges.iter().filter(|e| e.kind == EdgeKind::InspiredBy) {
        namesake[e.to as usize] = true;
    }

    // Steps from the sectors of what is named, over the sector tree without its root.
    let mut dist: Vec<Option<u32>> = vec![None; n];
    let mut frontier: Vec<u32> = Vec::new();
    for i in (0..n).filter(|&i| !is_sector(i) && named[i] >= anchor) {
        for &(s, _) in &up[i] {
            if !is_root(s) && dist[s as usize].is_none() {
                dist[s as usize] = Some(0);
                frontier.push(s);
            }
        }
    }
    let mut step = 0;
    while !frontier.is_empty() {
        step += 1;
        let mut next = Vec::new();
        for &s in &frontier {
            let around = up[s as usize].iter().map(|&(p, _)| p).chain(down[s as usize].iter().copied());
            for t in around {
                if !is_root(t) && dist[t as usize].is_none() {
                    dist[t as usize] = Some(step);
                    next.push(t);
                }
            }
        }
        frontier = next;
    }
    let prox: Vec<f32> = (0..n)
        .map(|i| {
            if is_sector(i) {
                return 0.0;
            }
            up[i]
                .iter()
                .filter_map(|&(s, w)| dist[s as usize].map(|d| w * SECTOR_LAMBDA.powi(d as i32)))
                .fold(0.0f32, f32::max)
        })
        .collect();

    // Everything under the macro-sector of fiction.
    let mut fiction = vec![false; n];
    let mut crossed = false;
    if let Some(f) = m.by_key(FICTION_KEY).filter(|&f| is_sector(f as usize)) {
        let mut inside = vec![false; n];
        inside[f as usize] = true;
        let mut todo = vec![f];
        while let Some(s) = todo.pop() {
            for &c in &down[s as usize] {
                if !inside[c as usize] {
                    inside[c as usize] = true;
                    todo.push(c);
                }
            }
        }
        let folded = text::fold(border_text);
        for i in 0..n {
            if is_sector(i) {
                fiction[i] = inside[i];
                if inside[i] && text::contains_phrase(&folded, &m.nodes[i].label) {
                    crossed = true;
                }
            } else if up[i].iter().any(|&(s, _)| inside[s as usize]) {
                fiction[i] = true;
                // Only a person, place, object or event of the story is a border: the traits and
                // facts of the seed carry everyday words as aliases («sola», «sentido»), and her own
                // name belongs to both Kal'tsits, so neither says the message is about fiction.
                let story = m.nodes[i].kind.is_entity() || m.nodes[i].kind == NodeKind::Event;
                if border[i] >= FICTION_NAMED && story && !namesake[i] {
                    crossed = true;
                }
            }
        }
    }
    Some(SectorView { prox, fiction, crossed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cthg::score;

    fn make_node(m: &mut Memory, label: &str, text: &str) -> u32 {
        m.add_node(Node {
            id: 0,
            kind: NodeKind::Fact,
            origin: Origin::Learned,
            flags: 0,
            period: 0,
            x: 0.0,
            y: 100.0,
            alpha: 5.0,
            beta: 1.0,
            activation: 0.0,
            recalls: 0,
            created_turn: 0,
            key: String::new(),
            label: label.to_string(),
            text: text.to_string(),
            aliases: vec![],
            module: 0,
            source: 0,
            born: 0.0,
        })
    }

    #[test]
    fn two_rules_injected_together_do_not_pull_each_other_in_on_the_next_query() {
        let mut m = Memory::default();
        let r1 = make_node(&mut m, "regla_manzana", "comer manzanas frescas por la mañana");
        let r2 = make_node(&mut m, "regla_motor", "revisar la presion de aceite del motor");
        // An associative edge like co-injection used to create
        m.upsert_edge(r1, r2, EdgeKind::Relates, 1.0, 1.0);

        let scores = score::score_all(&m, 200.0);
        // Query about apples should recall r1 but NOT r2
        let rec = recall(&m, &scores, "manzanas", 10, -0.25);
        let ids: Vec<u32> = rec.iter().map(|r| r.id).collect();
        assert!(ids.contains(&r1), "r1 should be recalled for query 'manzanas'");
        assert!(!ids.contains(&r2), "r2 must NOT be pulled in by associative Relates edge");
    }

    fn typed(m: &mut Memory, kind: NodeKind, key: &str, label: &str, text: &str, aliases: &[&str]) -> u32 {
        let id = make_node(m, label, text);
        let n = &mut m.nodes[id as usize];
        n.kind = kind;
        n.key = key.to_string();
        n.aliases = aliases.iter().map(|a| a.to_string()).collect();
        id
    }

    fn belongs(m: &mut Memory, node: u32, sector: u32) {
        m.upsert_edge(node, sector, EdgeKind::BelongsTo, 10.0, 0.0);
    }

    /// nucleo -> real -> {tecnologia, lugares}; nucleo -> ficticio -> arknights.
    struct Tree {
        m: Memory,
        real: u32,
        tech: u32,
        places: u32,
        ark: u32,
    }

    fn tree() -> Tree {
        let mut m = Memory::default();
        let sector = |m: &mut Memory, key: &str, label: &str, text: &str, parent: Option<u32>| {
            let id = typed(m, NodeKind::Sector, key, label, text, &[]);
            if let Some(p) = parent {
                belongs(m, id, p);
            }
            id
        };
        let root = sector(&mut m, "nucleo", "Núcleo", "el centro", None);
        let real = sector(&mut m, "real", "Real", "lo real", Some(root));
        let tech = sector(&mut m, "tecnologia", "Tecnología", "programas modelos hardware", Some(real));
        let places = sector(&mut m, "lugares", "Lugares", "sitios y casas", Some(real));
        let fiction = sector(&mut m, "ficticio", "Ficticio", "historias", Some(root));
        let ark = sector(&mut m, "arknights", "Arknights", "el mundo de Rhodes Island", Some(fiction));
        Tree { m, real, tech, places, ark }
    }

    fn ids(rec: &[Recall]) -> Vec<u32> {
        rec.iter().map(|r| r.id).collect()
    }

    fn view_of(m: &Memory, scores: &[Score], query: &str) -> SectorView {
        let named = lexical_seeds(m, scores, query, -1.0);
        sector_view(m, &named, &named, query, 0.5).expect("la memoria tiene sectores")
    }

    #[test]
    fn a_sector_is_never_named_nor_recalled_even_if_its_name_matches() {
        let mut t = tree();
        let fact = typed(&mut t.m, NodeKind::Fact, "", "Rust compila programas", "Rust compila programas muy rápido", &[]);
        belongs(&mut t.m, fact, t.tech);
        let scores = score::score_all(&t.m, 200.0);
        let seeds = lexical_seeds(&t.m, &scores, "háblame de tecnología, programas y modelos", -1.0);
        assert!(t.m.nodes.iter().filter(|n| n.kind == NodeKind::Sector).all(|n| seeds[n.id as usize] == 0.0), "a sector is not named");
        let q = "tecnología programas modelos hardware";
        let view = view_of(&t.m, &scores, q);
        for rec in [recall(&t.m, &scores, q, 50, -1.0), recall_in(&t.m, &scores, q, 50, -1.0, Some(&view))] {
            assert!(ids(&rec).contains(&fact), "the fact does come back");
            assert!(ids(&rec).iter().all(|&i| t.m.nodes[i as usize].kind != NodeKind::Sector), "{rec:?}");
        }
    }

    #[test]
    fn what_is_in_the_same_sector_as_what_is_named_wins_a_tie_but_does_not_filter() {
        let mut t = tree();
        // The far one is born first, so without the push it would come first.
        // Long enough that the single word they share with the message is a weak tie (< ANCHOR):
        // only Rust is named, so only its sector is a source.
        let body = "El servidor guarda copias encriptadas de fotografías antiguas, documentos escaneados, música favorita, recetas heredadas, planos arquitectónicos y viejos proyectos universitarios";
        // (The label does not repeat the message: a literal label is a strong tie, a seed.)
        let far = typed(&mut t.m, NodeKind::Fact, "", "Copias guardadas", body, &[]);
        let near = typed(&mut t.m, NodeKind::Fact, "", "Copias guardadas", body, &[]);
        let rust = typed(&mut t.m, NodeKind::Object, "", "Rust", "lenguaje", &["Rust"]);
        belongs(&mut t.m, far, t.places);
        belongs(&mut t.m, near, t.tech);
        belongs(&mut t.m, rust, t.tech);
        let scores = score::score_all(&t.m, 200.0);
        let q = "Rust y el servidor";
        let plain = ids(&recall(&t.m, &scores, q, 10, -1.0));
        let pos = |v: &[u32], x: u32| v.iter().position(|&i| i == x).unwrap();
        assert!(pos(&plain, far) < pos(&plain, near), "without the tree the tie goes to the older one");
        let view = view_of(&t.m, &scores, q);
        let pushed = recall_in(&t.m, &scores, q, 10, -1.0, Some(&view));
        let order = ids(&pushed);
        assert!(pos(&order, near) < pos(&order, far), "the same sector as Rust wins: {pushed:?}");
        assert!(order.contains(&far), "the other sector is pushed down, not removed");
        let rel = |id: u32| pushed.iter().find(|r| r.id == id).unwrap().relevance;
        let want = (1.0 + SECTOR_PUSH) / (1.0 + SECTOR_PUSH * SECTOR_LAMBDA * SECTOR_LAMBDA);
        assert!((rel(near) / rel(far) - want).abs() < 1e-4, "×1.25 in the same sector against ×1.16 in a sibling (λ² = 0.64): {} / {}", rel(near), rel(far));
    }

    #[test]
    fn proximity_falls_as_lambda_to_the_distance_and_stops_at_the_macro_sector() {
        let mut t = tree();
        let sub = typed(&mut t.m, NodeKind::Sector, "serv", "Servidores", "máquinas", &[]);
        belongs(&mut t.m, sub, t.tech);
        let seed = typed(&mut t.m, NodeKind::Object, "", "Rust", "lenguaje", &["Rust"]);
        belongs(&mut t.m, seed, t.tech);
        let (same, child, sibling, grand, other, fict, none) = (
            typed(&mut t.m, NodeKind::Fact, "", "a", "uno", &[]),
            typed(&mut t.m, NodeKind::Fact, "", "b", "dos", &[]),
            typed(&mut t.m, NodeKind::Fact, "", "c", "tres", &[]),
            typed(&mut t.m, NodeKind::Fact, "", "d", "cuatro", &[]),
            typed(&mut t.m, NodeKind::Fact, "", "e", "cinco", &[]),
            typed(&mut t.m, NodeKind::Fact, "", "f", "seis", &[]),
            typed(&mut t.m, NodeKind::Fact, "", "g", "siete", &[]),
        );
        let deeper = typed(&mut t.m, NodeKind::Sector, "serv2", "Racks", "máquinas", &[]);
        belongs(&mut t.m, deeper, sub);
        belongs(&mut t.m, same, t.tech);
        belongs(&mut t.m, child, sub);
        belongs(&mut t.m, sibling, t.places);
        belongs(&mut t.m, grand, deeper);
        belongs(&mut t.m, other, t.real);
        belongs(&mut t.m, fict, t.ark);
        let scores = score::score_all(&t.m, 200.0);
        let v = view_of(&t.m, &scores, "Rust");
        let l = SECTOR_LAMBDA;
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(close(v.prox[same as usize], 1.0), "same sector");
        assert!(close(v.prox[child as usize], l), "child sector");
        assert!(close(v.prox[other as usize], l), "parent sector");
        assert!(close(v.prox[sibling as usize], l * l), "sibling sector");
        assert!(close(v.prox[grand as usize], l * l), "grandchild sector");
        assert_eq!(v.prox[fict as usize], 0.0, "another macro-sector gets nothing");
        assert_eq!(v.prox[none as usize], 0.0, "without sector, nothing");
        // Nothing is named: no push at all.
        let v = view_of(&t.m, &scores, "nada de nada");
        assert!(v.prox.iter().all(|&p| p == 0.0));
    }

    #[test]
    fn fiction_stays_out_unless_the_message_crosses_into_it() {
        let mut t = tree();
        let real = typed(&mut t.m, NodeKind::Fact, "", "Servidor", "El servidor corre Linux", &[]);
        let fict = typed(&mut t.m, NodeKind::Fact, "", "Vigilancia", "Mon3tr vigila el servidor de Rhodes", &[]);
        let mon3tr = typed(&mut t.m, NodeKind::Person, "mon3tr", "Mon3tr", "la criatura", &["Mon3tr"]);
        let trait_ = typed(&mut t.m, NodeKind::Trait, "soledad", "Soledad", "vive sola", &["sola", "soledad"]);
        belongs(&mut t.m, real, t.tech);
        belongs(&mut t.m, fict, t.ark);
        belongs(&mut t.m, mon3tr, t.ark);
        belongs(&mut t.m, trait_, t.ark);
        let scores = score::score_all(&t.m, 200.0);
        let got = |q: &str| {
            let v = view_of(&t.m, &scores, q);
            (ids(&recall_in(&t.m, &scores, q, 50, -1.0, Some(&v))), v.crossed)
        };
        let (a, crossed) = got("¿Qué pasa con el servidor?");
        assert!(!crossed && a.contains(&real) && !a.contains(&fict), "{a:?}");
        // The old function (without the tree) does mix them: that is what the border is for.
        assert!(ids(&recall(&t.m, &scores, "¿Qué pasa con el servidor?", 50, -1.0)).contains(&fict));
        let (b, crossed) = got("¿Qué pasa con Mon3tr y el servidor?");
        assert!(crossed && b.contains(&fict) && b.contains(&real), "naming something of fiction crosses: {b:?}");
        let (c, crossed) = got("Cuéntame algo de Arknights y del servidor");
        assert!(crossed && c.contains(&fict), "naming a sector of fiction crosses: {c:?}");
        let (d, crossed) = got("estás sola con el servidor");
        assert!(!crossed && !d.contains(&fict) && !d.contains(&trait_), "an everyday alias of a trait does not cross: {d:?}");
    }

    #[test]
    fn being_inspired_by_fiction_is_not_a_border_and_her_own_name_does_not_cross() {
        let mut t = tree();
        let me = typed(&mut t.m, NodeKind::Person, "yo", "Quién eres", "Un programa con la forma de Kal'tsit", &[]);
        let twin = typed(&mut t.m, NodeKind::Person, "kaltsit", "Kal'tsit de Rhodes Island", "Médica de Rhodes Island", &["Kal'tsit", "Kaltsit"]);
        let story = typed(&mut t.m, NodeKind::Fact, "", "Babel", "Kal'tsit trabajó en Babel con el servidor", &[]);
        belongs(&mut t.m, me, t.real);
        belongs(&mut t.m, twin, t.ark);
        belongs(&mut t.m, story, t.ark);
        t.m.upsert_edge(me, twin, EdgeKind::InspiredBy, 4.0, 1.0);
        let scores = score::score_all(&t.m, 200.0);
        for q in ["Kal'tsit, revisa el servidor", "Kaltsit dime algo sobre Babel y el servidor"] {
            let v = view_of(&t.m, &scores, q);
            let got = ids(&recall_in(&t.m, &scores, q, 50, -1.0, Some(&v)));
            assert!(!v.crossed, "{q}");
            assert!(!got.contains(&story) && !got.contains(&twin), "{q}: {got:?}");
        }
        // The edge itself carries no activation: the twin is not lit up by naming the real one.
        let q = "Quién eres";
        let v = view_of(&t.m, &scores, q);
        let got = ids(&recall_in(&t.m, &scores, q, 50, -1.0, Some(&v)));
        assert!(!got.contains(&twin), "{got:?}");
    }

    #[test]
    fn without_sectors_there_is_no_view_and_recall_is_the_same() {
        let mut m = Memory::default();
        let a = make_node(&mut m, "regla_manzana", "comer manzanas frescas por la mañana");
        let b = make_node(&mut m, "otra", "manzanas verdes del huerto");
        m.upsert_edge(a, b, EdgeKind::Causes, 8.0, 2.0);
        let scores = score::score_all(&m, 200.0);
        let named = lexical_seeds(&m, &scores, "manzanas", -1.0);
        assert!(sector_view(&m, &named, &named, "manzanas", 0.5).is_none());
        let (x, y) = (recall(&m, &scores, "manzanas", 10, -0.25), recall_in(&m, &scores, "manzanas", 10, -0.25, None));
        assert_eq!(x.iter().map(|r| (r.id, r.relevance)).collect::<Vec<_>>(), y.iter().map(|r| (r.id, r.relevance)).collect::<Vec<_>>());
    }

    #[test]
    fn superseded_node_is_not_recalled() {
        let mut m = Memory::default();
        let old = make_node(&mut m, "regla_vieja", "guardar registros en formato plano txt");
        let new = make_node(&mut m, "regla_nueva", "guardar registros en formato binario cthg");
        m.upsert_edge(new, old, EdgeKind::Supersedes, 1.0, 1.0);

        let scores = score::score_all(&m, 200.0);
        let rec = recall(&m, &scores, "registros", 10, -0.25);
        let ids: Vec<u32> = rec.iter().map(|r| r.id).collect();
        assert!(ids.contains(&new), "new rule should be recalled");
        assert!(!ids.contains(&old), "superseded old rule must NOT be recalled");
    }
}
