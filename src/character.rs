//! Who can live on this desktop. Each companion has her own memory, born from her own seed, her
//! own body and her own voice; they share the model (taking turns), the computer's activity log,
//! the agenda and a board of notes between them.

use std::path::{Path, PathBuf};

/// A companion that can be chosen on start.
#[derive(Debug, PartialEq, Eq)]
pub struct Character {
    /// Internal and stable: names her memory folder and her seed in the memory meta.
    pub id: &'static str,
    /// What she is called (in the chat, the prompts, the window titles).
    pub name: &'static str,
    seed_es: &'static str,
    seed_en: Option<&'static str>,
    /// Her body on the desktop until one is chosen: `spine` (her Spine 3.8 model from Ark-Models,
    /// downloaded on demand) or `pixel` (her pixel art in `ui/personajes/<id>/`). Both exist for
    /// every companion; `Local::cuerpo` chooses.
    pub body: &'static str,
    /// Her Spine 3.8 model in Ark-Models: its folder and its three files (name, bytes, SHA-256).
    pub spine: SpineModel,
    /// Her colour in the chat.
    pub color: &'static str,
    /// The voices of the catalog she speaks with until one is chosen (in order of preference).
    pub voice: &'static [&'static str],
}

/// A Spine 3.8 chibi in `isHarryh/Ark-Models` (`models/<id>/build_char_<id>.{atlas,png,skel}`).
#[derive(Debug, PartialEq, Eq)]
pub struct SpineModel {
    pub id: &'static str,
    pub files: [(&'static str, u64, &'static str); 3],
}

pub const KALTSIT: Character = Character {
    id: "kaltsit",
    name: "Kal'tsit",
    seed_es: include_str!("../seed/base_es.ron"),
    seed_en: Some(include_str!("../seed/base_en.ron")),
    body: "spine",
    spine: SpineModel {
        id: "003_kalts",
        files: [
            ("build_char_003_kalts.atlas", 8_872, "cc245ca899d25df397a1aab19cf359c9851849dad8db3f8264f5aef725dc0a0d"),
            ("build_char_003_kalts.png", 277_909, "e0a3f9fc65139520536e9a7b4c2ee03eb481277bb3ef591cb658a95ad00bcd53"),
            ("build_char_003_kalts.skel", 159_443, "3f946202a188aadadd1adc1436f89891c545b50a210c12f3ac3e7a40f5bc4070"),
        ],
    },
    color: "#9fd24a",
    voice: &["voz-kaltsit-en"],
};

pub const LAPPLAND: Character = Character {
    id: "lappland",
    name: "Lappland",
    seed_es: include_str!("../seed/lappland_es.ron"),
    // Her seed is only written in Spanish for now: an English install is born from it too.
    seed_en: None,
    body: "pixel",
    spine: SpineModel {
        id: "140_whitew",
        files: [
            ("build_char_140_whitew.atlas", 4_073, "9fce8660db00380497ec6fb96b5bfbcb52bbae37adf1b384ea7947f9f5c87a56"),
            ("build_char_140_whitew.png", 252_984, "4105a05c0b74c197343d8beb854c6abd5b1c4bde9a0ec49a02c83d44970aeb33"),
            ("build_char_140_whitew.skel", 120_415, "3148b1c26351561b54914b0434e75f1b3fd158c2f0a186bb89e5277c7127d085"),
        ],
    },
    color: "#b9c4d6",
    voice: &["voz-lappland-en", "voz-daniela"],
};

pub const ALL: [&Character; 2] = [&KALTSIT, &LAPPLAND];

pub fn get(id: &str) -> Option<&'static Character> {
    ALL.iter().copied().find(|c| c.id == id)
}

impl Character {
    /// Her compiled seed in `lang` ("en", anything else is Spanish).
    pub fn seed(&self, lang: &str) -> &'static str {
        if lang == "en" {
            self.seed_en.unwrap_or(self.seed_es)
        } else {
            self.seed_es
        }
    }

    /// The app's `data` from her memory folder (the inverse of `data_dir`).
    pub fn root_of(&self, dir: &Path) -> PathBuf {
        if self.id == KALTSIT.id {
            dir.to_path_buf()
        } else {
            dir.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf())
        }
    }

    /// A voice of the catalog cloned from another companion (her id in the item's id or in its
    /// reference clip): she never speaks with it.
    pub fn is_other_clone(&self, id: &str, item: Option<&crate::catalog::Item>) -> bool {
        let reference = item.and_then(|i| i.voz.as_ref()).and_then(|v| v.referencia.as_deref());
        ALL.iter().any(|c| c.id != self.id && (id.contains(c.id) || reference.is_some_and(|r| r.contains(c.id))))
    }

    /// Her memory folder under the app's `data`. Kal'tsit's stays `data` itself (where it always
    /// was); every other companion has her own beside it.
    pub fn data_dir(&self, data: &Path) -> PathBuf {
        if self.id == KALTSIT.id {
            data.to_path_buf()
        } else {
            data.join("personajes").join(self.id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_companion_has_a_seed_that_builds_her_memory() {
        for c in ALL {
            for lang in ["es", "en"] {
                let m = crate::cthg::seed::build_character(c, lang, "Ana").unwrap_or_else(|e| panic!("{} {lang}: {e}", c.id));
                assert!(m.periods[0].persona.contains(c.name), "{}: her persona names her", c.id);
                assert!(m.nodes.iter().filter(|n| n.has(crate::cthg::model::flags::CORE)).count() >= 9, "{}: her nine axioms", c.id);
                assert!(!m.periods[0].persona.contains("{user}") && m.periods[0].persona.contains("Ana"));
            }
        }
        assert!(!LAPPLAND.seed("es").contains("Kal'tsit, la jefa"), "Lappland's seed is hers");
    }

    /// A new install of Lappland is born from her seed, in her own folder, and knows whose it is;
    /// opened again, it is still hers.
    #[test]
    fn a_new_lappland_is_born_from_her_seed() {
        let d = std::env::temp_dir().join(format!("kaltsit-lappland-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let dir = LAPPLAND.data_dir(&d);
        let m = crate::mind::Mind::open_for(&dir, "modelo-de-prueba", &LAPPLAND).unwrap();
        assert_eq!(m.character().id, "lappland");
        assert_eq!(m.mem.meta("semilla"), Some("lappland_es"));
        assert!(m.mem.periods[0].persona.starts_with("Eres Lappland"));
        drop(m);
        let again = crate::mind::Mind::open(&dir, "modelo-de-prueba").unwrap();
        assert_eq!(again.character().id, "lappland", "whoever it was born as, however it is opened");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn each_has_her_own_memory_folder() {
        let d = Path::new("data");
        assert_eq!(KALTSIT.data_dir(d), Path::new("data"), "Kal'tsit's memory stays where it was");
        assert_eq!(LAPPLAND.data_dir(d), Path::new("data").join("personajes").join("lappland"));
        assert_eq!(get("lappland"), Some(&LAPPLAND));
        assert_eq!(get("nadie"), None);
    }

    #[test]
    fn companions_have_their_preferred_voices_and_do_not_take_each_others_clone() {
        assert_eq!(KALTSIT.voice, &["voz-kaltsit-en"]);
        assert_eq!(LAPPLAND.voice, &["voz-lappland-en", "voz-daniela"]);

        let m = crate::catalog::Manifest::load();
        assert!(m.item("voz-lappland-en").is_some(), "voz-lappland-en is in manifest");
        assert!(m.item("voz-kaltsit-en").is_some(), "voz-kaltsit-en is in manifest");

        let is_other_clone = |who: &str, id: &str, item| get(who).unwrap().is_other_clone(id, item);

        // For Lappland, Kal'tsit's voice is detected as other clone
        assert!(is_other_clone("lappland", "voz-kaltsit-en", m.item("voz-kaltsit-en")));
        assert!(!is_other_clone("lappland", "voz-lappland-en", m.item("voz-lappland-en")));
        assert!(!is_other_clone("lappland", "voz-daniela", m.item("voz-daniela")));

        // For Kal'tsit, Lappland's voice is detected as other clone
        assert!(is_other_clone("kaltsit", "voz-lappland-en", m.item("voz-lappland-en")));
        assert!(!is_other_clone("kaltsit", "voz-kaltsit-en", m.item("voz-kaltsit-en")));
    }
}
