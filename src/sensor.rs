//! Sensory buffer for the chat input: tracks text typing and deletions (Backspace)
//! strictly inside the AI's chat box, detecting hesitation and wiped thoughts.

use crate::cthg::text;

/// A deletion longer than this many characters is remembered as hesitation, not a typo.
pub const GHOST_MIN_CHARS: usize = 6;
/// A pause this long in the middle of typing settles deletions.
pub const PAUSE_S: f64 = 1.2;

fn len(s: &str) -> usize {
    s.chars().count()
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// The last `n` words of `s`, for context in front of a deleted fragment.
fn last_words(s: &str, n: usize) -> String {
    let w: Vec<&str> = s.split_whitespace().collect();
    w[w.len().saturating_sub(n)..].join(" ")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Utterance {
    pub said: String,
    pub ghosts: Vec<String>,
    /// Said out loud, not typed: the turns the fine ear heard (who spoke and what), as the
    /// microphone's dictation returned them.
    pub voz: Option<serde_json::Value>,
    /// How it sounded (the open microphone's measures, `openmic::Flush::sound_json`).
    pub sonido: Option<serde_json::Value>,
}

#[derive(Default, Debug)]
pub struct Sensor {
    pub draft: String,
    /// The draft as it was when the current run of deletions started.
    pub peak: String,
    /// Characters of `peak` that survive the current run of deletions.
    pub keep: usize,
    pub deleting: bool,
    /// Deleted hesitations of the current turn.
    pub ghosts: Vec<String>,
    /// The last ghost wiped the whole draft: a sentence taken back, not a word corrected.
    pub wiped: bool,
    pub last_key: Option<f64>,
}

impl Sensor {
    /// One snapshot of the text box after an input event.
    pub fn observe(&mut self, text: &str, t: f64) {
        self.last_key = Some(t);
        if text == self.draft {
            return;
        }
        if text.is_empty() && self.draft.is_empty() && !self.deleting {
            return;
        }
        let cp = common_prefix(&self.draft, text);
        if cp < len(&self.draft) {
            if !self.deleting {
                self.deleting = true;
                self.peak = self.draft.clone();
                self.keep = cp;
            } else {
                self.keep = self.keep.min(cp);
            }
            // Typed over a selection: the deletion is over already.
            if len(text) > cp {
                self.settle();
            }
        } else if self.deleting {
            self.settle();
        }
        self.draft = text.to_string();
    }

    /// Ends a run of deletions. What was erased stays as a ghost if it was more than a typo.
    pub fn settle(&mut self) {
        if !self.deleting {
            return;
        }
        self.deleting = false;
        let kept: String = self.peak.chars().take(self.keep).collect();
        let gone: String = self.peak.chars().skip(self.keep).collect();
        if len(gone.trim()) > GHOST_MIN_CHARS {
            let ghost = if kept.trim().is_empty() {
                gone.trim().to_string()
            } else {
                format!("…{} {}", last_words(&kept, 3), gone.trim()).replace("  ", " ")
            };
            self.wiped = kept.trim().is_empty();
            self.ghosts.push(text::clip(&ghost, 240));
        }
    }

    /// A deletion with no typing after it is over once the user pauses.
    pub fn settle_if_idle(&mut self, t: f64) {
        if self.deleting && self.paused(t) >= PAUSE_S {
            self.settle();
        }
    }

    pub fn paused(&self, t: f64) -> f64 {
        self.last_key.map(|k| t - k).unwrap_or(f64::INFINITY)
    }

    /// Takes ghosts that were wiped when user went silent without sending.
    pub fn take_wiped_ghosts_if_silent(&mut self, t: f64, silence_s: f64) -> Vec<String> {
        self.settle_if_idle(t);
        if self.draft.trim().is_empty() && self.wiped && !self.ghosts.is_empty() && self.paused(t) >= silence_s {
            self.wiped = false;
            std::mem::take(&mut self.ghosts)
        } else {
            Vec::new()
        }
    }

    /// Consumes the buffer for a spoken message. A ghost the user ended up saying anyway is not a hesitation.
    pub fn take_for(&mut self, said: &str) -> Utterance {
        self.settle();
        let folded = text::fold(said);
        let ghosts = std::mem::take(&mut self.ghosts)
            .into_iter()
            .filter(|g| {
                let f = text::fold(g.trim_start_matches('…'));
                let f = f.trim();
                !f.is_empty() && !folded.contains(f)
            })
            .collect();
        self.draft.clear();
        self.peak.clear();
        self.keep = 0;
        self.deleting = false;
        self.wiped = false;
        self.ghosts.clear();
        Utterance {
            said: said.trim().to_string(),
            ghosts,
            voz: None,
            sonido: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &mut Sensor, text: &str, t: &mut f64) {
        let mut cur = s.draft.clone();
        for c in text.chars() {
            cur.push(c);
            *t += 0.1;
            s.observe(&cur, *t);
        }
    }

    fn backspace(s: &mut Sensor, n: usize, t: &mut f64) {
        let mut cur: Vec<char> = s.draft.chars().collect();
        for _ in 0..n {
            cur.pop();
            *t += 0.08;
            s.observe(&cur.iter().collect::<String>(), *t);
        }
    }

    #[test]
    fn deleted_sentence_is_a_ghost_and_typos_are_not() {
        let mut s = Sensor::default();
        let mut t = 0.0;
        typed(&mut s, "Oye, sobre Texas...", &mut t);
        backspace(&mut s, 19, &mut t);
        typed(&mut s, "Nada, olvidalo", &mut t);
        backspace(&mut s, 2, &mut t);
        typed(&mut s, "lo.", &mut t);
        let u = s.take_for("Nada, olvidalo.");
        assert_eq!(u.said, "Nada, olvidalo.");
        assert_eq!(u.ghosts, vec!["Oye, sobre Texas...".to_string()]);
        assert!(s.ghosts.is_empty() && s.draft.is_empty());
    }

    #[test]
    fn partial_deletion_keeps_context_and_resaid_text_is_dropped() {
        let mut s = Sensor::default();
        let mut t = 0.0;
        typed(&mut s, "Hoy vi a tu hermana Lunacub", &mut t);
        backspace(&mut s, 18, &mut t);
        typed(&mut s, "alguien.", &mut t);
        let u = s.take_for("Hoy vi a alguien.");
        assert_eq!(u.ghosts, vec!["…Hoy vi a tu hermana Lunacub".to_string()]);
        assert!(!s.wiped);

        // Deleted and typed back word for word: not a hesitation.
        typed(&mut s, "Te traje un regalo", &mut t);
        backspace(&mut s, 9, &mut t);
        typed(&mut s, "un regalo", &mut t);
        assert!(s.take_for("Te traje un regalo").ghosts.is_empty());
    }

    #[test]
    fn wiped_sentence_is_detected_after_silence() {
        let mut s = Sensor::default();
        let mut t = 0.0;
        typed(&mut s, "Un secreto que no debo contar", &mut t);
        backspace(&mut s, 29, &mut t);
        assert!(s.take_wiped_ghosts_if_silent(t + 2.0, 8.0).is_empty());
        let wiped = s.take_wiped_ghosts_if_silent(t + 8.5, 8.0);
        assert_eq!(wiped, vec!["Un secreto que no debo contar".to_string()]);
        assert!(s.take_wiped_ghosts_if_silent(t + 9.0, 8.0).is_empty());
    }
}
