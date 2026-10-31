//! The model catalog (`models_manifest.json`): light and medium models for the two computers
//! she lives on, the voices for later, and the engine. Knows this computer's hardware, which
//! profile it is closest to, how much VRAM each model would take, what is installed, and how to
//! point `data/local.json` at a model. Shared by the app and the `cthg` command line.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The manifest shipped with the program; a `models_manifest.json` in the working folder wins.
const BUILTIN: &str = include_str!("../models_manifest.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Profile {
    pub id: String,
    pub nombre: String,
    pub vram_gb: f64,
    /// VRAM kept free for the desktop and other programs.
    pub reserva_gb: f64,
    #[serde(default)]
    pub nota: String,
    /// No dedicated GPU: the model runs on the CPU and `vram_gb` is the RAM it may use.
    #[serde(default)]
    pub cpu_only: bool,
}

/// Id of the profile built from the detected hardware ("this computer").
pub const HERE: &str = "aqui";
/// VRAM kept free for the desktop and other programs on a computer with a GPU.
pub const DESKTOP_RESERVE_GB: f64 = 1.0;

impl Profile {
    pub fn budget_gb(&self) -> f64 {
        self.vram_gb - self.reserva_gb
    }

    /// The profile of this computer, from what was detected: its GPU (VRAM minus the desktop
    /// reserve) or, without a dedicated GPU, its RAM (half of it, leaving 6 GB for Windows).
    pub fn from_hardware(hw: &Hardware) -> Profile {
        let ram = format!("{:.0} GB RAM", hw.ram_gb);
        if hw.has_gpu() {
            Profile { id: HERE.into(), nombre: format!("{} · {:.0} GB VRAM · {ram}", hw.gpu, hw.vram_gb * 1e9 / GIB), vram_gb: hw.vram_gb, reserva_gb: DESKTOP_RESERVE_GB, nota: String::new(), cpu_only: false }
        } else {
            let budget = cpu_budget_gb(hw.ram_gb);
            let cpu = if hw.cpu.is_empty() { "CPU" } else { hw.cpu.as_str() };
            Profile { id: HERE.into(), nombre: format!("{cpu} · {ram}"), vram_gb: hw.ram_gb, reserva_gb: hw.ram_gb - budget, nota: String::new(), cpu_only: true }
        }
    }
}

const GIB: f64 = 1_073_741_824.0;

/// RAM a CPU-only model may take: half of it, and at least 6 GB left for the system.
pub fn cpu_budget_gb(ram_gb: f64) -> f64 {
    (ram_gb * 0.5).min(ram_gb - 6.0).max(0.5)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileSpec {
    /// Empty for a file of the user's that is already on disk.
    #[serde(default)]
    pub url: String,
    /// Relative to the models folder (or absolute, for a file of the user's elsewhere).
    pub destino: String,
    pub bytes: u64,
    #[serde(default)]
    pub sha256: String,
    /// modelo | mmproj | voz | zip
    pub rol: String,
    /// For a zip: folder (relative to the models folder) to unpack it into.
    #[serde(default)]
    pub extraer: Option<String>,
    /// For an Ogg clip: the WAV (relative to the models folder) it becomes, 10 s at most.
    #[serde(default)]
    pub convertir: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Item {
    pub id: String,
    /// cerebro | voz | oido | motor
    pub tipo: String,
    /// ligero | medio | propio (added by the user)
    pub nivel: String,
    pub nombre: String,
    #[serde(default)]
    pub nota: String,
    #[serde(default)]
    pub ctx: Option<u32>,
    pub archivos: Vec<FileSpec>,
    /// For an engine: its main file (llama-server.exe, the voice library), relative to the
    /// models folder.
    #[serde(default)]
    pub servidor: Option<String>,
    /// For a voice: how sherpa-onnx loads it.
    #[serde(default)]
    pub voz: Option<VoiceSpec>,
    /// For an ear (speech recognition, dictation): how sherpa-onnx loads it.
    #[serde(default)]
    pub oido: Option<EarSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EarSpec {
    /// whisper (dictation) | voces (noise, silences and who is speaking: the fine ear)
    pub motor: String,
    /// The unpacked folder, relative to the models folder.
    pub carpeta: String,
}

/// The fine ear's speaker-segmentation model, inside its folder once unpacked.
pub const SEGMENTATION: &str = "sherpa-onnx-pyannote-segmentation-3-0/model.onnx";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VoiceSpec {
    /// vits (Piper) | kokoro
    pub motor: String,
    /// The unpacked folder, relative to the models folder.
    pub carpeta: String,
    /// Speaker id inside a multi-voice model (Kokoro's ef_dora is 28).
    #[serde(default)]
    pub sid: i32,
    /// Language hint for Kokoro ("es").
    #[serde(default)]
    pub lang: String,
    /// The language it speaks: "es" (default) or "en" (then she says an English version).
    #[serde(default)]
    pub idioma: String,
    /// For a cloning voice (Pocket TTS): the clip of the voice to imitate, relative to the
    /// models folder. Not downloaded with the catalog: prepared from her lines.
    #[serde(default)]
    pub referencia: Option<String>,
    /// Model-specific settings passed as they are (Pocket: temperature, reference length…).
    #[allow(dead_code)] // read by the app's voice, not by `cthg`
    #[serde(default)]
    pub ajustes: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pack {
    pub id: String,
    pub perfil: String,
    pub nombre: String,
    pub incluye: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub carpeta: String,
    pub perfiles: Vec<Profile>,
    pub elementos: Vec<Item>,
    pub packs: Vec<Pack>,
}

/// How a model sits in a profile's VRAM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Up to 85 % of the budget.
    Holgado,
    /// Up to 100 %: it runs, with little room for anything else.
    Justo,
    NoCabe,
}

impl Fit {
    pub fn name(self) -> &'static str {
        match self {
            Fit::Holgado => "holgado",
            Fit::Justo => "justo",
            Fit::NoCabe => "no cabe",
        }
    }
}

impl Manifest {
    pub fn parse(src: &str) -> Result<Manifest, String> {
        let m: Manifest = serde_json::from_str(src).map_err(|e| format!("models_manifest.json: {e}"))?;
        if m.version > 1 {
            return Err(crate::lang::tr(format!("models_manifest.json es de una versión más nueva ({}): actualiza el programa", m.version), format!("models_manifest.json is from a newer version ({}): update the program", m.version)));
        }
        for p in &m.packs {
            for id in &p.incluye {
                if m.item(id).is_none() {
                    return Err(crate::lang::tr(format!("el pack {} incluye {id}, que no está en el catálogo", p.id), format!("the pack {} includes {id}, which is not in the catalog", p.id)));
                }
            }
        }
        Ok(m)
    }

    /// The manifest in the working folder if there is one, else the built-in one; with the
    /// user's own models after it.
    pub fn load() -> Manifest {
        let mut m = std::fs::read_to_string("models_manifest.json").ok().and_then(|s| Manifest::parse(&s).ok()).unwrap_or_else(|| Manifest::parse(BUILTIN).expect("manifiesto incluido válido"));
        for i in load_own(&m.root()) {
            if m.item(&i.id).is_none() {
                m.elementos.push(i);
            }
        }
        m
    }

    pub fn root(&self) -> PathBuf {
        PathBuf::from(&self.carpeta)
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.elementos.iter().find(|i| i.id == id)
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.perfiles.iter().find(|p| p.id == id)
    }

    #[allow(dead_code)] // the app downloads a pack by its ids; `cthg` by its name
    pub fn pack(&self, id: &str) -> Option<&Pack> {
        self.packs.iter().find(|p| p.id == id)
    }

    /// The packs made for a profile.
    pub fn packs_for(&self, profile: &str) -> Vec<&Pack> {
        self.packs.iter().filter(|p| p.perfil == profile).collect()
    }

    /// The ONE brain model to suggest for a profile: the most capable that fits with room to
    /// spare, else the best that fits tightly, else the smallest one, flagged as slow. On a CPU
    /// only the smallest is suggested (and flagged): speed there goes before size. Models the
    /// user added are never suggested.
    pub fn recommend_one<'a>(&'a self, p: &Profile) -> Option<Pick<'a>> {
        let brains: Vec<(&Item, f64, Fit)> = self
            .elementos
            .iter()
            .filter(|i| i.tipo == "cerebro" && i.nivel != "propio")
            .map(|i| {
                let gb = i.vram_gb();
                (i, gb, fit(gb, p))
            })
            .collect();
        let smallest = brains.iter().min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())?;
        let pick = |c: &(&'a Item, f64, Fit), slow: bool, reason: &'static str| Pick { item: c.0, gb: c.1, fit: c.2, slow, reason };
        if p.cpu_only {
            return Some(pick(smallest, true, "cpu"));
        }
        for (want, reason) in [(Fit::Holgado, "roomy"), (Fit::Justo, "tight")] {
            let best = brains.iter().filter(|c| c.2 == want).fold(None, |best: Option<&(&Item, f64, Fit)>, c| match best {
                Some(b) if !more_capable(c, b) => Some(b),
                _ => Some(c),
            });
            if let Some(b) = best {
                return Some(pick(b, false, reason));
            }
        }
        Some(pick(smallest, true, "tiny"))
    }
}

fn level_rank(n: &str) -> u8 {
    if n == "ligero" { 0 } else { 1 }
}

/// What `Manifest::recommend_one` suggests.
pub struct Pick<'a> {
    pub item: &'a Item,
    /// Estimated VRAM (RAM on a CPU) it takes, GB.
    pub gb: f64,
    pub fit: Fit,
    /// It will run slowly here (on the CPU, or too big for the GPU).
    pub slow: bool,
    /// roomy | tight | tiny | cpu: the UI's text for why.
    pub reason: &'static str,
}

/// `a` is a better pick than `b`: a higher level, else a bigger model. Models within 50 MB of
/// each other tie, and then the one that can see (it has a vision projector) wins.
fn more_capable(a: &(&Item, f64, Fit), b: &(&Item, f64, Fit)) -> bool {
    let (ra, rb) = (level_rank(&a.0.nivel), level_rank(&b.0.nivel));
    if ra != rb {
        return ra > rb;
    }
    if (a.1 - b.1).abs() < 0.05 {
        return a.0.file("mmproj").is_some() && b.0.file("mmproj").is_none();
    }
    a.1 > b.1
}

impl Item {
    pub fn download_bytes(&self) -> u64 {
        self.archivos.iter().map(|f| f.bytes).sum()
    }

    /// VRAM estimate (GB): weights and vision projector, plus the KV cache (q8, two slots
    /// sharing the context) and compute buffers, which grow with the context.
    pub fn vram_gb(&self) -> f64 {
        let weights: u64 = self.archivos.iter().filter(|f| f.rol == "modelo" || f.rol == "mmproj").map(|f| f.bytes).sum();
        estimate_gb(weights, self.ctx.unwrap_or(8192))
    }

    pub fn file(&self, rol: &str) -> Option<&FileSpec> {
        self.archivos.iter().find(|f| f.rol == rol)
    }

    /// Everything of this item is in the models folder (an engine or a voice: unpacked).
    pub fn installed(&self, root: &Path) -> bool {
        if self.servidor.is_some() || self.voz.is_some() || self.oido.is_some() {
            return self.unpacked(root);
        }
        self.archivos.iter().all(|f| std::fs::metadata(root.join(&f.destino)).map(|m| m.len() == f.bytes).unwrap_or(false))
    }

    /// What comes out of its archive is there (true for items without one).
    pub fn unpacked(&self, root: &Path) -> bool {
        if let Some(s) = &self.servidor {
            return root.join(s).exists();
        }
        if let Some(v) = &self.voz {
            let dir = root.join(&v.carpeta);
            let model = dir.join("tokens.txt").exists() || dir.join("vocab.json").exists();
            return model && v.referencia.as_ref().is_none_or(|r| root.join(r).exists());
        }
        if let Some(o) = &self.oido {
            // The fine ear is loose files plus the segmentation archive, unpacked.
            if o.motor == "voces" {
                let files = self.archivos.iter().filter(|f| f.extraer.is_none()).all(|f| root.join(&f.destino).exists());
                return files && root.join(&o.carpeta).join(SEGMENTATION).exists();
            }
            return std::fs::read_dir(root.join(&o.carpeta)).is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().ends_with("tokens.txt")));
        }
        true
    }
}

// ── the user's own models ──

/// File (in the models folder) with the models the user added: same shape as the catalog's.
const OWN: &str = "propios.json";

/// The brain models the user added, by link or from a file on disk.
pub fn load_own(root: &Path) -> Vec<Item> {
    std::fs::read_to_string(root.join(OWN)).ok().and_then(|s| serde_json::from_str::<Vec<Item>>(&s).ok()).unwrap_or_default().into_iter().filter(|i| i.tipo == "cerebro" && i.file("modelo").is_some()).collect()
}

#[allow(dead_code)] // the app adds them; `cthg` only lists them
pub fn save_own(root: &Path, items: &[Item]) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    std::fs::write(root.join(OWN), serde_json::to_string_pretty(items).unwrap_or_default()).map_err(|e| e.to_string())
}

/// VRAM a model takes, roughly: weights and projector, 0.35 GB of buffers and 0.07 GB per 1024
/// tokens of context. On the safe side (a q8_0 cache takes less).
pub fn estimate_gb(weights_bytes: u64, ctx: u32) -> f64 {
    weights_bytes as f64 / 1e9 + 0.35 + 0.07 * ctx as f64 / 1024.0
}

/// VRAM in use and in total right now, in GB (nvidia-smi).
pub fn vram_now() -> Option<(f64, f64)> {
    let s = quiet("nvidia-smi", &["--query-gpu=memory.used,memory.total", "--format=csv,noheader,nounits"])?;
    let (u, t) = s.lines().next()?.split_once(',')?;
    let gb = |v: &str| v.trim().parse::<f64>().ok().map(|m| m * 1_048_576.0 / 1e9);
    Some((gb(u)?, gb(t)?))
}

pub fn fit(gb: f64, p: &Profile) -> Fit {
    let budget = p.budget_gb();
    if budget <= 0.0 {
        return Fit::NoCabe;
    }
    let r = gb / budget;
    if r <= 0.85 {
        Fit::Holgado
    } else if r <= 1.0 {
        Fit::Justo
    } else {
        Fit::NoCabe
    }
}

// ── this computer ──

#[derive(Clone, Debug, Default)]
pub struct Hardware {
    /// The dedicated GPU with the most memory; empty when there is none.
    pub gpu: String,
    /// nvidia | amd | intel | other; empty without a dedicated GPU.
    pub vendor: String,
    /// Dedicated video memory, GB (bytes / 1e9).
    pub vram_gb: f64,
    pub ram_gb: f64,
    pub cpu: String,
}

impl Hardware {
    /// There is a dedicated GPU worth running a model on.
    pub fn has_gpu(&self) -> bool {
        self.vram_gb > 0.0
    }
}

/// A display adapter that can hold a model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    pub name: String,
    pub bytes: u64,
    pub vendor: &'static str,
}

/// Below this an adapter shares the system's memory: no dedicated GPU.
const MIN_DEDICATED_BYTES: u64 = 1_000_000_000;

pub fn vendor_of(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("quadro") || n.contains("tesla") {
        "nvidia"
    } else if n.contains("amd") || n.contains("radeon") || n.contains("ati ") {
        "amd"
    } else if n.contains("intel") {
        "intel"
    } else {
        "other"
    }
}

/// An adapter that draws on the system's RAM or is not a real card (Intel graphics other than
/// Arc, Microsoft's basic and remote drivers, virtual machines), or too small to hold a model.
pub fn is_integrated(name: &str, bytes: u64) -> bool {
    let n = name.to_lowercase();
    let intel_shared = n.contains("intel") && !n.contains("arc");
    let not_a_card = ["microsoft basic", "basic render", "basic display", "remote display", "hyper-v", "vmware", "virtualbox", "virtual display", "parsec"].iter().any(|s| n.contains(s));
    intel_shared || not_a_card || bytes < MIN_DEDICATED_BYTES
}

/// The adapter a model should run on, from (name, dedicated bytes) pairs: the dedicated one with
/// the most memory. `None` when every adapter is integrated.
pub fn pick_gpu(adapters: &[(String, u64)]) -> Option<Gpu> {
    adapters
        .iter()
        .filter(|(name, bytes)| !is_integrated(name, *bytes))
        .max_by_key(|(_, bytes)| *bytes)
        .map(|(name, bytes)| Gpu { name: name.clone(), bytes: *bytes, vendor: vendor_of(name) })
}

// Registry value types.
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;
const REG_QWORD: u32 = 11;

/// Video memory in bytes from a driver's registry value: `HardwareInformation.qwMemorySize`
/// (QWORD) or the older `HardwareInformation.MemorySize` (DWORD or little-endian binary).
pub fn decode_memory(kind: u32, data: &[u8]) -> Option<u64> {
    let ok = match kind {
        REG_QWORD => data.len() >= 8,
        REG_DWORD => data.len() >= 4,
        REG_BINARY => !data.is_empty(),
        _ => false,
    };
    if !ok {
        return None;
    }
    let mut le = [0u8; 8];
    let n = data.len().min(if kind == REG_DWORD { 4 } else { 8 });
    le[..n].copy_from_slice(&data[..n]);
    Some(u64::from_le_bytes(le))
}

/// A REG_SZ value (UTF-16, maybe NUL-terminated) as text.
pub fn decode_string(data: &[u8]) -> String {
    let wide: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
    String::from_utf16_lossy(&wide).trim().to_string()
}

/// The display adapters Windows knows (class key `{4d36e968-…}`), as (name, dedicated bytes).
/// Read with the registry API directly: no process to start, no text to parse, and binary
/// values come back as they are.
#[cfg(windows)]
fn registry_adapters() -> Vec<(String, u64)> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ};

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn open(parent: HKEY, path: &str) -> Option<Key> {
        let mut h: HKEY = null_mut();
        (unsafe { RegOpenKeyExW(parent, wide(path).as_ptr(), 0, KEY_READ, &mut h) } == 0).then_some(Key(h))
    }
    fn value(k: &Key, name: &str) -> Option<(u32, Vec<u8>)> {
        let n = wide(name);
        let (mut kind, mut len) = (0u32, 0u32);
        if unsafe { RegQueryValueExW(k.0, n.as_ptr(), null(), &mut kind, null_mut(), &mut len) } != 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        (unsafe { RegQueryValueExW(k.0, n.as_ptr(), null(), &mut kind, buf.as_mut_ptr(), &mut len) } == 0).then(|| {
            buf.truncate(len as usize);
            (kind, buf)
        })
    }

    let Some(class) = open(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}") else { return Vec::new() };
    let mut out = Vec::new();
    for i in 0u32.. {
        let mut name = [0u16; 256];
        let mut len = name.len() as u32;
        if unsafe { RegEnumKeyExW(class.0, i, name.as_mut_ptr(), &mut len, null(), null_mut(), null_mut(), null_mut()) } != 0 {
            break;
        }
        let sub = String::from_utf16_lossy(&name[..len as usize]);
        // The adapters are 0000, 0001…; "Configuration" and "Properties" sit beside them.
        if sub.len() != 4 || !sub.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Some(k) = open(class.0, &sub) else { continue };
        let Some((_, desc)) = value(&k, "DriverDesc") else { continue };
        let bytes = value(&k, "HardwareInformation.qwMemorySize")
            .and_then(|(t, d)| decode_memory(t, &d))
            .or_else(|| value(&k, "HardwareInformation.MemorySize").and_then(|(t, d)| decode_memory(t, &d)))
            .unwrap_or(0);
        let desc = decode_string(&desc);
        if !desc.is_empty() {
            out.push((desc, bytes));
        }
    }
    out
}

#[cfg(not(windows))]
fn registry_adapters() -> Vec<(String, u64)> {
    Vec::new()
}

/// NVIDIA cards with the exact memory nvidia-smi reports (the registry rounds a few of them).
fn nvidia_adapters() -> Vec<(String, u64)> {
    let Some(s) = quiet("nvidia-smi", &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]) else { return Vec::new() };
    s.lines()
        .filter_map(|l| {
            let (name, mib) = l.split_once(',')?;
            Some((name.trim().to_string(), mib.trim().parse::<u64>().ok()? * 1_048_576))
        })
        .collect()
}

fn quiet(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// GPU and VRAM from the display adapters in the registry (any vendor), with nvidia-smi's exact
/// value for NVIDIA cards; RAM and CPU from Windows.
pub fn hardware() -> Hardware {
    let mut h = Hardware::default();
    let mut adapters = registry_adapters();
    let nvidia = nvidia_adapters();
    if !nvidia.is_empty() {
        adapters.retain(|(name, _)| vendor_of(name) != "nvidia");
        adapters.extend(nvidia);
    }
    if let Some(g) = pick_gpu(&adapters) {
        h.vram_gb = g.bytes as f64 / 1e9;
        h.vendor = g.vendor.to_string();
        h.gpu = g.name;
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            // Sold (and shown by Windows) in GiB: 32 GB, not 34.
            h.ram_gb = m.ullTotalPhys as f64 / 1_073_741_824.0;
        }
    }
    if let Some(s) = quiet("reg", &["query", r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0", "/v", "ProcessorNameString"]) {
        h.cpu = s.lines().find_map(|l| l.split("REG_SZ").nth(1)).map(|c| c.trim().to_string()).unwrap_or_default();
    }
    h
}

// ── pointing local.json at a model ──

fn rel(root: &Path, dest: &str) -> String {
    root.join(dest).to_string_lossy().replace('/', "\\")
}

/// Points `local.json` (`data`) at a brain model of the catalog: its weights, its projector and
/// its context; and at the downloaded engine if the configured one does not exist.
pub fn use_item(m: &Manifest, id: &str, data: &Path) -> Result<Value, String> {
    let item = m.item(id).ok_or_else(|| crate::lang::tr(format!("no hay ningún «{id}» en el catálogo"), format!("there is no «{id}» in the catalog")))?;
    if item.tipo != "cerebro" {
        return Err(crate::lang::tr(format!("{} no es un modelo de cerebro", item.nombre), format!("{} is not a brain model", item.nombre)));
    }
    let root = m.root();
    if !item.installed(&root) {
        return Err(crate::lang::tr(format!("{} no está descargado: cthg models download {id}", item.nombre), format!("{} is not downloaded: cthg models download {id}", item.nombre)));
    }
    let path = data.join("local.json");
    let mut v: Value = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}));
    if !v["llama"].is_object() {
        v["llama"] = json!({});
    }
    let l = &mut v["llama"];
    l["modelo"] = json!(rel(&root, &item.file("modelo").ok_or_else(|| crate::lang::tr("sin archivo de modelo", "no model file"))?.destino));
    l["mmproj"] = json!(item.file("mmproj").map(|f| rel(&root, &f.destino)).unwrap_or_default());
    // The context goes in the arguments; the rest of them stay as they were.
    let ctx = item.ctx.unwrap_or(8192).to_string();
    let mut args: Vec<String> = l["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    match args.iter().position(|a| a == "-c" || a == "--ctx-size") {
        Some(i) if i + 1 < args.len() => args[i + 1] = ctx,
        _ => args.extend(["-c".to_string(), ctx]),
    }
    l["args"] = json!(args);
    let server_ok = l["servidor"].as_str().is_some_and(|s| Path::new(s).exists());
    if !server_ok {
        if let Some(engine) = m.elementos.iter().find(|i| i.tipo == "motor" && i.installed(&root)) {
            l["servidor"] = json!(rel(&root, engine.servidor.as_deref().unwrap_or_default()));
        }
    }
    std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
    std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_is_valid_and_only_light_or_medium() {
        let m = Manifest::parse(BUILTIN).unwrap();
        assert!(m.elementos.iter().all(|i| i.nivel == "ligero" || i.nivel == "medio"), "nothing heavy");
        for i in &m.elementos {
            for f in &i.archivos {
                assert!(f.url.starts_with("https://"), "{}", f.url);
                assert!(f.bytes > 0);
                assert!(f.sha256.is_empty() || f.sha256.len() == 64, "{} sha256", f.destino);
            }
            if i.tipo == "cerebro" {
                assert!(i.file("modelo").is_some() && i.file("mmproj").is_some(), "{}: she needs to see", i.id);
            }
        }
        assert!(m.pack("ligero-gpu-6gb").unwrap().incluye.contains(&"llama-b11633-cuda12".to_string()), "a stranger's computer gets the engine");
    }

    #[test]
    fn each_computer_gets_what_fits_it() {
        let m = Manifest::parse(BUILTIN).unwrap();
        let desk = m.profile("gpu-8gb").unwrap();
        let laptop = m.profile("gpu-6gb").unwrap();
        let fits = |id: &str, p: &Profile| fit(m.item(id).unwrap().vram_gb(), p) != Fit::NoCabe;
        assert!(fits("qwen35-9b-iq4", desk) && fits("gemma4-e4b-q4", desk));
        assert!(!fits("qwen35-9b-iq4", laptop) && !fits("gemma4-e4b-q4", laptop), "too big for 6 GB");
        assert!(fits("qwen35-4b-q4", laptop) && fits("qwen35-4b-q6", laptop));
        // Every pack's brain fits its own profile.
        for p in &m.packs {
            let prof = m.profile(&p.perfil).unwrap();
            for id in &p.incluye {
                let i = m.item(id).unwrap();
                if i.tipo == "cerebro" {
                    assert_ne!(fit(i.vram_gb(), prof), Fit::NoCabe, "{} in {}", i.id, p.id);
                }
            }
        }
    }

    fn adapters(list: &[(&str, u64)]) -> Vec<(String, u64)> {
        list.iter().map(|(n, b)| (n.to_string(), *b)).collect()
    }
    const GIB8: u64 = 8 * 1_073_741_824;

    #[test]
    fn the_dedicated_adapter_with_the_most_memory_wins() {
        let nv = pick_gpu(&adapters(&[("Intel(R) UHD Graphics 630", 1_073_741_824), ("NVIDIA GeForce RTX 3070", GIB8)])).unwrap();
        assert_eq!((nv.name.as_str(), nv.vendor), ("NVIDIA GeForce RTX 3070", "nvidia"));
        let amd = pick_gpu(&adapters(&[("AMD Radeon RX 6600", GIB8)])).unwrap();
        assert_eq!((amd.name.as_str(), amd.vendor, amd.bytes), ("AMD Radeon RX 6600", "amd", GIB8));
        // The roomier of two cards.
        let two = pick_gpu(&adapters(&[("NVIDIA GeForce GTX 1650", 4 * 1_073_741_824), ("AMD Radeon RX 7900 XT", 20 * 1_073_741_824)])).unwrap();
        assert_eq!(two.vendor, "amd");
        let arc = pick_gpu(&adapters(&[("Intel(R) Arc(TM) A750 Graphics", 8 * 1_073_741_824)])).unwrap();
        assert_eq!(arc.vendor, "intel", "Arc is a real card");
    }

    #[test]
    fn integrated_adapters_count_as_no_gpu() {
        assert!(pick_gpu(&adapters(&[("Intel(R) UHD Graphics", 1_073_741_824)])).is_none(), "Intel graphics share the RAM");
        assert!(pick_gpu(&adapters(&[("Intel(R) Iris(R) Xe Graphics", 2 * 1_073_741_824)])).is_none());
        assert!(pick_gpu(&adapters(&[("Microsoft Basic Display Adapter", 0)])).is_none());
        assert!(pick_gpu(&adapters(&[("AMD Radeon(TM) Graphics", 536_870_912)])).is_none(), "an APU with a tiny frame buffer");
        assert!(pick_gpu(&adapters(&[("NVIDIA GeForce GT 710", 512 * 1_048_576)])).is_none(), "under 1 GB holds nothing");
        assert!(pick_gpu(&[]).is_none());
    }

    #[test]
    fn the_registry_memory_value_is_a_qword_a_dword_or_bytes() {
        // qwMemorySize: 8 GiB as a QWORD.
        assert_eq!(decode_memory(11, &GIB8.to_le_bytes()), Some(GIB8));
        // MemorySize as a DWORD (2 GiB), and as binary (the driver writes 4 bytes or 8).
        assert_eq!(decode_memory(4, &0x8000_0000u32.to_le_bytes()), Some(0x8000_0000));
        assert_eq!(decode_memory(3, &0x4000_0000u32.to_le_bytes()), Some(0x4000_0000));
        assert_eq!(decode_memory(3, &GIB8.to_le_bytes()), Some(GIB8));
        // Not a number, or too short for its type.
        assert_eq!(decode_memory(1, b"8 GB"), None);
        assert_eq!(decode_memory(11, &[1, 2, 3, 4]), None);
        assert_eq!(decode_memory(3, &[]), None);
        // A string value: UTF-16, NUL-terminated.
        let sz: Vec<u8> = "AMD Radeon RX 6600\0".encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        assert_eq!(decode_string(&sz), "AMD Radeon RX 6600");
    }

    fn hw(gpu: &str, vram_gb: f64, ram_gb: f64) -> Hardware {
        Hardware { gpu: gpu.into(), vendor: if gpu.is_empty() { "" } else { vendor_of(gpu) }.into(), vram_gb, ram_gb, cpu: "AMD Ryzen 5 5600H".into() }
    }

    #[test]
    fn this_computers_profile_comes_from_its_hardware() {
        let p = Profile::from_hardware(&hw("NVIDIA GeForce RTX 3070", 8.59, 32.0));
        assert_eq!((p.id.as_str(), p.cpu_only), (HERE, false));
        assert!((p.budget_gb() - 7.59).abs() < 1e-9, "VRAM minus the 1 GB desktop reserve");
        assert!(p.nombre.contains("RTX 3070") && p.nombre.contains("8 GB VRAM") && p.nombre.contains("32 GB RAM"), "{}", p.nombre);
        // No GPU: half the RAM, but 6 GB stay for the system.
        for (ram, budget) in [(32.0, 16.0), (16.0, 8.0), (8.0, 2.0), (4.0, 0.5)] {
            let c = Profile::from_hardware(&hw("", 0.0, ram));
            assert!(c.cpu_only, "{ram}");
            assert!((c.budget_gb() - budget).abs() < 1e-9, "{ram} GB RAM -> {}", c.budget_gb());
        }
        assert!(Profile::from_hardware(&hw("", 0.0, 16.0)).nombre.contains("Ryzen 5 5600H"));
        // The budget is what fit() judges against.
        let m = Manifest::parse(BUILTIN).unwrap();
        let small = Profile::from_hardware(&hw("", 0.0, 8.0));
        assert_eq!(fit(m.item("qwen35-4b-q4").unwrap().vram_gb(), &small), Fit::NoCabe, "4.3 GB in a 2 GB budget");
        assert_eq!(fit(1.0, &Profile::from_hardware(&hw("", 0.0, 0.0))), Fit::NoCabe, "nothing detected is never roomy");
    }

    fn tier(m: &Manifest, id: &str) -> Pick<'static> {
        // Lifetimes: the manifest outlives the test body; the pick is only read inside it.
        let p = m.profile(id).unwrap();
        let pick = m.recommend_one(p).unwrap();
        unsafe { std::mem::transmute::<Pick<'_>, Pick<'static>>(pick) }
    }

    #[test]
    fn each_tier_gets_a_bigger_model_and_the_cpu_the_smallest() {
        let m = Manifest::parse(BUILTIN).unwrap();
        let cpu = tier(&m, "cpu");
        let smallest = m.elementos.iter().filter(|i| i.tipo == "cerebro").map(|i| i.vram_gb()).fold(f64::MAX, f64::min);
        assert!((cpu.gb - smallest).abs() < 1e-9 && cpu.slow && cpu.reason == "cpu", "the CPU gets the smallest, slow");
        assert_eq!(cpu.item.id, "qwen35-2b-q4");
        let (g4, g6, g8, g12, g16) = (tier(&m, "gpu-4gb"), tier(&m, "gpu-6gb"), tier(&m, "gpu-8gb"), tier(&m, "gpu-12gb"), tier(&m, "gpu-16gb"));
        assert!(!g4.slow && g4.reason == "roomy" && g4.fit == Fit::Holgado && g4.item.id == "qwen35-2b-q4", "4 GB fits the 2B Q4 with room");
        assert!(!g6.slow && g6.fit == Fit::Holgado && g6.reason == "roomy");
        assert!(!g8.slow && g8.fit == Fit::Holgado && !g12.slow && !g16.slow);
        assert!(g4.gb < g6.gb && g6.gb < g8.gb && g8.gb < g16.gb, "{} {} {} {}", g4.gb, g6.gb, g8.gb, g16.gb);
        assert!(g8.gb <= g12.gb && g12.gb <= g16.gb);
        assert_eq!(g12.item.id, g16.item.id);
        // A card too small even for the 2B falls back to the smallest, flagged as slow.
        let tiny = Profile { id: "gpu-2gb".into(), nombre: "2 GB".into(), vram_gb: 2.0, reserva_gb: 0.5, nota: String::new(), cpu_only: false };
        let t = m.recommend_one(&tiny).unwrap();
        assert!(t.slow && t.reason == "tiny" && t.fit == Fit::NoCabe && t.item.id == "qwen35-2b-q4");
    }

    #[test]
    fn a_tight_fit_is_picked_only_when_nothing_has_room() {
        let m = Manifest::parse(BUILTIN).unwrap();
        // 5.5 GB budget: the 4B Q4 (4.3 GB) has room, the E2B (5.0) and 4B Q6 (5.1) are tight.
        let mut p = m.profile("gpu-6gb").unwrap().clone();
        p.reserva_gb = p.vram_gb - 5.5;
        assert_eq!(m.recommend_one(&p).unwrap().item.id, "qwen35-4b-q4");
        // 3.0 GB budget: the 2B Q4 (2.8 GB) is 0.93 (> 0.85), so only a tight fit is left.
        p.reserva_gb = p.vram_gb - 3.0;
        let k = m.recommend_one(&p).unwrap();
        assert_eq!((k.fit, k.reason, k.slow), (Fit::Justo, "tight", false));
        assert_eq!(k.item.id, "qwen35-2b-q4");
    }

    #[test]
    fn vision_wins_a_tie_but_size_wins_otherwise() {
        let mut m = Manifest::parse(BUILTIN).unwrap();
        m.elementos.retain(|i| i.tipo != "cerebro");
        let file = |rol: &str, bytes: u64| FileSpec { url: "https://x".into(), destino: format!("{rol}.gguf"), bytes, sha256: String::new(), rol: rol.into(), extraer: None, convertir: None };
        let brain = |id: &str, files: Vec<FileSpec>| Item { id: id.into(), tipo: "cerebro".into(), nivel: "medio".into(), nombre: id.into(), nota: String::new(), ctx: Some(8192), archivos: files, servidor: None, voz: None, oido: None };
        // Same estimated size: 4 GB + 1 GB of projector against 5 GB of weights alone.
        m.elementos.push(brain("blind", vec![file("modelo", 5_000_000_000)]));
        m.elementos.push(brain("sees", vec![file("modelo", 4_000_000_000), file("mmproj", 1_000_000_000)]));
        let p = Profile { id: "t".into(), nombre: "t".into(), vram_gb: 16.0, reserva_gb: 1.0, nota: String::new(), cpu_only: false };
        assert_eq!(m.recommend_one(&p).unwrap().item.id, "sees");
        // A clearly bigger one beats vision.
        m.elementos.push(brain("bigger", vec![file("modelo", 6_000_000_000)]));
        assert_eq!(m.recommend_one(&p).unwrap().item.id, "bigger");
        // The user's own models are never suggested.
        m.elementos.push(Item { nivel: "propio".into(), ..brain("mine", vec![file("modelo", 7_000_000_000)]) });
        assert_eq!(m.recommend_one(&p).unwrap().item.id, "bigger");
    }

    #[test]
    fn the_manifest_has_the_generic_tiers_and_no_machine_of_the_owner() {
        let m = Manifest::parse(BUILTIN).unwrap();
        let ids: Vec<&str> = m.perfiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["cpu", "gpu-4gb", "gpu-6gb", "gpu-8gb", "gpu-12gb", "gpu-16gb"]);
        assert!(m.profile("cpu").unwrap().cpu_only && m.perfiles.iter().filter(|p| p.cpu_only).count() == 1);
        for p in &m.perfiles {
            assert!(p.budget_gb() > 0.0, "{}", p.id);
        }
        assert!(!BUILTIN.contains("rtx3070") && !BUILTIN.contains("rtx3060"), "the owner's machines are not in the shipped data");
    }

    /// The UI shows every profile, pack, brain model and note by id (`cat.name.<id>`,
    /// `cat.note.<id>`): each needs its Spanish and English text in the dictionary.
    #[test]
    fn every_profile_pack_and_model_has_its_texts_in_both_languages() {
        let js = include_str!("../ui/i18n.js");
        let (es, en) = js.split_once("const I18N_EN").expect("the English dictionary");
        let m = Manifest::parse(BUILTIN).unwrap();
        let has = |dict: &str, key: String| dict.contains(&format!("\n  \"{key}\":"));
        let mut missing = Vec::new();
        let mut need = |key: String| {
            for (lang, dict) in [("es", es), ("en", en)] {
                if !has(dict, key.clone()) {
                    missing.push(format!("{lang}:{key}"));
                }
            }
        };
        for p in &m.perfiles {
            need(format!("cat.name.{}", p.id));
            need(format!("cat.note.{}", p.id));
        }
        for p in &m.packs {
            need(format!("cat.name.{}", p.id));
        }
        for i in &m.elementos {
            need(format!("cat.note.{}", i.id));
            if i.tipo == "cerebro" {
                need(format!("cat.name.{}", i.id));
            }
        }
        assert!(missing.is_empty(), "missing in ui/i18n.js: {missing:?}");
    }

    /// Where this machine's hardware shows up: `cargo test --bin kaltsit this_machine -- --ignored --exact catalog::tests::this_machine_hardware --nocapture`
    #[test]
    #[ignore]
    fn this_machine_hardware() {
        let h = hardware();
        let p = Profile::from_hardware(&h);
        println!("registry adapters: {:?}", registry_adapters());
        println!("nvidia-smi: {:?}", nvidia_adapters());
        println!("hardware: {h:?}");
        println!("profile: {} budget {:.2} cpu_only {}", p.nombre, p.budget_gb(), p.cpu_only);
        let m = Manifest::parse(BUILTIN).unwrap();
        if let Some(k) = m.recommend_one(&p) {
            println!("recommends {} ({:.1} GB, {}, slow {}, {})", k.item.id, k.gb, k.fit.name(), k.slow, k.reason);
        }
    }

    #[test]
    fn using_a_model_points_local_json_at_it() {
        let d = std::env::temp_dir().join(format!("kaltsit-catalog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut m = Manifest::parse(BUILTIN).unwrap();
        m.carpeta = d.join("modelos").to_string_lossy().to_string();
        let root = m.root();
        assert!(use_item(&m, "qwen35-4b-q4", &d.join("data")).unwrap_err().contains("no está descargado"));
        // Pretend it was downloaded (right sizes) — sparse files are enough.
        for f in &m.item("qwen35-4b-q4").unwrap().archivos {
            let p = root.join(&f.destino);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::File::create(&p).unwrap().set_len(f.bytes).unwrap();
        }
        std::fs::create_dir_all(d.join("data")).unwrap();
        std::fs::write(d.join("data/local.json"), r#"{"nombre":"LAPTOP","llama":{"servidor":"no-existe.exe","args":["-ngl","99","-c","8192"]}}"#).unwrap();
        let v = use_item(&m, "qwen35-4b-q4", &d.join("data")).unwrap();
        assert!(v["llama"]["modelo"].as_str().unwrap().ends_with("Qwen3.5-4B-Q4_K_M.gguf"));
        assert!(v["llama"]["mmproj"].as_str().unwrap().ends_with("Qwen3.5-4B-mmproj-F16.gguf"));
        assert_eq!(v["nombre"], "LAPTOP", "the rest of the settings stay");
        assert_eq!(v["llama"]["args"], json!(["-ngl", "99", "-c", "8192"]));
        assert!(m.item("gemma4-e4b-q3").unwrap().ctx == Some(4096));
        let _ = std::fs::remove_dir_all(&d);
    }
}
