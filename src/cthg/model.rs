//! Data model of the CTHG (Causal-Temporal HyperGraph).
//!
//! Axes of the 3D memory:
//! - X: space — where the memory lives (Columbia ... Rhodes Island), in [-1, 1].
//! - Y: time — Terran year as a float. Immutable once written and forward-only.
//! - Z: causality — the χ score in [-1, 1], computed (never stored) by `score.rs`.
//!   +1: she understands why it happened. −1: she finds no sense in it having happened.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Person,
    Place,
    Event,
    Trait,
    Object,
    Fact,
    Episode,
    /// How someone looks: hair, scars, clothes of a given period.
    Appearance,
    /// A sector of the memory tree. Its `label` is the name, its `text` a description.
    /// Format 2 and later: an older reader refuses a memory that contains one.
    Sector,
}

impl NodeKind {
    pub const ALL: [NodeKind; 9] = [
        NodeKind::Person,
        NodeKind::Place,
        NodeKind::Event,
        NodeKind::Trait,
        NodeKind::Object,
        NodeKind::Fact,
        NodeKind::Episode,
        NodeKind::Appearance,
        NodeKind::Sector,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.get(c as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Person => "persona",
            NodeKind::Place => "lugar",
            NodeKind::Event => "evento",
            NodeKind::Trait => "rasgo",
            NodeKind::Object => "objeto",
            NodeKind::Fact => "hecho",
            NodeKind::Episode => "episodio",
            NodeKind::Appearance => "apariencia",
            NodeKind::Sector => "sector",
        }
    }

    /// First file format that can hold this kind.
    pub fn min_format(self) -> u16 {
        match self {
            NodeKind::Sector => format::SECTORS,
            _ => format::LEGACY,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.name() == s)
    }

    /// Entities are what episodes and facts get attached to.
    pub fn is_entity(self) -> bool {
        matches!(self, NodeKind::Person | NodeKind::Place | NodeKind::Object)
    }
}

/// Where a memory came from. Decides its prior and whether it is anchored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Canon from the manhua / story / archive files. Anchored: Y-locked, cannot be overturned.
    Canon,
    /// Deduced from canon (e.g. an uncertain date). Strong prior, not anchored.
    Inferred,
    /// Hearsay or fan claims. Even prior, may contradict canon.
    Rumor,
    /// Learned from the roleplay chat.
    Learned,
    /// Raw chat episode.
    Chat,
}

impl Origin {
    pub const ALL: [Origin; 5] = [
        Origin::Canon,
        Origin::Inferred,
        Origin::Rumor,
        Origin::Learned,
        Origin::Chat,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.get(c as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Origin::Canon => "canon",
            Origin::Inferred => "inferido",
            Origin::Rumor => "rumor",
            Origin::Learned => "aprendido",
            Origin::Chat => "chat",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|o| o.name() == s)
    }

    /// Beta prior (α, β) for the chance view.
    pub fn prior(self) -> (f32, f32) {
        match self {
            Origin::Canon => (20.0, 0.5),
            Origin::Inferred => (5.0, 1.5),
            Origin::Rumor => (1.5, 1.5),
            Origin::Learned => (1.0, 1.0),
            Origin::Chat => (3.0, 1.0),
        }
    }
}

pub mod flags {
    /// Y-locked canon: the engineer view treats it as a deterministic root.
    pub const ANCHORED: u8 = 1 << 0;
    /// A learned fact that gathered enough agreeing evidence.
    pub const CONSOLIDATED: u8 = 1 << 1;
    /// Always injected into the prompt (identity core).
    pub const CORE: u8 = 1 << 2;
    /// Canon after the chronicle's starting point: a possible future, not a promise.
    /// It never becomes known by the mere passing of time; only if it happens in the roleplay.
    pub const POTENTIAL: u8 = 1 << 3;
    /// Dated in the future on purpose (something she was told will happen): known already,
    /// and it does not move her present forward.
    pub const SCHEDULED: u8 = 1 << 4;
    // Bit 5 (`DATED`) lives in `mind.rs`.
    /// A personal-core memory: something she chose to keep as hers, beside `CORE`.
    /// Format 2 and later: an older reader refuses a memory that carries it.
    #[allow(dead_code)] // nothing sets it until phase 6; the format already carries it
    pub const PERSONAL_CORE: u8 = 1 << 6;
    /// Bits of the first format (0..=5).
    pub const LEGACY_MASK: u8 = 0b0011_1111;
    /// Every bit that has a meaning today (0..=6). Bit 7 is free and refused when read.
    pub const KNOWN_MASK: u8 = 0b0111_1111;
}

/// Versions of the file formats (`.cthm`, and `.cthg` that shares them).
pub mod format {
    /// Memories written before sectors: no sector nodes, no sector edges, no personal core.
    pub const LEGACY: u16 = 1;
    /// Sectors, belonging, inspired-by and the personal-core flag.
    pub const SECTORS: u16 = 2;
    /// The newest this build writes and reads.
    pub const CURRENT: u16 = SECTORS;

    /// Format a node needs: the newest of its kind and its flags.
    pub fn node_needs(kind: super::NodeKind, flags: u8) -> u16 {
        let f = if flags & !super::flags::LEGACY_MASK != 0 { SECTORS } else { LEGACY };
        kind.min_format().max(f)
    }

    /// Checks a node's kind and flags against the file's `version`; the error says what is wrong.
    pub fn check_node(version: u16, kind: super::NodeKind, flags: u8) -> Result<(), String> {
        if flags & !super::flags::KNOWN_MASK != 0 {
            return Err(format!("marcas de nodo desconocidas ({flags:#04x}): memoria de una app más nueva"));
        }
        if node_needs(kind, flags) > version {
            return Err(format!("el formato {version} no admite nodos «{}» ni la marca de núcleo personal (exigen {SECTORS})", kind.name()));
        }
        Ok(())
    }

    /// Checks an edge kind against the file's `version`.
    pub fn check_edge(version: u16, kind: super::EdgeKind) -> Result<(), String> {
        if kind.min_format() > version {
            return Err(format!("el formato {version} no admite aristas «{}» (exigen {})", kind.name(), kind.min_format()));
        }
        Ok(())
    }

    /// Checks the version number read from a file.
    pub fn check_version(version: u16) -> Result<(), String> {
        if version < LEGACY {
            return Err(format!("versión de formato {version} inválida"));
        }
        if version > CURRENT {
            return Err(format!("memoria de una versión más nueva ({version}); esta app llega hasta la {CURRENT}"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub kind: NodeKind,
    pub origin: Origin,
    pub flags: u8,
    /// Period that wrote this memory (index), or 255 for canon spanning all.
    pub period: u8,
    /// Space axis in [-1, 1].
    pub x: f32,
    /// Time axis: Terran year.
    pub y: f64,
    /// Chance view: Beta evidence for / against.
    pub alpha: f32,
    pub beta: f32,
    /// Recency of recall in [0, 1]; decays every turn (forgetting).
    pub activation: f32,
    pub recalls: u32,
    pub created_turn: u32,
    pub key: String,
    pub label: String,
    pub text: String,
    pub aliases: Vec<String>,
    /// Memory module it is stored in (yo, personas, cosas, saber, or an extra one).
    pub module: u8,
    /// Index in `Memory::sources`: the machine and model that produced this memory.
    pub source: u16,
    /// When it was written, unix seconds (real time).
    pub born: f64,
}

/// Where a memory comes from: the machine (MAC, computer name) and the model that produced it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub mac: String,
    pub host: String,
    pub model: String,
}

impl Node {
    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }

    pub fn anchored(&self) -> bool {
        self.has(flags::ANCHORED)
    }

    /// Whether this is a memory: something that can come to mind, go into a prompt or be learned
    /// on. A `Sector` is structure (the tree memories hang from), never a memory.
    pub fn is_memory(&self) -> bool {
        self.kind != NodeKind::Sector
    }

    /// Whether the moral layer defends this node: `CORE`, and a memory. A sector is never an
    /// axiom, even if it carried the flag.
    pub fn is_axiom(&self) -> bool {
        self.has(flags::CORE) && self.is_memory()
    }

    /// Evidence gathered beyond the uniform prior.
    pub fn evidence(&self) -> f32 {
        (self.alpha + self.beta - 2.0).max(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// Directed cause → effect. Its α/β measure how deterministic the link is.
    Causes,
    /// Pure temporal succession (episode chain).
    Precedes,
    /// Spatial / associative link, Hebbian weight.
    Relates,
    /// The source contradicts the target.
    Contradicts,
    /// An episode or fact mentions an entity.
    Mentions,
    /// The source supersedes the target (replaces it; the target is no longer injected).
    Supersedes,
    /// The source node belongs to the target sector; `alpha`/`beta` give how much.
    /// A node may have several. Format 2 and later.
    BelongsTo,
    /// Between two distinct entities that share a name: the source was inspired by the target
    /// (e.g. the Kal'tsit of here, the one of Arknights). Format 2 and later.
    InspiredBy,
}

impl EdgeKind {
    pub const ALL: [EdgeKind; 8] = [
        EdgeKind::Causes,
        EdgeKind::Precedes,
        EdgeKind::Relates,
        EdgeKind::Contradicts,
        EdgeKind::Mentions,
        EdgeKind::Supersedes,
        EdgeKind::BelongsTo,
        EdgeKind::InspiredBy,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.get(c as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeKind::Causes => "causa",
            EdgeKind::Precedes => "precede",
            EdgeKind::Relates => "relacion",
            EdgeKind::Contradicts => "contradice",
            EdgeKind::Mentions => "menciona",
            EdgeKind::Supersedes => "sustituye",
            EdgeKind::BelongsTo => "pertenece",
            EdgeKind::InspiredBy => "inspirada_en",
        }
    }

    /// First file format that can hold this kind.
    pub fn min_format(self) -> u16 {
        match self {
            EdgeKind::BelongsTo | EdgeKind::InspiredBy => format::SECTORS,
            _ => format::LEGACY,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.name() == s)
    }
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
    pub kind: EdgeKind,
    pub alpha: f32,
    pub beta: f32,
    /// Hebbian association strength in [0, 1].
    pub hebb: f32,
    pub created_turn: u32,
}

impl Edge {
    /// Determinism of a causal link in [0, 1]: 2p - 1 clamped.
    pub fn determinism(&self) -> f32 {
        let p = self.alpha / (self.alpha + self.beta).max(1e-6);
        (2.0 * p - 1.0).clamp(0.0, 1.0)
    }
}

/// A scene: one hyperedge binding many nodes (participants, place, events).
#[derive(Clone, Debug)]
pub struct Hyperedge {
    pub id: u32,
    pub label: String,
    pub y: f64,
    pub members: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Period {
    pub id: u8,
    pub key: String,
    pub name: String,
    pub source: String,
    pub start: f64,
    pub end: f64,
    /// The roleplay "present" of this period. Anything with y > now is outside her memory.
    pub now: f64,
    pub home_x: f32,
    pub persona: String,
    pub scene: String,
    pub opening: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    /// Narration written by the system (time skips, realized futures).
    Narrator,
    /// Something the user typed and deleted before speaking (`deleted_hesitation`).
    Ghost,
    /// What she perceived without words: a failed interruption, a sentence left half-said.
    Sense,
}

impl Role {
    pub fn code(self) -> u8 {
        match self {
            Role::User => 0,
            Role::Assistant => 1,
            Role::Narrator => 2,
            Role::Ghost => 3,
            Role::Sense => 4,
        }
    }

    pub fn from_code(c: u8) -> Self {
        match c {
            0 => Role::User,
            2 => Role::Narrator,
            3 => Role::Ghost,
            4 => Role::Sense,
            _ => Role::Assistant,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Narrator => "narrator",
            Role::Ghost => "ghost",
            Role::Sense => "sense",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub period: u8,
    pub role: Role,
    pub turn: u32,
    pub text: String,
    /// Node ids recalled for this reply (empty for user messages).
    pub recalled: Vec<u32>,
    /// When it was said, unix seconds (real time).
    pub at: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub hyperedges: Vec<Hyperedge>,
    pub periods: Vec<Period>,
    pub chat: Vec<ChatMessage>,
    pub turn: u32,
    pub meta: Vec<(String, String)>,
    pub sources: Vec<Source>,
}

impl Memory {
    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.nodes.get_mut(id as usize)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn by_key(&self, key: &str) -> Option<u32> {
        self.nodes.iter().find(|n| n.key == key).map(|n| n.id)
    }

    /// Index of `s` in the sources table, adding it if new.
    pub fn source_id(&mut self, s: &Source) -> u16 {
        match self.sources.iter().position(|x| x == s) {
            Some(i) => i as u16,
            None => {
                self.sources.push(s.clone());
                (self.sources.len() - 1) as u16
            }
        }
    }

    pub fn meta(&self, k: &str) -> Option<&str> {
        self.meta.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    pub fn set_meta(&mut self, k: &str, v: &str) {
        match self.meta.iter_mut().find(|(a, _)| a == k) {
            Some(e) => e.1 = v.to_string(),
            None => self.meta.push((k.to_string(), v.to_string())),
        }
    }

    pub fn add_node(&mut self, mut n: Node) -> u32 {
        let id = self.nodes.len() as u32;
        n.id = id;
        self.nodes.push(n);
        id
    }

    pub fn find_edge(&self, from: u32, to: u32, kind: EdgeKind) -> Option<usize> {
        self.edges
            .iter()
            .position(|e| e.from == from && e.to == to && e.kind == kind)
    }

    /// Adds the edge, or returns the index of the existing one with the same endpoints and kind.
    pub fn upsert_edge(&mut self, from: u32, to: u32, kind: EdgeKind, alpha: f32, beta: f32) -> usize {
        if let Some(i) = self.find_edge(from, to, kind) {
            return i;
        }
        self.edges.push(Edge {
            from,
            to,
            kind,
            alpha,
            beta,
            hebb: 0.0,
            created_turn: self.turn,
        });
        self.edges.len() - 1
    }

    /// The period's present: its seeded `now`, moved by time skips (meta `present`) and pushed
    /// forward by every turn written in it. Time only advances, so this never moves back, and it
    /// has no upper bound: a chronicle may live past the end of its period.
    pub fn horizon(&self, period: u8) -> f64 {
        let p = &self.periods[period as usize];
        let set = self.meta("present").and_then(|s| s.parse::<f64>().ok()).unwrap_or(p.now).max(p.now);
        self.nodes
            .iter()
            .filter(|n| n.period == period && matches!(n.origin, Origin::Chat | Origin::Learned) && !n.has(flags::SCHEDULED))
            .map(|n| n.y)
            .fold(set, f64::max)
    }

    /// Period whose window contains year `y`.
    pub fn period_of(&self, y: f64) -> Option<&Period> {
        self.periods.iter().find(|p| y >= p.start && y < p.end)
    }
}
