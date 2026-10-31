//! A small endocrine model of her behaviour. Five levels in [0, 1], each drifting back to
//! Kal'tsit's own baseline with its own half-life, pushed by what happens (never by keywords:
//! by the extractor's reading of the scene, by learning events, by who cut whom, by silence).
//!
//! - dopamina: wanting, novelty, reward: for her, mostly curiosity about what she sees.
//! - cortisol: stress, being contradicted, cut off or left alone.
//! - serotonina: calm and certainty; restraint over impulses.
//! - oxitocina: bond with the user: warmth, staying, coming back.
//! - adrenalina: the spike of the moment: tension rising, a cut, a fight.
//!
//! They decide how much of the immediate conversation she keeps (the dynamic window), how warm
//! the sampling runs, how strongly what she hears changes her opinions (plasticity), whether she
//! walks or dozes off, and a short "por dentro" line that colours her tone and judgement. At her
//! baseline every effect is neutral.

use crate::cthg::model::Memory;
use crate::lang::Lang;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H {
    Dopamina,
    Cortisol,
    Serotonina,
    Oxitocina,
    Adrenalina,
}

impl H {
    pub const ALL: [H; 5] = [H::Dopamina, H::Cortisol, H::Serotonina, H::Oxitocina, H::Adrenalina];

    pub fn name(self) -> &'static str {
        match self {
            H::Dopamina => "dopamina",
            H::Cortisol => "cortisol",
            H::Serotonina => "serotonina",
            H::Oxitocina => "oxitocina",
            H::Adrenalina => "adrenalina",
        }
    }

    /// Kal'tsit at rest: composed, a little weary, guarded with everyone.
    pub fn baseline(self) -> f32 {
        match self {
            H::Dopamina => 0.35,
            H::Cortisol => 0.3,
            H::Serotonina => 0.6,
            H::Oxitocina => 0.25,
            H::Adrenalina => 0.15,
        }
    }

    /// Seconds (of the story's clock) to fall halfway back to baseline.
    fn half_life(self) -> f64 {
        match self {
            H::Dopamina => 120.0,
            H::Cortisol => 600.0,
            H::Serotonina => 1800.0,
            H::Oxitocina => 1200.0,
            H::Adrenalina => 45.0,
        }
    }
}

/// A turn without the autonomous clock running still lets this much time pass for the hormones.
pub const TURN_S: f64 = 20.0;
/// Seconds between a save and an open below which no time is taken as having passed.
const AWAY_MIN_S: f64 = 5.0;
/// Immediate-window size at baseline and its bounds.
pub const WINDOW_BASE: f32 = 120.0;
const WINDOW_MIN: f32 = 60.0;
const WINDOW_MAX: f32 = 360.0;

/// What happened, as far as her body is concerned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stimulus {
    /// The extractor's reading of a message: scene tension (0..1), its rise since the last
    /// message, and how the user treats her (afecto, −1 hostile .. +1 warm).
    Read { tension: f32, rise: f32, afecto: f32 },
    /// Mean comprehension (u = (χ+1)/2) of what came to mind.
    Understood(f32),
    /// A possible future came true in the story.
    Fulfilled,
    NewFact,
    Reinforced,
    /// What she held was contradicted or weakened.
    Contradicted,
    /// She did something on her own.
    Acted,
    /// Bored for `dt` seconds at this level of boredom.
    Bored { dt: f64, level: f32 },
    /// The user typed and deleted something before speaking, or wiped a sentence.
    Hesitation,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hormones {
    pub level: [f32; 5],
}

impl Default for Hormones {
    fn default() -> Self {
        Hormones { level: H::ALL.map(H::baseline) }
    }
}

impl Hormones {
    pub fn get(&self, h: H) -> f32 {
        self.level[h as usize]
    }

    /// How far a level is from her baseline.
    fn dev(&self, h: H) -> f32 {
        self.get(h) - h.baseline()
    }

    fn add(&mut self, h: H, v: f32) {
        let x = &mut self.level[h as usize];
        *x = (*x + v).clamp(0.0, 1.0);
    }

    /// Each level relaxes toward its baseline: `dt` seconds of the story's clock.
    pub fn decay(&mut self, dt: f64) {
        for h in H::ALL {
            let k = 0.5f64.powf(dt / h.half_life()) as f32;
            let b = h.baseline();
            let x = &mut self.level[h as usize];
            *x = b + (*x - b) * k;
        }
    }

    pub fn feel(&mut self, s: Stimulus) {
        use H::*;
        match s {
            Stimulus::Read { tension, rise, afecto } => {
                let rise = rise.max(0.0);
                // Being spoken to is a little interesting; rising tension worries more than it thrills.
                self.add(Dopamina, 0.1 * rise + 0.04);
                self.add(Adrenalina, 0.35 * rise + 0.08 * tension);
                self.add(Cortisol, 0.18 * tension + 0.25 * (-afecto).max(0.0));
                self.add(Oxitocina, 0.2 * afecto.max(0.0) - 0.12 * (-afecto).max(0.0));
                self.add(Serotonina, 0.08 * afecto);
            }
            Stimulus::Understood(u) => {
                // Certainty settles her slowly toward a level set by how much she understands.
                let target = 0.25 + 0.5 * u;
                let s = self.get(Serotonina);
                self.add(Serotonina, 0.15 * (target - s));
            }
            Stimulus::Fulfilled => self.add(Dopamina, 0.3),
            Stimulus::NewFact => self.add(Dopamina, 0.04),
            Stimulus::Reinforced => self.add(Serotonina, 0.04),
            Stimulus::Contradicted => {
                self.add(Cortisol, 0.08);
                self.add(Dopamina, -0.03);
            }
            Stimulus::Acted => self.add(Dopamina, 0.05),
            Stimulus::Bored { dt, level } => {
                // Boredom pulls dopamine toward apathy (0.1) as hard as it is deep, against the
                // pull back to baseline: deep boredom settles around 0.25, never instantly.
                let k = 1.0 - 0.5f64.powf(dt * level.clamp(0.0, 1.5) as f64 / 120.0) as f32;
                let d = self.get(Dopamina);
                self.add(Dopamina, (0.1 - d) * k);
            }
            Stimulus::Hesitation => {
                self.add(Dopamina, 0.06);
                self.add(Adrenalina, 0.04);
            }
        }
    }

    /// Attention: tokens of the immediate conversation she keeps. Engagement, the bond and
    /// arousal widen it; strong stress narrows it (tunnel vision).
    pub fn window_tokens(&self) -> usize {
        use H::*;
        let a = 1.0 + 1.6 * self.dev(Dopamina) + 1.0 * self.dev(Oxitocina) + 0.8 * self.dev(Adrenalina)
            - 1.6 * (self.get(Cortisol) - 0.6).max(0.0);
        (WINDOW_BASE * a).clamp(WINDOW_MIN, WINDOW_MAX).round() as usize
    }

    /// Sampling temperature of her lines: excitement loosens, calm steadies.
    pub fn temperature(&self) -> f32 {
        use H::*;
        (0.75 + 0.2 * self.dev(Dopamina) + 0.15 * self.dev(Adrenalina) - 0.2 * self.dev(Serotonina)).clamp(0.5, 1.0)
    }

    /// How much one piece of evidence moves her opinion: dopamine gates plasticity, so what she
    /// hears while engaged settles her χ faster (and while apathetic, barely at all).
    pub fn plasticity(&self) -> f32 {
        (1.0 + 1.2 * self.dev(H::Dopamina)).clamp(0.5, 1.6)
    }

    /// "Por dentro": only the levels far enough from her baseline to colour her, in plain words.
    pub fn describe(&self, user: &str, lang: Lang) -> String {
        use H::*;
        let es = lang == Lang::Es;
        let mut parts: Vec<String> = Vec::new();
        let (d, c, s, o, a) = (self.dev(Dopamina), self.dev(Cortisol), self.dev(Serotonina), self.dev(Oxitocina), self.dev(Adrenalina));
        if d >= 0.3 {
            parts.push(if es { "con una curiosidad que no admitirías en voz alta" } else { "with a curiosity you would not admit aloud" }.into());
        } else if d <= -0.2 {
            parts.push(if es { "cansada: todo te parece previsible" } else { "weary: everything seems predictable to you" }.into());
        }
        if a >= 0.4 {
            parts.push(if es { "alerta" } else { "alert" }.into());
        }
        if c >= 0.3 {
            parts.push(if es { "tensa y con la paciencia corta" } else { "tense and short on patience" }.into());
        }
        if s >= 0.2 {
            parts.push(if es { "serena y segura de tu juicio" } else { "serene and sure of your judgment" }.into());
        } else if s <= -0.3 {
            parts.push(if es { "irritable: nada termina de encajar" } else { "irritable: nothing quite fits" }.into());
        }
        if o >= 0.3 {
            parts.push(if es { format!("{user} te importa más de lo que dirías") } else { format!("{user} matters to you more than you would say") });
        } else if o <= -0.15 {
            parts.push(if es { format!("no te fías de {user}") } else { format!("you do not trust {user}") });
        }
        match parts.len() {
            0 => String::new(),
            1 => parts.remove(0),
            _ => {
                let last = parts.pop().unwrap();
                format!("{} {} {last}", parts.join(", "), if es { "y" } else { "and" })
            }
        }
    }

    /// Her levels as they were saved, relaxed by the real time that passed since: time runs for
    /// her while the app is closed too, so a tense moment does not stay frozen until she wakes.
    pub fn load(m: &Memory) -> Hormones {
        Self::load_at(m, crate::clock::unix_now())
    }

    pub fn load_at(m: &Memory, now: f64) -> Hormones {
        let mut h = Hormones::default();
        for k in H::ALL {
            if let Some(v) = m.meta(&format!("vital.h.{}", k.name())).and_then(|s| s.parse::<f32>().ok()) {
                h.level[k as usize] = v.clamp(0.0, 1.0);
            }
        }
        if let Some(at) = m.meta("vital.h.at").and_then(|s| s.parse::<f64>().ok()) {
            h.relax_since(at, now);
        }
        h
    }

    /// Relaxes her levels by the time between `at` (when they were last true) and `now`.
    pub fn relax_since(&mut self, at: f64, now: f64) {
        let away = now - at;
        // A reload within the same moment (a save and an open in a row) changes nothing.
        if away > AWAY_MIN_S {
            self.decay(away);
        }
    }

    pub fn store(&self, m: &mut Memory) {
        for k in H::ALL {
            m.set_meta(&format!("vital.h.{}", k.name()), &format!("{:.3}", self.get(k)));
        }
        m.set_meta("vital.h.at", &format!("{:.0}", crate::clock::unix_now()));
    }

    pub fn json(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        for k in H::ALL {
            o.insert(k.name().into(), serde_json::json!(self.get(k)));
        }
        o.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_baseline_everything_is_neutral() {
        let h = Hormones::default();
        assert_eq!(h.window_tokens(), 120);
        assert!((h.plasticity() - 1.0).abs() < 1e-6);
        assert!((h.temperature() - 0.75).abs() < 1e-6);
        assert_eq!(h.describe("Ana", Lang::Es), "");
        assert_eq!(h.describe("Ana", Lang::En), "");
    }

    #[test]
    fn levels_relax_to_baseline() {
        let mut mut_h = Hormones::default();
        mut_h.level = [1.0; 5];
        mut_h.decay(1e6);
        assert!(H::ALL.iter().all(|&k| (mut_h.get(k) - k.baseline()).abs() < 1e-3));
    }

    #[test]
    fn warmth_and_hostility_move_her_trust() {
        let mut warm = Hormones::default();
        for _ in 0..4 {
            warm.feel(Stimulus::Read { tension: 0.1, rise: 0.0, afecto: 1.0 });
        }
        assert!(warm.describe("Ana", Lang::Es).contains("Ana te importa"), "{}", warm.describe("Ana", Lang::Es));
        assert!(warm.describe("Ana", Lang::En).contains("Ana matters to you more than you would say"), "{}", warm.describe("Ana", Lang::En));
        assert!(warm.window_tokens() > 120);
        let mut cold = Hormones::default();
        for _ in 0..3 {
            cold.feel(Stimulus::Read { tension: 0.7, rise: 0.2, afecto: -1.0 });
        }
        assert!(cold.describe("Ana", Lang::Es).contains("no te fías de Ana"), "{}", cold.describe("Ana", Lang::Es));
        assert!(cold.describe("Ana", Lang::En).contains("you do not trust Ana"), "{}", cold.describe("Ana", Lang::En));
    }

    #[test]
    fn hesitation_stimulates_curiosity() {
        let mut h = Hormones::default();
        let d0 = h.get(H::Dopamina);
        let a0 = h.get(H::Adrenalina);
        h.feel(Stimulus::Hesitation);
        assert!(h.get(H::Dopamina) > d0);
        assert!(h.get(H::Adrenalina) > a0);
    }

    #[test]
    fn hormones_roundtrip_through_meta() {
        let mut m = Memory::default();
        let mut h = Hormones::default();
        h.feel(Stimulus::Fulfilled);
        h.store(&mut m);
        assert_eq!(Hormones::load(&m).get(H::Dopamina), (h.get(H::Dopamina) * 1000.0).round() / 1000.0);
    }

    #[test]
    fn time_passes_while_the_app_is_closed() {
        let mut m = Memory::default();
        let mut h = Hormones::default();
        h.level[H::Cortisol as usize] = 0.94;
        h.level[H::Oxitocina as usize] = 0.135;
        h.store(&mut m);
        let saved = m.meta("vital.h.at").unwrap().parse::<f64>().unwrap();
        // An hour later her stress is gone (cortisol halves every 10 min) and the bond came back.
        let back = Hormones::load_at(&m, saved + 3600.0);
        assert!((back.get(H::Cortisol) - H::Cortisol.baseline()).abs() < 0.02, "{}", back.get(H::Cortisol));
        assert!((back.get(H::Oxitocina) - H::Oxitocina.baseline()).abs() < 0.03, "{}", back.get(H::Oxitocina));
        // Opening it again right away changes nothing.
        assert_eq!(Hormones::load_at(&m, saved + 1.0).get(H::Cortisol), 0.94);
    }
}
