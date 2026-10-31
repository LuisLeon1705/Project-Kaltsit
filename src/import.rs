//! Curated import (phase 3 of the sectored memory, see `DISENO-MEMORIA-SECTORES.md` §9).
//!
//! Builds the NEW memory from the CURRENT one: a sectored seed (the owner's personal
//! `seed/kaltsit_sectores.ron`, read from disk, or a base seed) plus what is worth keeping of the old memory, each thing placed in a sector, and a report for
//! the owner to review. Nothing here touches the files on disk and nothing calls it yet: the review
//! comes first, applying it is a later step.
//!
//! The rules are deterministic and use no language model. Every old node gets exactly one
//! decision (imported to a sector, merged into a seed node, or discarded with a reason) and the
//! report lists all of them.
#![allow(dead_code)] // phase 3: only the review test calls this until the owner decides

use crate::cthg::model::*;
use crate::cthg::sectors::{self, classify, episode_kind, EpisodeKind, CONVERSACIONES, FULL, KEYWORDS, POR_CLASIFICAR, SUBJECT_SECTORS};
use crate::lang::{conversations_text, Lang};
use crate::cthg::{score, text};
use crate::mind::TERRA;
use crate::modules::{COSAS, CORE, SABER};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Why an old node was left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The old identity core: the guided core of the seed replaces it.
    OldCore,
    /// Canon from before the roleplay (y < `TERRA`): the seed already has it under Ficticio.
    TerraCanon,
    /// An episode with no message of the user behind it: something she said on her own after
    /// staying silent, or any episode that is not one of the known forms (see `episode_kind`).
    Episode,
    /// An episode of what she saw on the screen, or of what she said on her own about it.
    ScreenEpisode,
    /// Its own memory finds no sense in it (χ < 0).
    NegativeChi,
    /// A sector of the old structure (there are none in a format-1 memory).
    OldSector,
}

impl Reason {
    pub const ALL: [Reason; 6] = [Reason::OldCore, Reason::TerraCanon, Reason::ScreenEpisode, Reason::Episode, Reason::NegativeChi, Reason::OldSector];

    pub fn name(self) -> &'static str {
        match self {
            Reason::OldCore => "núcleo viejo (lo sustituye el núcleo guiado de la semilla)",
            Reason::TerraCanon => "canon de Terra (la semilla ya lo tiene en Ficticio → Arknights)",
            Reason::Episode => "episodio sin mensaje del usuario (comentario suyo por su cuenta, o forma no reconocida)",
            Reason::ScreenEpisode => "episodio de pantalla",
            Reason::NegativeChi => "χ < 0 (su propia memoria no le encuentra sentido)",
            Reason::OldSector => "sector de la estructura vieja",
        }
    }
}

/// What happened to one old node.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Imported, under the sector with this key.
    Imported(String),
    /// Merged into the seed node with this key.
    Merged(String),
    Discarded(Reason),
}

#[derive(Clone, Debug)]
pub struct NodeReport {
    pub old_id: u32,
    pub key: String,
    pub kind: NodeKind,
    pub label: String,
    pub chi: f32,
    pub decision: Decision,
    /// The rule that decided, in words (for the sorting rule: the keywords that matched).
    pub why: String,
    /// Its id in the new memory (imported and merged nodes).
    pub new_id: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct EquipmentReport {
    pub key: String,
    pub label: String,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    /// One entry per old node, in old id order.
    pub nodes: Vec<NodeReport>,
    /// Nodes the seed brought (sectors, guided core, the user, Arknights canon).
    pub seed_nodes: usize,
    /// (key, path like "Real → Personas") of every sector, in tree order.
    pub sectors: Vec<(String, String)>,
    pub equipment: Vec<EquipmentReport>,
    pub edges_old: usize,
    pub edges_kept: usize,
    /// An end was not imported.
    pub edges_dropped: usize,
    /// Both ends became the same node (a merge) or the edge already existed.
    pub edges_collapsed: usize,
    /// «Precedes» links added between consecutive imported conversations, to repair the chain
    /// that the discarded screen episodes and comments of her own had cut.
    pub precedes_added: usize,
    pub scenes_kept: usize,
    pub scenes_dropped: usize,
    pub meta_kept: Vec<String>,
    pub meta_dropped: Vec<(String, &'static str)>,
    pub chat_messages: usize,
    pub recalled_translated: usize,
    pub recalled_dropped: usize,
}

// The sorting rule, the inbox («Por clasificar») and the sector of the conversations are shared
// with learning: see `cthg::sectors`. The import creates «Conversaciones» only when there is a
// conversation to put in it (the seed does not have it).

/// What the first version of this import (edges between imported nodes, history quotes that
/// still exist) gave when no episode was imported, for the «Cambios» table: (edges kept,
/// quotes translated, quotes dropped).
const BEFORE_EPISODES: (usize, usize, usize) = (110, 599, 1462);

/// Nodes per sector that the first pass (before the frame and subject rules) imported from the
/// real memory, for the «Cambios respecto a la primera pasada» table of the report.
const FIRST_PASS: [(&str, usize); 7] =
    [("tecnologia", 19), ("juegos", 5), ("conocimiento", 2), ("marcadas", 2), ("opiniones", 4), ("preferencias", 1), (POR_CLASIFICAR, 87)];

impl ImportReport {
    pub fn imported(&self) -> impl Iterator<Item = &NodeReport> {
        self.nodes.iter().filter(|n| matches!(n.decision, Decision::Imported(_)))
    }

    pub fn merged(&self) -> impl Iterator<Item = &NodeReport> {
        self.nodes.iter().filter(|n| matches!(n.decision, Decision::Merged(_)))
    }

    pub fn discarded(&self, why: Reason) -> impl Iterator<Item = &NodeReport> {
        self.nodes.iter().filter(move |n| n.decision == Decision::Discarded(why))
    }

    /// Imported nodes per sector key, only the sectors that received something.
    pub fn by_sector(&self) -> Vec<(String, usize)> {
        let mut c: HashMap<&str, usize> = HashMap::new();
        for n in self.imported() {
            if let Decision::Imported(s) = &n.decision {
                *c.entry(s.as_str()).or_default() += 1;
            }
        }
        self.sectors.iter().filter_map(|(k, _)| c.get(k.as_str()).map(|&n| (k.clone(), n))).collect()
    }

    pub fn path_of<'a>(&'a self, sector: &'a str) -> &'a str {
        self.sectors.iter().find(|(k, _)| k == sector).map(|(_, p)| p.as_str()).unwrap_or(sector)
    }

    /// The review document, in Spanish, for the owner.
    pub fn to_markdown(&self, now: f64) -> String {
        let intro = format!(
            "Documento generado por la prueba `import::tests::curated_import_over_a_copy_of_the_real_memory` sobre una COPIA de la memoria actual (fecha: {}). \
             No se ha aplicado nada: `data/` sigue exactamente como estaba. Tú decides qué se aplica.\n",
            date_of(now)
        );
        self.markdown("# Importación curada: revisión", &intro)
    }

    /// The same document for an import that WAS applied (`kaltsit --migrar-sectores`).
    pub fn to_markdown_applied(&self, now: f64, backup: &str) -> String {
        let intro = format!(
            "Documento generado por `kaltsit --migrar-sectores` al aplicar la importación curada a la memoria real (fecha: {}). \
             La memoria anterior, completa, está en `{backup}` (esta carpeta). Las secciones de abajo describen lo que se hizo con cada recuerdo; \
             las comparaciones «antes/ahora» de la pasada de revisión ya no se refieren a este paso.\n",
            date_of(now)
        );
        self.markdown("# Importación aplicada", &intro)
    }

    fn markdown(&self, title: &str, intro: &str) -> String {
        use std::fmt::Write;
        let mut o = String::new();
        let imported = self.imported().count();
        let merged = self.merged().count();
        let discarded: usize = Reason::ALL.iter().map(|r| self.discarded(*r).count()).sum();
        let _ = writeln!(o, "{title}\n");
        let _ = writeln!(o, "{intro}");
        let _ = writeln!(o, "## Resumen\n");
        let _ = writeln!(o, "| Concepto | Nodos |\n|---|---:|");
        let _ = writeln!(o, "| Nodos en la memoria actual | {} |", self.nodes.len());
        let _ = writeln!(o, "| Importados a un sector | {imported} |");
        let _ = writeln!(o, "| Fusionados con un nodo de la semilla (el usuario) | {merged} |");
        let _ = writeln!(o, "| Descartados | {discarded} |");
        for r in Reason::ALL {
            let n = self.discarded(r).count();
            if n > 0 {
                let _ = writeln!(o, "| &nbsp;&nbsp;· {} | {n} |", r.name());
            }
        }
        let por = self.imported().filter(|n| n.decision == Decision::Imported(POR_CLASIFICAR.into())).count();
        let _ = writeln!(o, "| De los importados, en «Por clasificar» | {por} |");
        let conv = self.imported().filter(|n| n.decision == Decision::Imported(CONVERSACIONES.into())).count();
        let _ = writeln!(o, "| De los importados, en «Conversaciones» (episodios) | {conv} |");
        let _ = writeln!(o, "| Nodos que trae la semilla nueva (sectores, núcleo guiado, usuario, Arknights) | {} |", self.seed_nodes);
        let _ = writeln!(o, "| Nodos de equipo creados | {} |", self.equipment.len());
        let _ = writeln!(o, "| **Nodos en la memoria nueva** | **{}** |", self.seed_nodes + imported + self.equipment.len());
        let _ = writeln!(o, "\n| Aristas | |\n|---|---:|");
        let _ = writeln!(o, "| En la memoria actual | {} |", self.edges_old);
        let _ = writeln!(o, "| Conservadas (unen dos nodos importados, con ids traducidos) | {} (antes de importar episodios: {}) |", self.edges_kept, BEFORE_EPISODES.0);
        let _ = writeln!(o, "| Descartadas (un extremo no se importó) | {} |", self.edges_dropped);
        let _ = writeln!(o, "| Colapsadas (por la fusión, o ya existían) | {} |", self.edges_collapsed);
        let _ = writeln!(o, "| «Precedes» añadidas entre conversaciones consecutivas (la cadena que cortó el descarte de pantalla) | {} |", self.precedes_added);
        let _ = writeln!(o, "| Escenas conservadas / descartadas | {} / {} |", self.scenes_kept, self.scenes_dropped);
        let _ = writeln!(o, "\n| Historial de chat | |\n|---|---:|");
        let _ = writeln!(o, "| Mensajes (el texto no cambia) | {} |", self.chat_messages);
        let _ = writeln!(o, "| Recuerdos citados que siguen existiendo (traducidos) | {} (antes de importar episodios: {}) |", self.recalled_translated, BEFORE_EPISODES.1);
        let _ = writeln!(o, "| Recuerdos citados que ya no existen (quitados de la cita) | {} (antes: {}) |", self.recalled_dropped, BEFORE_EPISODES.2);

        let _ = writeln!(o, "\n## Importados por sector\n");
        let _ = writeln!(o, "| Sector | Nodos |\n|---|---:|");
        for (k, n) in self.by_sector() {
            let _ = writeln!(o, "| {} | {n} |", self.path_of(&k));
        }

        let _ = writeln!(o, "\n## Cambios respecto a la primera pasada\n");
        let _ = writeln!(
            o,
            "La segunda pasada cambió solo la clasificación por palabras clave: quita el marco «el usuario afirma que…» antes de clasificar, da prioridad al tema, \
             usa el sujeto (el usuario → Personas, Kal'tsit → Experiencias u Opiniones) cuando no hay tema, reserva Preferencias para las de ella y añade vocabulario \
             (Ideas, Ética, Sistemas, Minecraft, archivos de código…). Descartes, fusión, equipos, historial y metadatos no cambian.\n"
        );
        let _ = writeln!(
            o,
            "**Tercera pasada (episodios).** Ya no se descartan todos los episodios. Los de CONVERSACIÓN (el mensaje del usuario y la respuesta de Kal'tsit) se importan a un \
             sector nuevo, Mi mundo → Experiencias → Conversaciones, con su texto, fechas, evidencia, activación y recalls; el sector solo existe si hay alguno. \
             Los de PANTALLA («Vio la pantalla» y «(mira la pantalla)») se descartan con el motivo «episodio de pantalla»; también se descartan los comentarios suyos \
             sin mensaje del usuario («(guarda silencio tras borrar lo que escribía)» y cualquier otra marca entre paréntesis). Por eso se conservan más aristas \
             (episodio → hecho, episodio → episodio) y se traducen más recuerdos citados del historial. Es la única regla que cambia: la clasificación por palabras clave no. \
             El sector de Conversaciones no está en `seed/kaltsit_sectores.ron`: lo crea la importación.\n"
        );
        let _ = writeln!(o, "| Concepto | Antes (episodios descartados) | Ahora |\n|---|---:|---:|");
        let _ = writeln!(o, "| Nodos importados | {} | {imported} |", FIRST_PASS.iter().map(|(_, n)| n).sum::<usize>());
        let _ = writeln!(o, "| Aristas conservadas | {} | {} |", BEFORE_EPISODES.0, self.edges_kept);
        let _ = writeln!(o, "| Recuerdos citados traducidos | {} | {} |", BEFORE_EPISODES.1, self.recalled_translated);
        let _ = writeln!(o, "| Recuerdos citados quitados | {} | {} |\n", BEFORE_EPISODES.2, self.recalled_dropped);
        let _ = writeln!(o, "| Sector | Antes | Ahora |\n|---|---:|---:|");
        let now_counts: HashMap<String, usize> = self.by_sector().into_iter().collect();
        for (k, path) in &self.sectors {
            let before = FIRST_PASS.iter().find(|(f, _)| f == k).map_or(0, |(_, n)| *n);
            let now = now_counts.get(k).copied().unwrap_or(0);
            if before + now > 0 {
                let _ = writeln!(o, "| {path} | {before} | {now} |");
            }
        }
        let _ = writeln!(o, "| **Total importados** | **{}** | **{imported}** |", FIRST_PASS.iter().map(|(_, n)| n).sum::<usize>());

        let _ = writeln!(o, "\n## Qué entra en cada sector\n");
        let _ = writeln!(o, "Cada fila: el recuerdo (recortado), su tipo y su χ en la memoria actual.\n");
        for (k, _) in self.by_sector() {
            if k == POR_CLASIFICAR {
                continue;
            }
            let _ = writeln!(o, "### {}\n", self.path_of(&k));
            let all: Vec<&NodeReport> = self.imported().filter(|n| n.decision == Decision::Imported(k.clone())).collect();
            // Hundreds of conversations would bury the document: the count and ten examples, evenly spread.
            let shown: Vec<&NodeReport> = if k == CONVERSACIONES && all.len() > 10 {
                let _ = writeln!(o, "{} episodios de conversación; no se listan uno a uno, estos son 10 de ejemplo repartidos en el tiempo.\n", all.len());
                (0..10).map(|i| all[i * (all.len() - 1) / 9]).collect()
            } else {
                all
            };
            let _ = writeln!(o, "| Recuerdo | Tipo | χ |\n|---|---|---:|");
            for n in shown {
                let _ = writeln!(o, "| {} | {} | {:.2} |", cell(&n.label, 80), n.kind.name(), n.chi);
            }
            let _ = writeln!(o);
        }

        let _ = writeln!(o, "## Por clasificar\n");
        let _ = writeln!(
            o,
            "Lo que no encajó con claridad en un solo sector (o en ninguno). Se importa a Mi mundo → Por clasificar y su reflexión lo ubicará. \
             Es preferible a ponerlo en un sector equivocado.\n"
        );
        let _ = writeln!(o, "| Recuerdo | Tipo | χ | Por qué |\n|---|---|---:|---|");
        for n in self.imported().filter(|n| n.decision == Decision::Imported(POR_CLASIFICAR.into())) {
            let _ = writeln!(o, "| {} | {} | {:.2} | {} |", cell(&n.label, 80), n.kind.name(), n.chi, cell(&n.why, 90));
        }

        let _ = writeln!(o, "\n## Fusiones\n");
        let merges: Vec<&NodeReport> = self.merged().collect();
        if merges.is_empty() {
            let _ = writeln!(o, "Ninguna.");
        }
        for n in merges {
            if let Decision::Merged(k) = &n.decision {
                let _ = writeln!(o, "- «{}» (clave `{}`) → nodo `{k}` de la semilla. {}", cell(&n.label, 80), n.key, n.why);
            }
        }

        let _ = writeln!(o, "\n## Equipos donde existió\n");
        if self.equipment.is_empty() {
            let _ = writeln!(o, "Ninguno (la tabla de orígenes no tenía equipos).");
        }
        for e in &self.equipment {
            let _ = writeln!(o, "- **{}** (`{}`): {}", cell(&e.label, 60), e.key, e.text);
        }

        let _ = writeln!(o, "\n## Descartados\n");
        for r in Reason::ALL {
            let list: Vec<&NodeReport> = self.discarded(r).collect();
            if list.is_empty() {
                continue;
            }
            let _ = writeln!(o, "### {} ({})\n", r.name(), list.len());
            // Thousands of episodes would bury the document: they are only counted.
            if matches!(r, Reason::Episode | Reason::ScreenEpisode) {
                let _ = writeln!(o, "Solo se cuentan; no se listan uno a uno.\n");
                continue;
            }
            let _ = writeln!(o, "| Recuerdo | Tipo | χ |\n|---|---|---:|");
            for n in list {
                let _ = writeln!(o, "| {} | {} | {:.2} |", cell(&n.label, 80), n.kind.name(), n.chi);
            }
            let _ = writeln!(o);
        }

        let _ = writeln!(o, "## Metadatos\n");
        let _ = writeln!(
            o,
            "Se conservan los vitales y de configuración (hormonas, homeostasis, ajustes, usuario, identidad, turno, nacimiento…). \
             No se conservan los que describen la estructura vieja: la semilla antigua y la persona/escena/saludo, que sustituye la nueva semilla.\n"
        );
        let _ = writeln!(o, "- Conservados ({}): {}", self.meta_kept.len(), self.meta_kept.join(", "));
        for (k, why) in &self.meta_dropped {
            let _ = writeln!(o, "- Descartado `{k}`: {why}");
        }
        o
    }
}

fn cell(s: &str, max: usize) -> String {
    text::clip(&s.replace(['\n', '\r'], " ").replace('|', "\\|"), max)
}

/// "2026-10-25" in UTC (no time zone lookups: the report must read the same everywhere).
pub fn date_of(unix: f64) -> String {
    let days = (unix / 86_400.0).floor() as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn is_suspension(n: &Node) -> bool {
    n.kind == NodeKind::Event && (n.label.starts_with("Suspensión crítica") || n.label.starts_with("Salida de suspensión"))
}

/// The primary user as the old memory kept them: the seed's «creador», or any person named just
/// as the user (`user_folded` is the user's name, folded).
fn is_user(n: &Node, user_folded: &str) -> bool {
    n.kind == NodeKind::Person && (n.key == "creador" || text::fold(&n.label).trim() == user_folded)
}

/// The origin the seed's nodes are signed with: «semilla (<file the seed was built from>)».
fn seed_model(guided: &Memory) -> String {
    format!("semilla ({})", guided.meta("seed").unwrap_or("kaltsit_sectores.ron"))
}

/// The key of the seed's node for the primary user: «usuario» in the base seeds; in the owner's
/// personal seed (which names the user in its own key), the person of the present in Personas.
fn user_key(guided: &Memory) -> String {
    if guided.by_key("usuario").is_some() {
        return "usuario".into();
    }
    let personas = sectors::sector_by_key(guided, "personas");
    guided
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Person && n.y > TERRA && !n.has(flags::CORE) && personas.is_some_and(|s| sectors::parents_of(guided, n.id).iter().any(|&(p, _)| p == s)))
        .map(|n| n.key.clone())
        .unwrap_or_else(|| "usuario".into())
}

fn fnv(s: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in s.bytes() {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    h
}

/// Key of a node that is not in the seed: its own key, or one made from its old id when it had
/// none; a trailing «-viejo» keeps it apart from a seed key it would collide with.
fn fresh_key(taken: &HashSet<String>, key: &str, old_id: u32) -> String {
    let mut k = if key.is_empty() { format!("viejo-{old_id:x}") } else { key.to_string() };
    while taken.contains(&k) {
        k.push_str("-viejo");
    }
    k
}

/// Builds the new memory from the current one on top of `guided`, the sectored seed already
/// built (the owner's personal one or a base one with the user's name). `now` (unix seconds)
/// dates the seed's nodes and the import itself.
pub fn curated(old: &Memory, now: f64, guided: Memory) -> Result<(Memory, ImportReport), String> {
    let mut new = guided;
    let user_node = user_key(&new);
    // Whose words the history holds: the user the old memory records. This import is the
    // migration of the owner's own memory, whose user is the one the seed names even where the old
    // file says nothing.
    let seed_user = new.by_key(&user_node).and_then(|id| new.nodes[id as usize].aliases.first().cloned()).unwrap_or_default();
    let import_user = old.meta("usuario").unwrap_or(&seed_user);
    let user_folded = text::fold(import_user);
    let mut rep = ImportReport { seed_nodes: new.nodes.len(), edges_old: old.edges.len(), chat_messages: old.chat.len(), ..ImportReport::default() };

    // Sources: the old table as it is, plus the one the seed's nodes are signed with.
    new.sources = old.sources.clone();
    let seed_src = new.source_id(&Source { mac: String::new(), host: String::new(), model: seed_model(&new) });
    for n in new.nodes.iter_mut() {
        n.source = seed_src;
        n.born = now;
    }

    let sector_id = |m: &Memory, key: &str| m.by_key(key).ok_or_else(|| format!("la semilla no tiene el sector {key}"));
    let s_personas = sector_id(&new, "personas")?;
    let s_opiniones = sector_id(&new, "opiniones")?;
    let s_marcadas = sector_id(&new, "marcadas")?;
    let s_equipos = sector_id(&new, "equipos")?;
    let by_sector: HashMap<&str, u32> = KEYWORDS.iter().map(|(k, _)| *k).chain(SUBJECT_SECTORS).chain([POR_CLASIFICAR]).filter_map(|k| new.by_key(k).map(|id| (k, id))).collect();

    // χ as seen from the end of time: the whole memory counts, nothing is «in the future».
    let scores = score::score_all(old, f64::INFINITY);
    let mut taken: HashSet<String> = new.nodes.iter().map(|n| n.key.clone()).collect();
    let mut map: Vec<Option<u32>> = vec![None; old.nodes.len()];
    let mut s_conversaciones: Option<u32> = None;

    for o in &old.nodes {
        let chi = scores[o.id as usize].chi;
        let mut r = NodeReport { old_id: o.id, key: o.key.clone(), kind: o.kind, label: o.label.clone(), chi, decision: Decision::Discarded(Reason::Episode), why: String::new(), new_id: None };
        // Where does it go? (sector id, sector key, why) or a reason to leave it out.
        enum Fate {
            Merge,
            Go(u32, String, String),
            Out(Reason),
        }
        let fate = if is_user(o, user_folded.trim()) {
            Fate::Merge
        } else if o.kind == NodeKind::Sector {
            Fate::Out(Reason::OldSector)
        } else if o.has(flags::CORE) {
            Fate::Out(Reason::OldCore)
        } else if o.kind == NodeKind::Episode {
            match episode_kind(&o.label) {
                EpisodeKind::Conversation => {
                    // The sector is born with the first conversation, so it never exists empty.
                    let s = match s_conversaciones {
                        Some(s) => s,
                        None => {
                            sector_id(&new, "experiencias")?;
                            let id = sectors::ensure_structural(&mut new, CONVERSACIONES, "Conversaciones", &conversations_text(Lang::Es, import_user), "experiencias").ok_or("no se pudo crear el sector de conversaciones")?;
                            taken.insert(CONVERSACIONES.into());
                            s_conversaciones = Some(id);
                            id
                        }
                    };
                    Fate::Go(s, CONVERSACIONES.into(), "mensaje del usuario y su respuesta".into())
                }
                EpisodeKind::Screen => Fate::Out(Reason::ScreenEpisode),
                EpisodeKind::Own => Fate::Out(Reason::Episode),
            }
        } else if o.y < TERRA {
            Fate::Out(Reason::TerraCanon)
        } else if is_suspension(o) {
            Fate::Go(s_marcadas, "marcadas".into(), "suspensión crítica o salida de ella".into())
        } else if o.kind == NodeKind::Person {
            Fate::Go(s_personas, "personas".into(), "persona de su vida".into())
        } else if chi < 0.0 {
            Fate::Out(Reason::NegativeChi)
        } else if o.module == SABER {
            Fate::Go(s_opiniones, "opiniones".into(), "su saber: lo que concluyó por sí misma".into())
        } else {
            let p = classify(o.kind, &o.label, &o.text, import_user);
            Fate::Go(by_sector[p.key], p.key.to_string(), p.why)
        };
        match fate {
            Fate::Out(reason) => r.decision = Decision::Discarded(reason),
            Fate::Merge => {
                // The user is the seed's node: its guided text stays, the old one only adds.
                let id = new.by_key(&user_node).ok_or("la semilla no tiene al usuario principal")?;
                let l = &mut new.nodes[id as usize];
                let before = (l.alpha, l.beta);
                for a in &o.aliases {
                    if !l.aliases.iter().any(|x| text::fold(x) == text::fold(a)) {
                        l.aliases.push(a.clone());
                    }
                }
                if o.alpha + o.beta > l.alpha + l.beta {
                    l.alpha = o.alpha;
                    l.beta = o.beta;
                }
                l.activation = l.activation.max(o.activation);
                l.recalls = l.recalls.max(o.recalls);
                l.created_turn = l.created_turn.min(o.created_turn);
                map[o.id as usize] = Some(id);
                r.decision = Decision::Merged(user_node.clone());
                r.new_id = Some(id);
                r.why = format!("se conserva el texto guiado; evidencia α/β {:.1}/{:.1} (antes {:.1}/{:.1}), activación {:.2}, {} recalls", l.alpha, l.beta, before.0, before.1, l.activation, l.recalls);
            }
            Fate::Go(sector, key, why) => {
                let mut n = o.clone();
                n.key = fresh_key(&taken, &o.key, o.id);
                taken.insert(n.key.clone());
                n.flags &= !(flags::CORE | flags::PERSONAL_CORE);
                n.period = 0;
                n.created_turn = o.created_turn;
                // Extra modules brought from other computers fold into the four core ones.
                n.module = if (o.module as usize) < CORE.len() { o.module } else { COSAS };
                let id = new.add_node(n);
                new.upsert_edge(id, sector, EdgeKind::BelongsTo, FULL.0, FULL.1);
                map[o.id as usize] = Some(id);
                r.decision = Decision::Imported(key);
                r.new_id = Some(id);
                r.why = why;
            }
        }
        rep.nodes.push(r);
    }

    // Edges between nodes that came across, with translated ids.
    for e in &old.edges {
        match (map[e.from as usize], map[e.to as usize]) {
            (Some(a), Some(b)) if a != b => {
                let i = match new.find_edge(a, b, e.kind) {
                    Some(i) => {
                        rep.edges_collapsed += 1;
                        i
                    }
                    None => {
                        rep.edges_kept += 1;
                        new.upsert_edge(a, b, e.kind, e.alpha, e.beta)
                    }
                };
                new.edges[i].hebb = new.edges[i].hebb.max(e.hebb);
                new.edges[i].created_turn = new.edges[i].created_turn.max(e.created_turn);
            }
            (Some(_), Some(_)) => rep.edges_collapsed += 1,
            _ => rep.edges_dropped += 1,
        }
    }

    // Leaving out the screen episodes and her own comments cut the chain of «Precedes» that ran
    // through them (conversation → screen → conversation). The conversations that came across are
    // chained again, one after the next in time, so the chain keeps existing between the ones left.
    // Only the missing links are added: the ones that survived translated are not touched.
    if let Some(s) = s_conversaciones {
        let mut chain: Vec<u32> = new.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo && e.to == s).map(|e| e.from).collect();
        chain.sort_by(|&a, &b| new.nodes[a as usize].y.partial_cmp(&new.nodes[b as usize].y).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
        for w in chain.windows(2) {
            if new.find_edge(w[0], w[1], EdgeKind::Precedes).is_none() {
                let i = new.upsert_edge(w[0], w[1], EdgeKind::Precedes, 1.0, 1.0);
                new.edges[i].created_turn = new.nodes[w[1] as usize].created_turn;
                rep.precedes_added += 1;
            }
        }
    }

    // Scenes keep the members that came across, if two or more remain.
    for h in &old.hyperedges {
        let mut members: Vec<u32> = Vec::new();
        for m in h.members.iter().filter_map(|&m| map.get(m as usize).copied().flatten()) {
            if !members.contains(&m) {
                members.push(m);
            }
        }
        if members.len() >= 2 {
            let id = new.hyperedges.len() as u32;
            new.hyperedges.push(Hyperedge { id, label: h.label.clone(), y: h.y, members });
            rep.scenes_kept += 1;
        } else {
            rep.scenes_dropped += 1;
        }
    }

    // The machines she existed on, from the origins every memory signs.
    let mut machines: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, s) in old.sources.iter().enumerate() {
        if !(s.mac.is_empty() && s.host.is_empty()) {
            machines.entry((s.mac.clone(), s.host.clone())).or_default().push(i);
        }
    }
    for ((mac, host), srcs) in machines {
        let born: Vec<f64> = old.nodes.iter().filter(|n| srcs.contains(&(n.source as usize)) && n.born > 0.0).map(|n| n.born).collect();
        let (first, last) = (born.iter().copied().fold(f64::INFINITY, f64::min), born.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        let mut models: Vec<&str> = Vec::new();
        for &i in &srcs {
            let m = old.sources[i].model.as_str();
            if !m.is_empty() && !m.starts_with("semilla") && !models.contains(&m) {
                models.push(m);
            }
        }
        let name = if host.is_empty() { mac.clone() } else { host.clone() };
        let mut body = format!("Equipo «{name}»");
        if !mac.is_empty() {
            body.push_str(&format!(" (MAC {mac})"));
        }
        body.push_str(". ");
        if models.is_empty() {
            body.push_str("Sin modelos registrados. ");
        } else {
            body.push_str(&format!("Modelos con los que pensó en él: {}. ", models.join(", ")));
        }
        if born.is_empty() {
            body.push_str("Sin fechas registradas.");
        } else {
            body.push_str(&format!("Recuerdos escritos allí desde {} hasta {}.", date_of(first), date_of(last)));
        }
        let key = fresh_key(&taken, &format!("equipo-{:08x}", fnv(&format!("{mac}|{host}"))), 0);
        taken.insert(key.clone());
        let aliases: Vec<String> = [host.clone(), mac.clone()].into_iter().filter(|a| !a.is_empty()).collect();
        let (alpha, beta) = Origin::Inferred.prior();
        let id = new.add_node(Node {
            id: 0,
            kind: NodeKind::Object,
            origin: Origin::Inferred,
            flags: 0,
            period: 0,
            x: 0.0,
            y: crate::clock::year_of(if born.is_empty() { now } else { last }),
            alpha,
            beta,
            activation: 0.0,
            recalls: 0,
            created_turn: old.turn,
            key: key.clone(),
            label: name.clone(),
            text: body.clone(),
            aliases,
            module: COSAS,
            source: srcs[0] as u16,
            born: if born.is_empty() { now } else { first },
        });
        new.upsert_edge(id, s_equipos, EdgeKind::BelongsTo, FULL.0, FULL.1);
        rep.equipment.push(EquipmentReport { key, label: name, text: body });
    }

    // Meta: her vital state and settings travel; what describes the old structure does not.
    for (k, v) in &old.meta {
        let dropped = if k == "seed" {
            Some("dice de qué semilla vino la memoria vieja; la nueva lleva la suya")
        } else if k.starts_with("p.") {
            Some("persona, escena y saludo de la semilla vieja; los sustituye la semilla nueva")
        } else {
            None
        };
        match dropped {
            Some(why) => rep.meta_dropped.push((k.clone(), why)),
            None => {
                new.set_meta(k, v);
                rep.meta_kept.push(k.clone());
            }
        }
    }
    new.set_meta("importacion", &format!("{}", now as u64));
    new.turn = old.turn;

    // History: the same lines; what they recalled, in the new ids and only if it still exists.
    for c in &old.chat {
        let mut recalled = Vec::new();
        for &r in &c.recalled {
            match map.get(r as usize).copied().flatten() {
                Some(id) => {
                    rep.recalled_translated += 1;
                    if !recalled.contains(&id) {
                        recalled.push(id);
                    }
                }
                None => rep.recalled_dropped += 1,
            }
        }
        new.chat.push(ChatMessage { recalled, ..c.clone() });
    }

    rep.sectors = sector_paths(&new);
    Ok((new, rep))
}

/// (key, "Real → Personas") of every sector except the root, in the order of the tree.
fn sector_paths(m: &Memory) -> Vec<(String, String)> {
    let parent = |id: u32| m.edges.iter().find(|e| e.kind == EdgeKind::BelongsTo && e.from == id).map(|e| e.to);
    m.nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Sector)
        .filter(|n| parent(n.id).is_some())
        .map(|n| {
            let mut names = vec![n.label.clone()];
            let mut cur = n.id;
            while let Some(p) = parent(cur) {
                if parent(p).is_none() {
                    break;
                }
                names.push(m.nodes[p as usize].label.clone());
                cur = p;
            }
            names.reverse();
            (n.key.clone(), names.join(" → "))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cthg::binary;
    use crate::cthg::seed;
    use crate::cthg::sectors::strip_frame;
    use crate::modules::{self, Module, PERSONAS, YO};
    use std::path::{Path, PathBuf};

    const NOW: f64 = 1_793_000_000.0;
    const MAC_A: &str = "AA-BB-CC-DD-EE-01";
    const MAC_B: &str = "AA-BB-CC-DD-EE-02";

    /// The sectored seed the tests import onto: the Spanish base seed with «Ana» as its user
    /// (the personal seed is not compiled in; the node for the user is «usuario» here).
    fn guided_seed() -> Memory {
        seed::build_base("es", "Ana").unwrap()
    }

    const USER: &str = "usuario";

    /// `super::curated` on `guided_seed()`.
    fn curated(old: &Memory, now: f64) -> Result<(Memory, ImportReport), String> {
        super::curated(old, now, guided_seed())
    }

    /// The old memory as the first sectors-free seed builds it, plus a source table.
    fn old_memory() -> Memory {
        let mut m = seed::build_legacy().unwrap();
        let a = m.source_id(&Source { mac: MAC_A.into(), host: "SOBREMESA".into(), model: "gemma-4-12B".into() });
        for n in m.nodes.iter_mut() {
            n.source = a;
            n.born = 1_790_000_000.0;
            if n.y > TERRA {
                n.y = 2026.8;
            }
        }
        m.turn = 7;
        m.set_meta("usuario", "Ana");
        m.set_meta("seed", "kaltsit.ron");
        m.set_meta("p.persona", "persona vieja");
        m.set_meta("vital.h.at", "123");
        m
    }

    #[allow(clippy::too_many_arguments)] // a test builder: every argument is one field of the node
    fn add(m: &mut Memory, key: &str, kind: NodeKind, module: u8, label: &str, body: &str, alpha: f32, beta: f32) -> u32 {
        m.add_node(Node {
            id: 0,
            kind,
            origin: Origin::Learned,
            flags: 0,
            period: 0,
            x: 0.1,
            y: 2026.8,
            alpha,
            beta,
            activation: 0.4,
            recalls: 2,
            created_turn: 3,
            key: key.into(),
            label: label.into(),
            text: body.into(),
            aliases: vec![],
            module,
            source: 0,
            born: 1_791_000_000.0,
        })
    }

    fn fact(m: &mut Memory, key: &str, label: &str, body: &str) -> u32 {
        add(m, key, NodeKind::Fact, COSAS, label, body, 3.0, 1.0)
    }

    fn parents(m: &Memory, id: u32) -> Vec<String> {
        m.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo && e.from == id).map(|e| m.nodes[e.to as usize].key.clone()).collect()
    }

    fn find<'a>(rep: &'a ImportReport, key: &str) -> &'a NodeReport {
        rep.nodes.iter().find(|n| n.key == key).unwrap_or_else(|| panic!("falta {key}"))
    }

    fn sector_of(new: &Memory, rep: &ImportReport, key: &str) -> Vec<String> {
        parents(new, find(rep, key).new_id.unwrap_or_else(|| panic!("{key} no se importó")))
    }

    #[test]
    fn old_core_is_not_imported_and_the_new_one_is_complete() {
        let old = old_memory();
        let (new, rep) = curated(&old, NOW).unwrap();
        let guided = guided_seed();
        // Every seed node is there with the seed's exact text.
        for g in &guided.nodes {
            let id = new.by_key(&g.key).unwrap_or_else(|| panic!("falta {}", g.key));
            assert_eq!(new.nodes[id as usize].text, g.text, "{}", g.key);
            assert_eq!(new.nodes[id as usize].flags, g.flags, "{}", g.key);
        }
        // The old core nodes are listed as discarded, and none of their text came across.
        let core: Vec<&Node> = old.nodes.iter().filter(|n| n.has(flags::CORE)).collect();
        assert!(core.len() >= 8);
        for c in &core {
            assert_eq!(find(&rep, &c.key).decision, Decision::Discarded(Reason::OldCore), "{}", c.key);
            // (The Terra canon is also in the seed, with the same text; only her own core must not come twice.)
            if c.y >= TERRA {
                assert!(new.nodes.iter().all(|n| n.text != c.text), "{} se coló", c.key);
            }
        }
        assert_eq!(new.nodes.iter().filter(|n| n.has(flags::CORE)).count(), guided.nodes.iter().filter(|n| n.has(flags::CORE)).count());
        assert_eq!(new.meta("seed"), Some("base_es.ron"));
    }

    #[test]
    fn terra_canon_and_screen_episodes_are_left_out_and_counted() {
        // (Before the third pass this test also expected «Ana dijo hola» to be discarded: now it is a conversation, see below.)
        let mut old = old_memory();
        let ep = add(&mut old, "ep-1", NodeKind::Episode, COSAS, "Vio la pantalla", "Vio: un editor", 3.0, 1.0);
        add(&mut old, "ep-2", NodeKind::Episode, COSAS, "(mira la pantalla)", "(mira la pantalla)\n— Kal'tsit: Cierra eso.", 3.0, 1.0);
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(rep.discarded(Reason::ScreenEpisode).count(), 2);
        assert_eq!(rep.discarded(Reason::Episode).count(), 0);
        assert!(new.by_key("ep-1").is_none() && new.by_key("ep-2").is_none());
        assert!(new.by_key(CONVERSACIONES).is_none(), "nothing to put in it");
        let terra: Vec<&Node> = old.nodes.iter().filter(|n| n.y < TERRA && !n.has(flags::CORE)).collect();
        assert!(terra.len() > 10);
        for t in &terra {
            assert_eq!(find(&rep, &t.key).decision, Decision::Discarded(Reason::TerraCanon), "{}", t.key);
        }
        assert_eq!(rep.discarded(Reason::TerraCanon).count(), terra.len());
        let _ = ep;
        // The canon in the new memory is the seed's own: the 13 nodes of the base seed under
        // Arknights (24 in the owner's personal one), none doubled.
        let ark = new.by_key("arknights").unwrap();
        let under = new.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo && e.to == ark).count();
        assert_eq!(under, 13);
    }

    #[test]
    fn the_user_is_merged_into_the_seed_node_keeping_the_guided_text_and_the_richer_evidence() {
        let mut old = old_memory();
        let creador = old.by_key("creador").unwrap();
        {
            let c = &mut old.nodes[creador as usize];
            c.alpha = 42.0;
            c.beta = 2.0;
            c.activation = 0.9;
            c.recalls = 55;
            c.aliases.push("Lucho".into());
        }
        // A second node that is also just «Ana», learned later.
        add(&mut old, "personas-x-1", NodeKind::Person, PERSONAS, "Ana", "el usuario", 4.0, 1.0);
        let (new, rep) = curated(&old, NOW).unwrap();
        let guided = guided_seed();
        let ana: Vec<&Node> = new.nodes.iter().filter(|n| n.key == USER).collect();
        assert_eq!(ana.len(), 1);
        let l = ana[0];
        assert_eq!(l.text, guided.nodes[guided.by_key(USER).unwrap() as usize].text);
        assert_eq!((l.alpha, l.beta), (42.0, 2.0));
        assert!((l.activation - 0.9).abs() < 1e-6 && l.recalls == 55);
        assert!(l.aliases.contains(&"Ana".to_string()) && l.aliases.contains(&"Lucho".to_string()));
        assert_eq!(l.aliases.iter().filter(|a| text::fold(a) == "ana").count(), 1);
        assert_eq!(parents(&new, l.id), ["personas"]);
        assert_eq!(rep.merged().count(), 2);
        assert_eq!(find(&rep, "creador").decision, Decision::Merged(USER.into()));
        assert_eq!(find(&rep, "personas-x-1").new_id, Some(l.id));
        // No other node is called Ana: the seed's node for him is the only one (its label is his name).
        assert_eq!(new.nodes.iter().filter(|n| n.kind == NodeKind::Person && text::fold(&n.label).trim() == "ana").count(), 1);
    }

    #[test]
    fn the_primary_user_node_is_found_in_every_seed() {
        for lang in ["es", "en"] {
            assert_eq!(user_key(&seed::build_base(lang, "Marta").unwrap()), "usuario");
        }
        // The owner's personal seed names the node in its own key (skipped without the file).
        if let Ok(m) = seed::build_personal() {
            let k = user_key(&m);
            assert_ne!(k, "usuario");
            let n = &m.nodes[m.by_key(&k).expect("the key names a node") as usize];
            assert_eq!(n.kind, NodeKind::Person);
            assert!(!n.aliases.is_empty(), "its first alias is the user's name");
        }
    }

    #[test]
    fn people_of_her_life_go_to_personas() {
        let mut old = old_memory();
        add(&mut old, "personas-x-2", NodeKind::Person, PERSONAS, "Eva", "amiga de Ana", 2.0, 1.0);
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(sector_of(&new, &rep, "personas-x-2"), ["personas"]);
    }

    #[test]
    fn facts_are_sorted_conservatively_and_negative_chi_is_dropped() {
        let mut old = old_memory();
        fact(&mut old, "f-tec", "Ana usa Rust", "Ana programa en Rust y corre un modelo GGUF en la GPU.");
        fact(&mut old, "f-amb", "Minecraft en el servidor", "Ana juega a Minecraft en un servidor de la ciudad.");
        fact(&mut old, "f-none", "Le gusta el café", "A Ana le gusta tomar café por la mañana.");
        fact(&mut old, "f-juego", "Partida larga", "Jugaron una partida de Minecraft hasta tarde.");
        add(&mut old, "f-neg", NodeKind::Fact, COSAS, "Falso", "Ana usa un programa en Rust", 1.0, 20.0);
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(sector_of(&new, &rep, "f-tec"), ["tecnologia"]);
        assert_eq!(sector_of(&new, &rep, "f-juego"), ["juegos"]);
        // Games and a place (and technology): ambiguous, so it waits.
        assert_eq!(sector_of(&new, &rep, "f-amb"), ["por_clasificar"]);
        assert!(find(&rep, "f-amb").why.starts_with("ambiguo"), "{}", find(&rep, "f-amb").why);
        // «le gusta» is a preference word, but it is Ana's (rule D): Personas, not her Preferencias.
        // (Changed by the second pass: the first one sent it to «preferencias».)
        assert_eq!(sector_of(&new, &rep, "f-none"), ["personas"]);
        // χ < 0: left out, and listed.
        assert!(find(&rep, "f-neg").chi < 0.0);
        assert_eq!(find(&rep, "f-neg").decision, Decision::Discarded(Reason::NegativeChi));
        assert!(new.by_key("f-neg").is_none());
        // Nothing matching at all goes to «Por clasificar» too.
        let (s, why) = classify(NodeKind::Fact, "Algo", "una cosa cualquiera sin más");
        assert_eq!(s, POR_CLASIFICAR);
        assert!(why.contains("sin palabras"));
        let (s, _) = classify(NodeKind::Place, "El parque", "un sitio tranquilo");
        assert_ne!(s, "lugares", "tranquilo (calma) y lugar chocan: ambiguo");
        assert_eq!(classify(NodeKind::Place, "El parque", "un sitio verde").0, "lugares");
    }

    /// The shared rule, as (sector key, why): what these tests always compared.
    fn classify(kind: NodeKind, label: &str, body: &str) -> (&'static str, String) {
        let p = sectors::classify(kind, label, body, "Ana");
        (p.key, p.why)
    }

    fn sector(label: &str) -> (&'static str, String) {
        classify(NodeKind::Fact, label, label)
    }

    /// Phrases that cover every branch of the sorting rule, with (kind, label, body).
    const CORPUS: &[(NodeKind, &str, &str)] = &[
        (NodeKind::Fact, "Ana afirma que GGUF es un tipo de archivo", "Ana afirma que GGUF es un tipo de archivo"),
        (NodeKind::Fact, "Ana programa en Rust", "Ana programa en Rust y corre un modelo GGUF en la GPU."),
        (NodeKind::Fact, "Minecraft en el servidor", "Ana juega a Minecraft en un servidor de la ciudad."),
        (NodeKind::Fact, "Le gusta el café", "A Ana le gusta tomar café por la mañana."),
        (NodeKind::Fact, "Partida larga", "Jugaron una partida de Minecraft hasta tarde."),
        (NodeKind::Fact, "Algo", "una cosa cualquiera sin más"),
        (NodeKind::Place, "El parque", "un sitio tranquilo"),
        (NodeKind::Place, "El parque", "un sitio verde"),
        (NodeKind::Fact, "Ana pide a Kal'tsit que baile", "Ana pide a Kal'tsit que baile"),
        (NodeKind::Fact, "Ana va a buscar comida", "Ana va a buscar comida"),
        (NodeKind::Fact, "Ana indica que está tomando decisiones", "Ana indica que está tomando decisiones sobre su vida."),
        (NodeKind::Fact, "Ana declara que ya logró el cometido", "Ana declara que ya logró el cometido original."),
        (NodeKind::Fact, "Kal'tsit muestra cierta duda", "Kal'tsit muestra cierta duda"),
        (NodeKind::Fact, "Ana percibe", "Ana percibe que Kal'tsit muestra cierta duda."),
        (NodeKind::Fact, "Kal'tsit es libre", "Kal'tsit es una entidad semi libre"),
        (NodeKind::Fact, "Kal'tsit entiende", "Kal'tsit entiende que la mejor solución es no actuar directamente"),
        (NodeKind::Fact, "Ana afirma que el contexto", "Ana afirma que el contexto se maneja más rápido"),
        (NodeKind::Fact, "Ana pregunta si", "Ana pregunta si Kal'tsit tiene alguna molestia"),
        (NodeKind::Fact, "La memoria de Kal'tsit", "La memoria de Kal'tsit es la prueba"),
        (NodeKind::Fact, "el refugio nuevo", "el refugio nuevo"),
        (NodeKind::Fact, "El camino es largo", "El camino es largo"),
        (NodeKind::Fact, "Ana prefiere el café", "Ana prefiere el café por la mañana"),
        (NodeKind::Fact, "Ana afirma que se prefiere", "Ana afirma que se prefiere el mejor resultado con menor gasto."),
        (NodeKind::Fact, "Kal'tsit prefiere el silencio", "Kal'tsit prefiere el silencio de la noche"),
        (NodeKind::Fact, "Le encanta el café", "Le encanta el café"),
        (NodeKind::Fact, "Ana prefiere Rust", "Ana prefiere Rust"),
        (NodeKind::Fact, "El teorema original", "El teorema original"),
        (NodeKind::Fact, "Rechazar una premisa es moral", "Rechazar una premisa es moral"),
        (NodeKind::Fact, "learn.rs", "learn.rs"),
        (NodeKind::Fact, "Cambió main.rs", "Cambió main.rs, y también config.toml."),
        (NodeKind::Fact, "Hay que talar madera", "Hay que talar madera y juntar troncos"),
        (NodeKind::Fact, "La arquitectura del flujo", "La arquitectura del flujo"),
        (NodeKind::Fact, "Ana sube el commit", "Ana sube el commit con el hook"),
        (NodeKind::Fact, "La estabilidad del sistema", "La estabilidad del sistema depende del fallback"),
        (NodeKind::Fact, "La arquitectura del programa", "La arquitectura del programa"),
        (NodeKind::Fact, "El principio de inmutabilidad", "El principio de inmutabilidad en Minecraft"),
        (NodeKind::Fact, "Una fórmula de química", "Una fórmula de química"),
        (NodeKind::Fact, "Ana siente cariño", "Ana siente cariño por Kal'tsit"),
        (NodeKind::Fact, "Hubo una discusión", "Hubo una discusión y mucho enfado"),
        (NodeKind::Fact, "Calma en la noche", "Una noche de calma y curiosidad"),
        (NodeKind::Fact, "Nació como asistente", "Empezaste como asistente en su origen"),
        (NodeKind::Fact, "Kal'tsit cree que el principio es válido", "Kal'tsit cree que el principio es válido"),
        (NodeKind::Place, "Casa", "donde vive Ana en una ciudad"),
        (NodeKind::Fact, "Ana desea un portátil", "Ana desea un portátil con GPU"),
        (NodeKind::Object, "Rustacean", "un lenguaje, el compilador de Rust"),
    ];

    /// What the sorting rule answered for `CORPUS` when it still lived in this file (phase 3),
    /// copied before moving it to `cthg::sectors`: (sector key, why). Moving it must not change
    /// a word of it.
    const BEFORE_THE_MOVE: &[(&str, &str)] = &[
        ("tecnologia", "tecnologia: gguf, archivo (marco «Ana afirma» quitado)"),
        ("tecnologia", "tecnologia: programa, modelo, gguf, gpu, rust"),
        ("por_clasificar", "ambiguo: tecnologia [servidor] / juegos [minecraft, juega] / lugares [ciudad]"),
        ("personas", "sujeto Ana, su preferencia"),
        ("juegos", "juegos: minecraft, partida"),
        ("por_clasificar", "sin palabras clave de ningún sector ni sujeto claro"),
        ("por_clasificar", "ambiguo: lugares [(es un lugar)] / calma [tranquilo]"),
        ("lugares", "lugares: (es un lugar)"),
        ("por_clasificar", "sin palabras clave de ningún sector ni sujeto claro (marco «Ana pide» quitado)"),
        ("personas", "sujeto Ana"),
        ("personas", "sujeto Ana (marco «Ana indica» quitado)"),
        ("personas", "sujeto Ana (marco «Ana declara» quitado)"),
        ("experiencias", "sujeto Kal'tsit (marco «Kal'tsit muestra» quitado)"),
        ("personas", "sujeto Ana (marco «Ana percibe» quitado)"),
        ("experiencias", "sujeto Kal'tsit"),
        ("opiniones", "sujeto Kal'tsit con verbo cognitivo"),
        ("por_clasificar", "sin palabras clave de ningún sector ni sujeto claro (marco «Ana afirma» quitado)"),
        ("personas", "sujeto Ana (marco «Ana pregunta» quitado)"),
        ("por_clasificar", "sin palabras clave de ningún sector ni sujeto claro"),
        ("juegos", "juegos: refugio"),
        ("por_clasificar", "sin palabras clave de ningún sector ni sujeto claro"),
        ("personas", "sujeto Ana, su preferencia"),
        ("personas", "sujeto Ana, su preferencia (marco «Ana afirma» quitado)"),
        ("preferencias", "preferencia de Kal'tsit"),
        ("por_clasificar", "preferencia sin sujeto claro"),
        ("tecnologia", "tecnologia: rust"),
        ("ideas", "ideas: teorema"),
        ("etica", "etica: moral, moralmente, premisa"),
        ("tecnologia", "tecnologia: (learn.rs)"),
        ("tecnologia", "tecnologia: (main.rs)"),
        ("juegos", "juegos: talar, tronco, madera"),
        ("sistemas", "sistemas: arquitectura, flujo"),
        ("tecnologia", "tecnologia: commit, hook"),
        ("tecnologia", "tecnologia: fallback"),
        ("por_clasificar", "ambiguo: tecnologia [programa] / sistemas [arquitectura]"),
        ("por_clasificar", "ambiguo: juegos [minecraft] / etica [principio]"),
        ("conocimiento", "conocimiento: quimica, formula"),
        ("vinculo", "vinculo: carino"),
        ("conflicto", "conflicto: enfado, discusion"),
        ("calma", "calma: calma, curiosidad"),
        ("origen", "origen: origen, nacio, asistente, empezaste"),
        ("etica", "etica: principio (marco «Kal'tsit cree» quitado)"),
        ("lugares", "lugares: ciudad, (es un lugar)"),
        ("tecnologia", "tecnologia: gpu, portatil (marco «Ana desea» quitado)"),
        ("tecnologia", "tecnologia: rust"),
    ];

    #[test]
    fn the_shared_classifier_answers_exactly_what_the_import_answered_before_the_move() {
        assert_eq!(CORPUS.len(), BEFORE_THE_MOVE.len());
        for (&(k, l, b), &(key, why)) in CORPUS.iter().zip(BEFORE_THE_MOVE) {
            let p = sectors::classify(k, l, b, "Ana");
            assert_eq!((p.key, p.why.as_str()), (key, why), "{l} / {b}");
            // `ambiguous` is only ever filled when the answer is «ambiguo».
            assert_eq!(!p.ambiguous.is_empty(), why.starts_with("ambiguo"), "{why}");
        }
        // And the import reaches the same sector through it: a fact of the corpus lands where it says.
        let mut old = old_memory();
        fact(&mut old, "f-c0", CORPUS[0].1, CORPUS[0].2);
        fact(&mut old, "f-c4", CORPUS[4].1, CORPUS[4].2);
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(sector_of(&new, &rep, "f-c0"), ["tecnologia"]);
        assert_eq!(sector_of(&new, &rep, "f-c4"), ["juegos"]);
        assert_eq!(find(&rep, "f-c0").why, BEFORE_THE_MOVE[0].1);
    }

    #[test]
    fn the_reporting_frame_is_taken_off_before_sorting() {
        // Rule A + B: the topic of X decides, not «Ana afirma que».
        let (s, why) = sector("Ana afirma que GGUF es un tipo de archivo");
        assert_eq!(s, "tecnologia", "{why}");
        assert!(why.contains("marco"), "{why}");
        assert_eq!(sector("Ana pide a Kal'tsit que baile").0, POR_CLASIFICAR);
        // The frame is only «<name> <verb>»: a name inside the sentence is not one.
        let framed = strip_frame("Ana pide a Kal'tsit que cree una realidad", "Ana");
        assert_eq!(framed.x, "cree una realidad");
        assert_eq!(strip_frame("Ana pregunta si Kal'tsit puede jugar", "Ana").x, "Kal'tsit puede jugar");
        assert_eq!(strip_frame("Ana programa en Rust", "Ana").x, "Ana programa en Rust");
    }

    #[test]
    fn without_a_topic_the_subject_decides() {
        // Rule C: Ana.
        assert_eq!(sector("Ana va a buscar comida").0, "personas");
        assert_eq!(sector("Ana indica que está tomando decisiones sobre su vida.").0, "personas");
        assert_eq!(sector("Ana afirma que él se encuentra bien, eficiente y trabajador.").0, "personas");
        assert_eq!(sector("Ana declara que ya logró el cometido original.").0, "personas");
        // Rule C: Kal'tsit, as an experience, or an opinion with a thinking verb.
        assert_eq!(sector("Kal'tsit muestra cierta duda").0, "experiencias");
        assert_eq!(sector("Ana percibe que Kal'tsit muestra cierta duda.").0, "experiencias");
        assert_eq!(sector("Kal'tsit es una entidad semi libre").0, "experiencias");
        assert_eq!(sector("Kal'tsit entiende que la mejor solución es no actuar directamente").0, "opiniones");
        // Strict: Ana stating something about the world says nothing of his own subject; a question or an order neither.
        assert_eq!(sector("Ana afirma que el contexto se maneja más rápido").0, POR_CLASIFICAR);
        assert_eq!(sector("Ana pregunta si Kal'tsit tiene alguna molestia").0, POR_CLASIFICAR);
        assert_eq!(sector("La memoria de Kal'tsit es la prueba").0, POR_CLASIFICAR);
        assert_eq!(sector("el refugio nuevo").0, "juegos");
        assert_eq!(sector("El camino es largo").0, POR_CLASIFICAR, "«El» is the article, not «él»");
    }

    #[test]
    fn preferences_are_hers_or_they_are_the_users() {
        // Rule D.
        let (s, why) = sector("Ana prefiere el café por la mañana");
        assert_eq!(s, "personas", "{why}");
        assert_eq!(sector("Ana afirma que se prefiere el mejor resultado con menor gasto.").0, "personas");
        let (s, why) = sector("Kal'tsit prefiere el silencio de la noche");
        assert_eq!(s, "preferencias", "{why}");
        assert_eq!(sector("Le encanta el café").0, POR_CLASIFICAR, "a preference with no subject waits");
        // A topic still goes first (rule B): the preference is a hint, not a topic.
        assert_eq!(sector("Ana prefiere Rust").0, "tecnologia");
    }

    #[test]
    fn the_new_vocabulary_reaches_the_abstract_and_game_sectors() {
        assert_eq!(sector("El teorema original").0, "ideas");
        assert_eq!(sector("Rechazar una premisa es moral").0, "etica", "rechazo (conflicto) does not match «rechazar»");
        assert_eq!(sector("learn.rs").0, "tecnologia");
        assert_eq!(sector("Cambió main.rs, y también config.toml.").0, "tecnologia");
        assert_eq!(sector("Hay que talar madera y juntar troncos").0, "juegos");
        assert_eq!(sector("La arquitectura del flujo").0, "sistemas");
        assert_eq!(sector("Ana sube el commit con el hook").0, "tecnologia");
        // Several sectors: no tie-break, it waits. (Technology and systems; games and ethics.)
        assert_eq!(sector("La estabilidad del sistema depende del fallback").0, "tecnologia", "«sistema» is not a word of any sector");
        let (s, why) = sector("La arquitectura del programa");
        assert_eq!(s, POR_CLASIFICAR);
        assert!(why.starts_with("ambiguo"), "{why}");
        assert_eq!(sector("El principio de inmutabilidad en Minecraft").0, POR_CLASIFICAR);
        // Conocimiento keeps concrete science only.
        assert_eq!(sector("Una fórmula de química").0, "conocimiento");
    }

    #[test]
    fn every_keyword_survives_the_tokenizer() {
        for (sector, words) in KEYWORDS {
            for w in words {
                if !w.contains(' ') {
                    assert_eq!(text::tokens(w).len(), 1, "{sector}: «{w}» se pierde al tokenizar");
                }
            }
        }
    }

    #[test]
    fn suspensions_go_to_marked_experiences_and_her_knowledge_to_opinions() {
        let mut old = old_memory();
        let s1 = add(&mut old, "ev-1", NodeKind::Event, YO, "Suspensión crítica por paradoja (Ω = 0.85)", "Entró en CriticalSuspension ante la orden: «x».", 4.0, 1.0);
        add(&mut old, "ev-2", NodeKind::Event, YO, "Salida de suspensión crítica (Ω = 0.10)", "Salió de CriticalSuspension.", 4.0, 1.0);
        // Even a mediocre χ does not keep a suspension out.
        old.nodes[s1 as usize].alpha = 1.0;
        old.nodes[s1 as usize].beta = 3.0;
        add(&mut old, "sab-1", NodeKind::Fact, SABER, "Rust es estable", "Concluyó que Rust le da estabilidad al programa.", 3.0, 1.0);
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(sector_of(&new, &rep, "ev-1"), ["marcadas"]);
        assert_eq!(sector_of(&new, &rep, "ev-2"), ["marcadas"]);
        // Her knowledge is an opinion even if it mentions technology.
        assert_eq!(sector_of(&new, &rep, "sab-1"), ["opiniones"]);
        assert_eq!(find(&rep, "sab-1").new_id.map(|i| new.nodes[i as usize].module), Some(SABER));
    }

    #[test]
    fn edges_between_imported_nodes_are_kept_translated_and_the_rest_vanish() {
        let mut old = old_memory();
        let a = fact(&mut old, "f-a", "Usa Rust", "Ana programa en Rust.");
        let b = fact(&mut old, "f-b", "Corre un modelo", "Un modelo GGUF en la GPU.");
        let neg = add(&mut old, "f-n", NodeKind::Fact, COSAS, "Falso", "x", 1.0, 20.0);
        // A screen episode: still discarded, so its edges still vanish (before the third pass any episode was).
        let ep = add(&mut old, "ep-9", NodeKind::Episode, COSAS, "Vio la pantalla", "Vio: un editor", 3.0, 1.0);
        let creador = old.by_key("creador").unwrap();
        let rhodes = old.by_key("rhodes").unwrap();
        let i = old.upsert_edge(a, b, EdgeKind::Relates, 1.0, 1.0);
        old.edges[i].hebb = 0.6;
        old.upsert_edge(a, neg, EdgeKind::Relates, 1.0, 1.0);
        old.upsert_edge(ep, a, EdgeKind::Mentions, 1.0, 1.0);
        old.upsert_edge(a, rhodes, EdgeKind::Mentions, 1.0, 1.0);
        old.upsert_edge(a, creador, EdgeKind::Mentions, 1.0, 1.0);
        let (new, rep) = curated(&old, NOW).unwrap();
        let (na, nb) = (find(&rep, "f-a").new_id.unwrap(), find(&rep, "f-b").new_id.unwrap());
        let e = &new.edges[new.find_edge(na, nb, EdgeKind::Relates).expect("la arista se conserva")];
        assert!((e.hebb - 0.6).abs() < 1e-6);
        // The edge to Ana goes to the seed's «ana».
        assert!(new.find_edge(na, new.by_key(USER).unwrap(), EdgeKind::Mentions).is_some());
        // The ones that touch a discarded node do not exist.
        assert!(new.edges.iter().all(|e| (e.from as usize) < new.nodes.len() && (e.to as usize) < new.nodes.len()));
        assert_eq!(new.edges.iter().filter(|e| e.from == na && e.kind != EdgeKind::BelongsTo).count(), 2, "Relates→b y Mentions→ana; nada más");
        assert_eq!(rep.edges_old, old.edges.len());
        assert!(rep.edges_dropped >= 3);
        assert!(rep.edges_kept >= 2);
        assert_eq!(rep.edges_kept + rep.edges_dropped + rep.edges_collapsed, old.edges.len());
    }

    #[test]
    fn recalled_ids_are_translated_and_none_dangles() {
        let mut old = old_memory();
        let a = fact(&mut old, "f-a", "Usa Rust", "Ana programa en Rust.");
        let neg = add(&mut old, "f-n", NodeKind::Fact, COSAS, "Falso", "x", 1.0, 20.0);
        // A screen episode: still discarded, so the quote that points at it is still dropped.
        let ep = add(&mut old, "ep-9", NodeKind::Episode, COSAS, "Vio la pantalla", "Vio: un editor", 3.0, 1.0);
        let creador = old.by_key("creador").unwrap();
        let yo = old.by_key("yo").unwrap();
        old.chat.push(ChatMessage { period: 0, role: Role::User, turn: 1, text: "hola".into(), recalled: vec![], at: 1.0 });
        old.chat.push(ChatMessage { period: 0, role: Role::Assistant, turn: 1, text: "¿Qué hay?".into(), recalled: vec![a, neg, ep, creador, yo, a], at: 2.0 });
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(new.chat.len(), 2);
        assert_eq!(new.chat[1].text, "¿Qué hay?");
        assert_eq!(new.chat[1].recalled, vec![find(&rep, "f-a").new_id.unwrap(), new.by_key(USER).unwrap()]);
        for c in &new.chat {
            assert!(c.recalled.iter().all(|&r| (r as usize) < new.nodes.len()));
        }
        assert_eq!(rep.recalled_translated, 3);
        assert_eq!(rep.recalled_dropped, 3);
    }

    fn episode(m: &mut Memory, key: &str, label: &str, body: &str) -> u32 {
        add(m, key, NodeKind::Episode, COSAS, label, body, 3.0, 1.0)
    }

    #[test]
    fn episode_kind_follows_how_the_code_writes_them() {
        // Mind::record_seen
        assert_eq!(episode_kind("Vio la pantalla"), EpisodeKind::Screen);
        // life.rs `look`: a comment of her own after looking
        assert_eq!(episode_kind("(mira la pantalla)"), EpisodeKind::Screen);
        // life.rs: she speaks up after deleting what she was writing
        assert_eq!(episode_kind("(guarda silencio tras borrar lo que escribía)"), EpisodeKind::Own);
        assert_eq!(episode_kind("(prueba)"), EpisodeKind::Own);
        // learn::record_turn: the label is what Ana typed
        assert_eq!(episode_kind("Hey kal"), EpisodeKind::Conversation);
        assert_eq!(episode_kind("¿Qué ves en la pantalla?"), EpisodeKind::Conversation, "a message that talks about the screen is still his");
        assert_eq!(episode_kind("(jaja) vale"), EpisodeKind::Conversation, "only a label that is entirely a parenthesis is a placeholder");
    }

    #[test]
    fn a_conversation_episode_is_imported_into_conversaciones_under_experiencias() {
        let mut old = old_memory();
        let ep = episode(&mut old, "ep-c", "Hola Kal", "Hola Kal\n— Kal'tsit: Dime.");
        {
            let n = &mut old.nodes[ep as usize];
            n.alpha = 4.5;
            n.beta = 0.5;
            n.activation = 0.7;
            n.recalls = 5;
            n.born = 1_791_234_567.0;
            n.y = 2026.85;
        }
        let (new, rep) = curated(&old, NOW).unwrap();
        let guided = guided_seed();
        assert!(guided.by_key(CONVERSACIONES).is_none(), "the seed is untouched: the import makes the sector");
        let s = new.by_key(CONVERSACIONES).expect("el sector existe");
        assert_eq!(new.nodes[s as usize].kind, NodeKind::Sector);
        assert_eq!(new.nodes[s as usize].label, "Conversaciones");
        assert_eq!(parents(&new, s), ["experiencias"]);
        let e = &new.edges[new.find_edge(s, new.by_key("experiencias").unwrap(), EdgeKind::BelongsTo).unwrap()];
        assert_eq!(e.alpha / (e.alpha + e.beta), 1.0);
        // The episode is in it with everything it had.
        assert_eq!(sector_of(&new, &rep, "ep-c"), [CONVERSACIONES]);
        let n = &new.nodes[find(&rep, "ep-c").new_id.unwrap() as usize];
        assert_eq!((n.kind, n.label.as_str(), n.text.as_str()), (NodeKind::Episode, "Hola Kal", "Hola Kal\n— Kal'tsit: Dime."));
        assert_eq!((n.alpha, n.beta, n.activation, n.recalls, n.born, n.y), (4.5, 0.5, 0.7, 5, 1_791_234_567.0, 2026.85));
        let be = &new.edges[new.find_edge(n.id, s, EdgeKind::BelongsTo).unwrap()];
        assert_eq!(be.alpha / (be.alpha + be.beta), 1.0);
        assert_eq!(rep.by_sector().iter().find(|(k, _)| k == CONVERSACIONES).map(|(_, c)| *c), Some(1));
        assert_eq!(rep.path_of(CONVERSACIONES), "Mi mundo → Experiencias → Conversaciones");
        assert_eq!(rep.discarded(Reason::Episode).count() + rep.discarded(Reason::ScreenEpisode).count(), 0);
    }

    #[test]
    fn screen_episodes_and_her_own_comments_are_discarded_with_their_reason() {
        let mut old = old_memory();
        episode(&mut old, "ep-s1", "Vio la pantalla", "Vio: Ana programa en Rust");
        episode(&mut old, "ep-s2", "(mira la pantalla)", "(mira la pantalla)\n— Kal'tsit: Ese error es tuyo.");
        episode(&mut old, "ep-o1", "(guarda silencio tras borrar lo que escribía)", "(guarda silencio tras borrar lo que escribía)\n— Kal'tsit: …");
        episode(&mut old, "ep-c1", "¿Cómo va el proyecto?", "¿Cómo va el proyecto?\n— Kal'tsit: Avanza.");
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(find(&rep, "ep-s1").decision, Decision::Discarded(Reason::ScreenEpisode));
        assert_eq!(find(&rep, "ep-s2").decision, Decision::Discarded(Reason::ScreenEpisode));
        assert_eq!(find(&rep, "ep-o1").decision, Decision::Discarded(Reason::Episode));
        assert_eq!(find(&rep, "ep-c1").decision, Decision::Imported(CONVERSACIONES.into()));
        assert!(["ep-s1", "ep-s2", "ep-o1"].iter().all(|k| new.by_key(k).is_none()));
        assert_eq!(Reason::ScreenEpisode.name(), "episodio de pantalla");
        let md = rep.to_markdown(NOW);
        assert!(md.contains("episodio de pantalla (2)"), "{md}");
    }

    #[test]
    fn conversaciones_exists_only_when_there_is_something_to_put_in_it() {
        // No episodes at all.
        let old = old_memory();
        let (new, _) = curated(&old, NOW).unwrap();
        assert!(new.by_key(CONVERSACIONES).is_none());
        // Only screen episodes and her own comments.
        let mut old = old_memory();
        episode(&mut old, "ep-s", "Vio la pantalla", "Vio: algo");
        episode(&mut old, "ep-o", "(guarda silencio tras borrar lo que escribía)", "x");
        let (new, rep) = curated(&old, NOW).unwrap();
        assert!(new.by_key(CONVERSACIONES).is_none());
        assert!(rep.sectors.iter().all(|(k, _)| k != CONVERSACIONES));
        assert_eq!(new.nodes.iter().filter(|n| n.kind == NodeKind::Sector).count(), 1 + 5 + 20, "the seed's sectors, no more");
        // Two conversations: one sector, not two.
        let mut old = old_memory();
        episode(&mut old, "ep-1", "Hola", "Hola\n— Kal'tsit: Dime.");
        episode(&mut old, "ep-2", "Adiós", "Adiós\n— Kal'tsit: Cuídate.");
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(new.nodes.iter().filter(|n| n.key == CONVERSACIONES).count(), 1);
        assert_eq!(new.nodes.iter().filter(|n| n.kind == NodeKind::Sector).count(), 1 + 5 + 20 + 1);
        assert_eq!(rep.by_sector().iter().find(|(k, _)| k == CONVERSACIONES).map(|(_, c)| *c), Some(2));
        // Its words are not a topic: the sorting rule never sends anything to it.
        assert!(KEYWORDS.iter().all(|(k, _)| *k != CONVERSACIONES) && !SUBJECT_SECTORS.contains(&CONVERSACIONES));
    }

    #[test]
    fn edges_from_an_imported_episode_to_a_fact_survive_translated() {
        let mut old = old_memory();
        let fact_id = fact(&mut old, "f-a", "Usa Rust", "Ana programa en Rust.");
        let c1 = episode(&mut old, "ep-1", "Programo en Rust", "Programo en Rust\n— Kal'tsit: Bien.");
        let c2 = episode(&mut old, "ep-2", "Y hoy compilé", "Y hoy compilé\n— Kal'tsit: Sigue.");
        let scr = episode(&mut old, "ep-s", "Vio la pantalla", "Vio: un editor");
        let creador = old.by_key("creador").unwrap();
        old.upsert_edge(c1, fact_id, EdgeKind::Mentions, 1.0, 1.0);
        old.upsert_edge(c1, c2, EdgeKind::Precedes, 1.0, 1.0);
        old.upsert_edge(c2, creador, EdgeKind::Mentions, 1.0, 1.0);
        old.upsert_edge(scr, c1, EdgeKind::Precedes, 1.0, 1.0);
        old.upsert_edge(c2, scr, EdgeKind::Precedes, 1.0, 1.0);
        let (new, rep) = curated(&old, NOW).unwrap();
        let (nf, n1, n2) = (find(&rep, "f-a").new_id.unwrap(), find(&rep, "ep-1").new_id.unwrap(), find(&rep, "ep-2").new_id.unwrap());
        assert!(new.find_edge(n1, nf, EdgeKind::Mentions).is_some(), "episodio → hecho");
        assert!(new.find_edge(n1, n2, EdgeKind::Precedes).is_some(), "episodio → episodio");
        assert!(new.find_edge(n2, new.by_key(USER).unwrap(), EdgeKind::Mentions).is_some(), "episodio → Ana (fusionado)");
        // The ones that touch the screen episode do not exist.
        assert_eq!(new.edges.iter().filter(|e| e.kind == EdgeKind::Precedes).count(), 1);
        assert_eq!(rep.edges_kept + rep.edges_dropped + rep.edges_collapsed, old.edges.len());
        assert!(rep.edges_dropped >= 2);
        assert!(new.edges.iter().all(|e| (e.from as usize) < new.nodes.len() && (e.to as usize) < new.nodes.len()));
    }

    #[test]
    fn the_precedes_chain_is_rebuilt_between_the_conversations_that_remain() {
        let mut old = old_memory();
        let c1 = episode(&mut old, "ep-1", "Hola", "Hola\n— Kal'tsit: Dime.");
        let s1 = episode(&mut old, "ep-s1", "Vio la pantalla", "Vio: un editor");
        let c2 = episode(&mut old, "ep-2", "Sigo con Rust", "Sigo con Rust\n— Kal'tsit: Bien.");
        let o1 = episode(&mut old, "ep-o1", "(guarda silencio tras borrar lo que escribía)", "x");
        let c3 = episode(&mut old, "ep-3", "Adiós", "Adiós\n— Kal'tsit: Cuídate.");
        let c4 = episode(&mut old, "ep-4", "Ya volví", "Ya volví\n— Kal'tsit: Dime.");
        // The old chain ran through the ones that are discarded; c3 → c4 survives as it was.
        for (a, b) in [(c1, s1), (s1, c2), (c2, o1), (o1, c3), (c3, c4)] {
            old.upsert_edge(a, b, EdgeKind::Precedes, 1.0, 1.0);
        }
        // Time decides the order, not the id: c3 happened before c2.
        old.nodes[c1 as usize].y = 2026.80;
        old.nodes[c3 as usize].y = 2026.81;
        old.nodes[c2 as usize].y = 2026.82;
        old.nodes[c4 as usize].y = 2026.83;
        let (new, rep) = curated(&old, NOW).unwrap();
        let id = |k: &str| find(&rep, k).new_id.unwrap();
        let mut chain: Vec<(u32, u32)> = new.edges.iter().filter(|e| e.kind == EdgeKind::Precedes).map(|e| (e.from, e.to)).collect();
        chain.sort_unstable();
        // c1 → c3 → c2 → c4 in time order, plus the old c3 → c4 that survived translated.
        let mut want = vec![(id("ep-1"), id("ep-3")), (id("ep-3"), id("ep-2")), (id("ep-2"), id("ep-4")), (id("ep-3"), id("ep-4"))];
        want.sort_unstable();
        assert_eq!(chain, want);
        assert_eq!(rep.precedes_added, 3, "the three links through discarded episodes are new; the survivor is not counted");
        // Nothing is chained to what was discarded.
        assert!(["ep-s1", "ep-o1"].iter().all(|k| new.by_key(k).is_none()));
        // Without the discarded episodes in the middle nothing is added.
        let mut plain = old_memory();
        let a = episode(&mut plain, "ep-a", "Hola", "Hola\n— Kal'tsit: Dime.");
        let b = episode(&mut plain, "ep-b", "Adiós", "Adiós\n— Kal'tsit: Cuídate.");
        plain.upsert_edge(a, b, EdgeKind::Precedes, 1.0, 1.0);
        let (new, rep) = curated(&plain, NOW).unwrap();
        assert_eq!(rep.precedes_added, 0);
        assert_eq!(new.edges.iter().filter(|e| e.kind == EdgeKind::Precedes).count(), 1);
        // A single conversation has nothing to be chained to.
        let mut one = old_memory();
        episode(&mut one, "ep-a", "Hola", "Hola\n— Kal'tsit: Dime.");
        assert_eq!(curated(&one, NOW).unwrap().1.precedes_added, 0);
        // The report says it.
        assert!(rep.to_markdown(NOW).contains("«Precedes» añadidas"));
    }

    #[test]
    fn history_quotes_that_point_at_imported_episodes_are_translated() {
        let mut old = old_memory();
        let a = fact(&mut old, "f-a", "Usa Rust", "Ana programa en Rust.");
        let c = episode(&mut old, "ep-1", "Programo en Rust", "Programo en Rust\n— Kal'tsit: Bien.");
        let scr = episode(&mut old, "ep-s", "Vio la pantalla", "Vio: un editor");
        old.chat.push(ChatMessage { period: 0, role: Role::Assistant, turn: 1, text: "Dime".into(), recalled: vec![c, scr, a], at: 2.0 });
        let (new, rep) = curated(&old, NOW).unwrap();
        let (nc, na) = (find(&rep, "ep-1").new_id.unwrap(), find(&rep, "f-a").new_id.unwrap());
        assert_eq!(new.chat[0].recalled, vec![nc, na], "the screen one is taken out of the quote, the conversation one is translated");
        assert_eq!((rep.recalled_translated, rep.recalled_dropped), (2, 1));
        assert!(new.chat[0].recalled.iter().all(|&r| (r as usize) < new.nodes.len()));
    }

    #[test]
    fn one_equipment_node_per_machine_with_models_and_dates() {
        let mut old = old_memory();
        // Same machine, second model; and a second machine.
        let a2 = old.source_id(&Source { mac: MAC_A.into(), host: "SOBREMESA".into(), model: "qwen3-8b".into() });
        let b = old.source_id(&Source { mac: MAC_B.into(), host: "LAPTOP".into(), model: "gemma-3-4b".into() });
        let x = fact(&mut old, "f-x", "Dato del sobremesa", "dato");
        let y = fact(&mut old, "f-y", "Dato del laptop", "dato");
        old.nodes[x as usize].source = a2;
        old.nodes[x as usize].born = 1_792_000_000.0;
        old.nodes[y as usize].source = b;
        old.nodes[y as usize].born = 1_791_500_000.0;
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(rep.equipment.len(), 2);
        let eq = new.by_key("equipos").unwrap();
        let nodes: Vec<&Node> = new.nodes.iter().filter(|n| parents(&new, n.id) == ["equipos"]).collect();
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|n| n.kind == NodeKind::Object && new.find_edge(n.id, eq, EdgeKind::BelongsTo).is_some()));
        let sob = nodes.iter().find(|n| n.label == "SOBREMESA").unwrap();
        assert!(sob.text.contains("gemma-4-12B") && sob.text.contains("qwen3-8b"), "{}", sob.text);
        assert!(sob.text.contains(MAC_A));
        assert!(sob.text.contains(&date_of(1_790_000_000.0)) && sob.text.contains(&date_of(1_792_000_000.0)), "{}", sob.text);
        let lap = nodes.iter().find(|n| n.label == "LAPTOP").unwrap();
        assert!(lap.text.contains("gemma-3-4b") && !lap.text.contains("qwen"), "{}", lap.text);
        // The table of origins is kept as it was, and the seed is signed apart.
        assert_eq!(&new.sources[..old.sources.len()], &old.sources[..]);
        let g = new.by_key("memoria").unwrap();
        assert_eq!(new.sources[new.nodes[g as usize].source as usize].model, "semilla (base_es.ron)");
    }

    #[test]
    fn meta_keeps_the_vital_state_and_drops_the_old_structure() {
        let old = old_memory();
        let (new, rep) = curated(&old, NOW).unwrap();
        assert_eq!(new.meta("usuario"), Some("Ana"));
        assert_eq!(new.meta("vital.h.at"), Some("123"));
        assert_eq!(new.meta("p.persona"), None);
        assert_eq!(new.meta("seed"), Some("base_es.ron"));
        assert_eq!(new.turn, 7);
        assert!(rep.meta_dropped.iter().any(|(k, _)| k == "p.persona") && rep.meta_dropped.iter().any(|(k, _)| k == "seed"));
        assert!(rep.meta_kept.contains(&"usuario".to_string()));
    }

    #[test]
    fn the_result_encodes_and_decodes_in_format_2_and_saves_as_modules() {
        let mut old = old_memory();
        let a = fact(&mut old, "f-a", "Usa Rust", "Ana programa en Rust.");
        let b = fact(&mut old, "f-b", "Corre un modelo", "Un modelo GGUF en la GPU.");
        old.upsert_edge(a, b, EdgeKind::Relates, 1.0, 1.0);
        old.chat.push(ChatMessage { period: 0, role: Role::Assistant, turn: 1, text: "ok".into(), recalled: vec![a, b], at: 2.0 });
        let (new, rep) = curated(&old, NOW).unwrap();
        // One file.
        let bytes = binary::encode(&new);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), format::SECTORS);
        let back = binary::decode(&bytes).unwrap();
        // The single-file format does not carry module, source or birth time: compare the rest.
        let essence = |m: &Memory| m.nodes.iter().map(|n| (n.key.clone(), n.kind, n.flags, n.label.clone(), n.text.clone(), n.alpha, n.beta, n.aliases.clone())).collect::<Vec<_>>();
        assert_eq!(essence(&new), essence(&back));
        assert_eq!(format!("{:?}", new.edges), format!("{:?}", back.edges));
        assert_eq!(new.chat.len(), back.chat.len());
        // Module files.
        let d = std::env::temp_dir().join(format!("kaltsit-import-synth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mods: Vec<Module> = CORE.iter().map(|(n, t)| Module { name: n.to_string(), title: t.to_string() }).collect();
        modules::save(&d, &new, &mods, &[], NOW).unwrap();
        assert_roundtrip(&d, &new);
        let _ = std::fs::remove_dir_all(&d);
        assert!(rep.imported().count() >= 2);
    }

    /// The saved folder opens again and holds the same nodes, edges and chat (compared by key).
    fn assert_roundtrip(dir: &Path, new: &Memory) {
        let l = modules::load(dir).unwrap().expect("la carpeta se abre");
        assert!(l.pending.is_empty(), "aristas pendientes: {:?}", l.pending.len());
        assert_eq!(l.merged, 0);
        assert_eq!(l.mem.nodes.len(), new.nodes.len());
        let k = |m: &Memory, id: u32| m.nodes[id as usize].key.clone();
        let edges = |m: &Memory| {
            let mut v: Vec<(String, String, u8)> = m.edges.iter().map(|e| (k(m, e.from), k(m, e.to), e.kind.code())).collect();
            v.sort();
            v
        };
        assert_eq!(edges(&l.mem), edges(new));
        let chat = |m: &Memory| m.chat.iter().map(|c| (c.text.clone(), c.recalled.iter().map(|&r| k(m, r)).collect::<Vec<_>>())).collect::<Vec<_>>();
        assert_eq!(chat(&l.mem), chat(new));
        for n in &new.nodes {
            let b = &l.mem.nodes[l.mem.by_key(&n.key).unwrap() as usize];
            assert_eq!((&b.label, &b.text, b.kind, b.alpha, b.beta), (&n.label, &n.text, n.kind, n.alpha, n.beta), "{}", n.key);
        }
        let yo = std::fs::read(modules::dir_of(dir).join("yo.cthm")).unwrap();
        assert_eq!(u16::from_le_bytes([yo[4], yo[5]]), format::SECTORS);
    }

    // ── the real thing, over a copy ──

    fn walk(dir: &Path, base: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        use sha2::{Digest, Sha256};
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if p.is_dir() {
                walk(&p, base, out);
            } else if let Ok(b) = std::fs::read(&p) {
                out.insert(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"), Sha256::digest(&b).to_vec());
            }
        }
    }

    fn fingerprint(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    #[test]
    #[ignore = "reads data/ (copying it) and writes IMPORTACION-REVISION.md; run it by name"]
    fn curated_import_over_a_copy_of_the_real_memory() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let data = root.join("data");
        let before = fingerprint(&data);
        assert!(!before.is_empty(), "no hay data/");

        // Work on a copy, never on data/.
        let tmp = std::env::temp_dir().join(format!("kaltsit-import-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let copy = tmp.join("copia");
        std::fs::create_dir_all(modules::dir_of(&copy)).unwrap();
        for e in std::fs::read_dir(modules::dir_of(&data)).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "cthm") {
                std::fs::copy(&p, modules::dir_of(&copy).join(p.file_name().unwrap())).unwrap();
            }
        }
        std::fs::copy(data.join("historial.cthm"), copy.join("historial.cthm")).unwrap();

        let loaded = modules::load(&copy).unwrap().expect("la copia tiene memoria");
        let now = crate::clock::unix_now();
        let (new, rep) = curated(&loaded.mem, now).unwrap();

        std::fs::write(root.join("IMPORTACION-REVISION.md"), rep.to_markdown(now)).unwrap();

        // Save the new memory (format 2) in the temp folder and open it again.
        let out = tmp.join("nueva");
        modules::save(&out, &new, &loaded.modules, &[], now).unwrap();
        assert_roundtrip(&out, &new);

        // Summary for the person running it.
        println!("viejos: {} nodos, {} aristas, {} mensajes", loaded.mem.nodes.len(), loaded.mem.edges.len(), loaded.mem.chat.len());
        println!("semilla: {} nodos; importados {}; fusionados {}", rep.seed_nodes, rep.imported().count(), rep.merged().count());
        for (k, n) in rep.by_sector() {
            println!("  {:<48} {n}", rep.path_of(&k));
        }
        for r in Reason::ALL {
            println!("descartados · {}: {}", r.name(), rep.discarded(r).count());
        }
        for r in [Reason::Episode, Reason::ScreenEpisode] {
            let mut by_label: BTreeMap<String, usize> = BTreeMap::new();
            for n in rep.discarded(r) {
                *by_label.entry(n.label.clone()).or_default() += 1;
            }
            println!("  etiquetas de «{}»: {by_label:?}", r.name());
        }
        println!("aristas: conservadas {}, descartadas {}, colapsadas {}", rep.edges_kept, rep.edges_dropped, rep.edges_collapsed);
        println!("escenas: {} / {}", rep.scenes_kept, rep.scenes_dropped);
        println!("equipos: {}", rep.equipment.len());
        for e in &rep.equipment {
            println!("  {}", e.text);
        }
        println!("recalled: traducidos {}, quitados {}; mensajes {}", rep.recalled_translated, rep.recalled_dropped, rep.chat_messages);
        println!("meta conservados {}, descartados {}", rep.meta_kept.len(), rep.meta_dropped.len());
        println!("nuevos: {} nodos, {} aristas", new.nodes.len(), new.edges.len());

        // data/ is exactly as it was.
        assert_eq!(fingerprint(&data), before, "data/ cambió");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
