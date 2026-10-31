//! What belongs to this computer and is never shared: its name and ports, the model it can run,
//! which computers sit to its left and right, its identity on the link and the computers it is
//! linked to, and how long the user has been idle here. Kept in `data/local.json` (ignored by
//! the memory repository).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Llama {
    /// llama-server.exe
    pub servidor: String,
    pub modelo: String,
    /// Vision projector (optional: without it she cannot see).
    pub mmproj: String,
    pub puerto: u16,
    /// Extra arguments after the model.
    pub args: Vec<String>,
}

impl Default for Llama {
    fn default() -> Self {
        let m = r"modelos";
        Llama {
            servidor: format!(r"{m}\bin\llama\llama-server.exe"),
            modelo: format!(r"{m}\llm\gemma-4-E4B-it-Q4_K_M.gguf"),
            mmproj: format!(r"{m}\mmproj\gemma-4-E4B-mmproj-F16.gguf"),
            puerto: 8081,
            args: ["-ngl", "99", "-c", "8192", "-np", "2", "-kvu", "--no-cache-idle-slots", "-fa", "on", "-ctk", "q8_0", "-ctv", "q8_0", "--jinja"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

/// Her voice on this computer (it speaks where her body is).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Voz {
    /// A voice of the catalog; empty: the first one downloaded.
    pub id: String,
    pub activa: bool,
    pub velocidad: f32,
}

impl Default for Voz {
    fn default() -> Self {
        Voz { id: String::new(), activa: true, velocidad: 1.0 }
    }
}

/// Where the accessories (game mods and other add-ons) come from and where they go.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Accesorios {
    /// GitHub repository ("owner/name") whose `catalog.json` lists them.
    pub repo: String,
    pub rama: String,
    /// Only for a private repository (read access to its contents). Never leaves this computer.
    pub token: String,
    /// Minecraft `mods` folders added by hand, besides the ones found on their own.
    pub carpetas: Vec<String>,
}

impl Default for Accesorios {
    fn default() -> Self {
        Accesorios { repo: "LuisLeon1705/Project-Kaltsit".into(), rama: "accessories".into(), token: String::new(), carpetas: Vec::new() }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Vecinos {
    /// Name of the computer whose screen is to the left of this one (empty: none).
    pub izquierda: String,
    pub derecha: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Local {
    /// How this computer is called on the network.
    pub nombre: String,
    /// HTTP port of the link between computers.
    pub puerto: u16,
    /// UDP port where computers announce themselves.
    pub descubrimiento: u16,
    pub llama: Llama,
    pub vecinos: Vecinos,
    /// Extra addresses to announce to ("192.168.1.20:47820"), when broadcast does not reach.
    pub pares: Vec<String>,
    /// The chat window shows up in screen captures (to share it or screenshot it). Her own
    /// eyes still never read it: they grey it out.
    pub chat_en_capturas: bool,
    pub voz: Voz,
    /// This computer's secret key on the link (X25519, hex). Created the first time; never shared.
    pub identidad: String,
    /// The computers it is linked to (accepted on both screens).
    pub vinculados: Vec<crate::net::Vinculo>,
    /// Let the link work on networks Windows marks as public (a café, a hotel).
    pub redes_publicas: bool,
    pub accesorios: Accesorios,
}

impl Default for Local {
    fn default() -> Self {
        Local {
            nombre: crate::origin::host(),
            puerto: 47821,
            descubrimiento: 47820,
            llama: Llama::default(),
            vecinos: Vecinos::default(),
            pares: Vec::new(),
            chat_en_capturas: false,
            voz: Voz::default(),
            identidad: String::new(),
            vinculados: Vec::new(),
            redes_publicas: false,
            accesorios: Accesorios::default(),
        }
    }
}

impl Local {
    pub fn path(data: &Path) -> PathBuf {
        data.join("local.json")
    }

    /// Reads `local.json`, writing the defaults the first time so they can be edited.
    pub fn load(data: &Path) -> Local {
        let p = Self::path(data);
        let (mut l, fresh) = match std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok()) {
            Some(l) => (l, false),
            None => (Local::default(), true),
        };
        let needs_identity = l.identidad.len() != 64;
        if needs_identity {
            l.identidad = crate::net::new_identity();
        }
        if fresh || needs_identity {
            l.save(data);
        }
        l
    }

    pub fn save(&self, data: &Path) {
        let _ = std::fs::create_dir_all(data);
        let _ = std::fs::write(Self::path(data), serde_json::to_string_pretty(self).unwrap_or_default());
    }

    /// Whether this computer has a model it could run (the files exist).
    pub fn can_think(&self) -> bool {
        Path::new(&self.llama.servidor).exists() && Path::new(&self.llama.modelo).exists()
    }
}

/// Seconds since the last keyboard or mouse input on this computer.
#[cfg(windows)]
pub fn idle_seconds() -> f64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    unsafe {
        let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if GetLastInputInfo(&mut li) == 0 {
            return 0.0;
        }
        GetTickCount().wrapping_sub(li.dwTime) as f64 / 1000.0
    }
}

#[cfg(not(windows))]
pub fn idle_seconds() -> f64 {
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_settings_and_identity_are_created_once() {
        let d = std::env::temp_dir().join(format!("kaltsit-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let l = Local::load(&d);
        assert_eq!(l.puerto, 47821);
        assert!(Local::path(&d).exists(), "defaults are written so they can be edited");
        assert_eq!(l.identidad.len(), 64);
        assert_eq!(Local::load(&d).identidad, l.identidad, "the identity is stable");
        assert!(!l.redes_publicas && l.vinculados.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}

