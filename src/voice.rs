//! Her voice: sherpa-onnx's C library (`sherpa-onnx-c-api.dll`, downloaded with the catalog),
//! loaded at run time, with a Piper (VITS) or Kokoro voice. The text goes in as UTF-8 through
//! the C API — the command-line tool would get it through Windows' ANSI code page and lose the
//! accents. The voice stays loaded between messages; a reply is synthesized sentence by sentence
//! and each one plays (waveOut, from memory) while the next one is being prepared.
//!
//! She only speaks out loud to answer a message, once the whole reply is written.

use crate::catalog::Manifest;
use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

/// What to load: the library, the voice's folder and how to drive it.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub dll: PathBuf,
    /// vits | kokoro | pocket
    pub motor: String,
    pub dir: PathBuf,
    pub sid: i32,
    pub lang: String,
    pub speed: f32,
    /// The language it speaks ("es", "en").
    pub idioma: String,
    /// Pocket TTS: the clip whose voice it takes.
    pub reference: Option<PathBuf>,
    /// Pocket TTS: its JSON settings.
    pub extra: String,
}

/// The voice `id` of the catalog, with the downloaded engine. Errors say what is missing.
pub fn setup(m: &Manifest, id: &str, speed: f32) -> Result<Setup, String> {
    let root = m.root();
    let item = m.item(id).ok_or_else(|| crate::lang::tr(format!("no hay ninguna voz «{id}» en el catálogo"), format!("there is no voice «{id}» in the catalog")))?;
    let spec = item.voz.as_ref().ok_or_else(|| crate::lang::tr(format!("{} no es una voz", item.nombre), format!("{} is not a voice", item.nombre)))?;
    if !item.installed(&root) {
        return Err(crate::lang::tr(format!("{} no está descargada (Modelos → Voz)", item.nombre), format!("{} is not downloaded (Models → Voice)", item.nombre)));
    }
    let engine = m
        .elementos
        .iter()
        .filter(|i| i.servidor.as_deref().is_some_and(|s| s.ends_with(".dll")))
        .find(|i| i.installed(&root))
        .ok_or_else(|| crate::lang::tr("falta el motor de voz (Modelos → sherpa-onnx)", "the voice engine is missing (Models → sherpa-onnx)"))?;
    Ok(Setup {
        dll: root.join(engine.servidor.as_deref().unwrap_or_default()),
        motor: spec.motor.clone(),
        dir: root.join(&spec.carpeta),
        sid: spec.sid,
        lang: spec.lang.clone(),
        speed: if speed > 0.0 { speed } else { 1.0 },
        idioma: if spec.idioma.is_empty() { "es".into() } else { spec.idioma.clone() },
        reference: spec.referencia.as_ref().map(|r| root.join(r)),
        extra: spec.ajustes.as_ref().map(|v| v.to_string()).unwrap_or_default(),
    })
}

// ── text ──

/// What a voice can read: no roleplay actions, no markdown, no code, no links, no emoji.
pub fn speakable(text: &str) -> String {
    let mut t = crate::mind::strip_actions(text);
    while let Some(a) = t.find("```") {
        let end = t[a + 3..].find("```").map(|b| a + 3 + b + 3).unwrap_or(t.len());
        t.replace_range(a..end, " (código) ");
    }
    // Remove parenthesized action cues like (sighs), (suspira), (pausa), (bosteza)
    while let Some(a) = t.find('(') {
        match t[a + 1..].find(')') {
            Some(b) => {
                let inside = t[a + 1..a + 1 + b].trim();
                let is_action = inside != "código" && inside != "enlace" && inside.chars().all(|c| c.is_alphabetic() || c.is_whitespace() || c == '.' || c == ',');
                if is_action {
                    t.replace_range(a..=a + 1 + b, " ");
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    // Remove bracketed stage directions like [sighs], [suspira]
    while let Some(a) = t.find('[') {
        match t[a + 1..].find(']') {
            Some(b) => {
                t.replace_range(a..=a + 1 + b, " ");
            }
            None => break,
        }
    }
    // Replace ellipses and repeated dots with a clean period:
    // Autoregressive / flow-matching speech models (like Pocket TTS) interpret '...' as literal sighs, gasps and panting.
    t = t.replace("...", ". ").replace('…', ". ");
    let words: Vec<&str> = t.split_whitespace().map(|w| if w.starts_with("http://") || w.starts_with("https://") { "(enlace)" } else { w }).collect();
    let t = words.join(" ");
    let cleaned: String = t.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || ".,;:!?¿¡-'\"()%/€$+=".contains(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    cleaned.replace(". .", ".").replace("..", ".").replace(", ,", ",")
}

/// Pieces to synthesize one after another: sentences, joined when very short, cut at commas
/// when very long, so the first one is ready (and audible) soon.
pub fn sentences(text: &str) -> Vec<String> {
    const SHORT: usize = 40;
    const LONG: usize = 220;
    let mut raw: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let end = matches!(c, '.' | '!' | '?' | '…') && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        if end || (cur.chars().count() > LONG && matches!(c, ',' | ';' | ':')) {
            raw.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        raw.push(cur.trim().to_string());
    }
    let mut out: Vec<String> = Vec::new();
    // Discard empty or single-character noise that triggers acoustic artifacts/sighs in Pocket TTS
    for s in raw.into_iter().filter(|s| s.chars().filter(|c| c.is_alphanumeric()).count() >= 2) {
        match out.last_mut() {
            Some(last) if last.chars().count() < SHORT => {
                last.push(' ');
                last.push_str(&s);
            }
            _ => out.push(s),
        }
    }
    out
}

#[cfg(test)]
use crate::download::wav;

// ── the C API of sherpa-onnx 1.13.8 (c-api.h), only what text-to-speech needs ──

type Str = *const c_char;

#[repr(C)]
struct VitsCfg {
    model: Str,
    lexicon: Str,
    tokens: Str,
    data_dir: Str,
    noise_scale: f32,
    noise_scale_w: f32,
    length_scale: f32,
    dict_dir: Str,
}

#[repr(C)]
struct MatchaCfg {
    acoustic_model: Str,
    vocoder: Str,
    lexicon: Str,
    tokens: Str,
    data_dir: Str,
    noise_scale: f32,
    length_scale: f32,
    dict_dir: Str,
}

#[repr(C)]
struct KokoroCfg {
    model: Str,
    voices: Str,
    tokens: Str,
    data_dir: Str,
    length_scale: f32,
    dict_dir: Str,
    lexicon: Str,
    lang: Str,
}

#[repr(C)]
struct KittenCfg {
    model: Str,
    voices: Str,
    tokens: Str,
    data_dir: Str,
    length_scale: f32,
}

#[repr(C)]
struct ZipvoiceCfg {
    tokens: Str,
    encoder: Str,
    decoder: Str,
    vocoder: Str,
    data_dir: Str,
    lexicon: Str,
    feat_scale: f32,
    t_shift: f32,
    target_rms: f32,
    guidance_scale: f32,
}

#[repr(C)]
struct PocketCfg {
    lm_flow: Str,
    lm_main: Str,
    encoder: Str,
    decoder: Str,
    text_conditioner: Str,
    vocab_json: Str,
    token_scores_json: Str,
    voice_embedding_cache_capacity: i32,
}

#[repr(C)]
struct SupertonicCfg {
    duration_predictor: Str,
    text_encoder: Str,
    vector_estimator: Str,
    vocoder: Str,
    tts_json: Str,
    unicode_indexer: Str,
    voice_style: Str,
}

#[repr(C)]
struct ModelCfg {
    vits: VitsCfg,
    num_threads: i32,
    debug: i32,
    provider: Str,
    matcha: MatchaCfg,
    kokoro: KokoroCfg,
    kitten: KittenCfg,
    zipvoice: ZipvoiceCfg,
    pocket: PocketCfg,
    supertonic: SupertonicCfg,
}

#[repr(C)]
struct TtsCfg {
    model: ModelCfg,
    rule_fsts: Str,
    max_num_sentences: i32,
    rule_fars: Str,
    silence_scale: f32,
}

/// SherpaOnnxGenerationConfig: what GenerateWithConfig takes (reference audio for cloning).
#[repr(C)]
struct GenCfg {
    silence_scale: f32,
    speed: f32,
    sid: i32,
    reference_audio: *const f32,
    reference_audio_len: i32,
    reference_sample_rate: i32,
    reference_text: Str,
    num_steps: i32,
    extra: Str,
}

#[repr(C)]
struct Audio {
    samples: *const f32,
    n: i32,
    sample_rate: i32,
}

type CreateFn = unsafe extern "C" fn(*const TtsCfg) -> *const c_void;
type DestroyFn = unsafe extern "C" fn(*const c_void);
type GenerateFn = unsafe extern "C" fn(*const c_void, *const c_char, i32, f32) -> *const Audio;
type FreeAudioFn = unsafe extern "C" fn(*const Audio);
type GenerateCfgFn = unsafe extern "C" fn(*const c_void, *const c_char, *const GenCfg, *const c_void, *mut c_void) -> *const Audio;

/// A loaded voice. Lives on the voice thread only.
struct Tts {
    tts: *const c_void,
    generate: GenerateFn,
    generate_cfg: GenerateCfgFn,
    free_audio: FreeAudioFn,
    destroy: DestroyFn,
    setup: Setup,
    /// Pocket TTS: the reference clip, read once.
    reference: Option<(Vec<f32>, u32)>,
    extra: CString,
}

/// A 16-bit PCM WAV (mono, or its channels averaged): samples in [-1, 1] and their rate.
pub fn read_wav(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(crate::lang::tr(format!("{} no es un WAV", path.display()), format!("{} is not a WAV", path.display())));
    }
    let (mut channels, mut rate, mut bits, mut data) = (0u16, 0u32, 0u16, None);
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = &b[i + 8..(i + 8 + len).min(b.len())];
        if id == b"fmt " && body.len() >= 16 {
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            data = Some(body);
        }
        i += 8 + len + (len & 1);
    }
    let data = data.ok_or_else(|| crate::lang::tr(format!("{}: sin datos de audio", path.display()), format!("{}: no audio data", path.display())))?;
    if bits != 16 || channels == 0 {
        return Err(crate::lang::tr(format!("{}: tiene que ser PCM de 16 bits", path.display()), format!("{}: must be 16-bit PCM", path.display())));
    }
    let ch = channels as usize;
    let samples = data.chunks_exact(2 * ch).map(|f| f.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0).sum::<f32>() / ch as f32).collect();
    Ok((samples, rate))
}

#[cfg(windows)]
pub(crate) fn symbol(lib: isize, name: &str) -> Result<*const c_void, String> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    let c = CString::new(name).unwrap();
    unsafe { GetProcAddress(lib as _, c.as_ptr().cast()) }.map(|f| f as *const c_void).ok_or_else(|| crate::lang::tr(format!("la biblioteca de voz no tiene {name}"), format!("the voice library has no {name}")))
}

#[cfg(windows)]
pub(crate) fn load_library(path: &Path) -> Result<isize, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH};
    let abs = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Its onnxruntime.dll sits next to it: altered search path finds it there.
    let wide: Vec<u16> = abs.as_os_str().encode_wide().chain([0]).collect();
    let h = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
    if h.is_null() {
        return Err(crate::lang::tr(format!("no se pudo cargar {}", abs.display()), format!("could not load {}", abs.display())));
    }
    Ok(h as isize)
}

pub(crate) fn cstr(p: &Path) -> CString {
    CString::new(p.to_string_lossy().as_bytes()).unwrap_or_default()
}

/// The first file in `dir` whose name ends with `suffix`.
pub(crate) fn find(dir: &Path, suffix: &str) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(suffix)).collect();
    v.sort();
    v.into_iter().next()
}

impl Tts {
    #[cfg(windows)]
    fn load(s: &Setup) -> Result<Tts, String> {
        // The library stays loaded for the life of the app: a voice may be reloaded, the DLL not.
        static LIB: OnceLock<Result<isize, String>> = OnceLock::new();
        let lib = LIB.get_or_init(|| load_library(&s.dll)).clone()?;
        let (create, destroy, generate, generate_cfg, free_audio) = unsafe {
            (
                std::mem::transmute::<*const c_void, CreateFn>(symbol(lib, "SherpaOnnxCreateOfflineTts")?),
                std::mem::transmute::<*const c_void, DestroyFn>(symbol(lib, "SherpaOnnxDestroyOfflineTts")?),
                std::mem::transmute::<*const c_void, GenerateFn>(symbol(lib, "SherpaOnnxOfflineTtsGenerate")?),
                std::mem::transmute::<*const c_void, GenerateCfgFn>(symbol(lib, "SherpaOnnxOfflineTtsGenerateWithConfig")?),
                std::mem::transmute::<*const c_void, FreeAudioFn>(symbol(lib, "SherpaOnnxDestroyOfflineTtsGeneratedAudio")?),
            )
        };
        let kokoro = s.motor == "kokoro";
        let pocket = s.motor == "pocket";
        // Pocket TTS: its seven files, by the start of their names (int8 or not).
        let part = |prefix: &str| -> Result<CString, String> {
            let mut v: Vec<PathBuf> = std::fs::read_dir(&s.dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|f| f.to_string_lossy().starts_with(prefix))).collect();
            v.sort();
            v.first().map(|p| cstr(p)).ok_or_else(|| crate::lang::tr(format!("falta {prefix}* en {}", s.dir.display()), format!("{prefix}* is missing in {}", s.dir.display())))
        };
        let pocket_files = if pocket {
            Some([part("lm_flow")?, part("lm_main")?, part("encoder")?, part("decoder")?, part("text_conditioner")?, part("vocab.json")?, part("token_scores.json")?])
        } else {
            None
        };
        let tokens = cstr(&s.dir.join("tokens.txt"));
        let data = cstr(&s.dir.join("espeak-ng-data"));
        let provider = CString::new("cpu").unwrap();
        let lang = CString::new(s.lang.as_str()).unwrap_or_default();
        let model_path = if kokoro { find(&s.dir, ".onnx").filter(|p| p.file_name().is_some_and(|f| f.to_string_lossy().starts_with("model"))) } else { find(&s.dir, ".onnx") };
        let model = cstr(&model_path.ok_or_else(|| crate::lang::tr(format!("no hay modelo .onnx en {}", s.dir.display()), format!("there is no .onnx model in {}", s.dir.display())))?);
        let reference = match (&s.reference, pocket) {
            (Some(r), true) => Some(read_wav(r)?),
            (None, true) => return Err(crate::lang::tr("una voz que clona necesita su clip de referencia", "a cloning voice needs its reference clip")),
            _ => None,
        };
        let voices = cstr(&s.dir.join("voices.bin"));
        let lexicon = cstr(&s.dir.join("lexicon-us-en.txt"));
        // Zero is "not set": the library fills in its defaults.
        let mut cfg: TtsCfg = unsafe { std::mem::zeroed() };
        // 4 threads: twice as fast as 2 on the 5800X; more gain nothing.
        cfg.model.num_threads = 4;
        cfg.model.provider = provider.as_ptr();
        cfg.max_num_sentences = 1;
        if let Some(f) = &pocket_files {
            cfg.model.pocket.lm_flow = f[0].as_ptr();
            cfg.model.pocket.lm_main = f[1].as_ptr();
            cfg.model.pocket.encoder = f[2].as_ptr();
            cfg.model.pocket.decoder = f[3].as_ptr();
            cfg.model.pocket.text_conditioner = f[4].as_ptr();
            cfg.model.pocket.vocab_json = f[5].as_ptr();
            cfg.model.pocket.token_scores_json = f[6].as_ptr();
            // The voice embedding of her clip is computed once and kept.
            cfg.model.pocket.voice_embedding_cache_capacity = 4;
        } else if kokoro {
            cfg.model.kokoro.model = model.as_ptr();
            cfg.model.kokoro.voices = voices.as_ptr();
            cfg.model.kokoro.tokens = tokens.as_ptr();
            cfg.model.kokoro.data_dir = data.as_ptr();
            if s.dir.join("lexicon-us-en.txt").exists() {
                cfg.model.kokoro.lexicon = lexicon.as_ptr();
            }
            cfg.model.kokoro.lang = lang.as_ptr();
        } else {
            cfg.model.vits.model = model.as_ptr();
            cfg.model.vits.tokens = tokens.as_ptr();
            cfg.model.vits.data_dir = data.as_ptr();
        }
        // The library copies every string while creating the voice.
        let tts = unsafe { create(&cfg) };
        if tts.is_null() {
            return Err(crate::lang::tr(format!("sherpa-onnx no pudo cargar la voz de {}", s.dir.display()), format!("sherpa-onnx could not load the voice from {}", s.dir.display())));
        }
        let extra = CString::new(s.extra.as_str()).unwrap_or_default();
        Ok(Tts { tts, generate, generate_cfg, free_audio, destroy, setup: s.clone(), reference, extra })
    }

    #[cfg(not(windows))]
    fn load(_s: &Setup) -> Result<Tts, String> {
        Err(crate::lang::tr("la voz solo funciona en Windows", "voice only works on Windows"))
    }

    /// Samples in [-1, 1] and their rate.
    fn synth(&self, text: &str) -> Result<(Vec<f32>, u32), String> {
        let c = CString::new(text.replace('\0', " ")).unwrap_or_default();
        let a = match &self.reference {
            Some((samples, rate)) => {
                let g = GenCfg {
                    silence_scale: 0.2,
                    speed: self.setup.speed,
                    sid: 0,
                    reference_audio: samples.as_ptr(),
                    reference_audio_len: samples.len() as i32,
                    reference_sample_rate: *rate as i32,
                    reference_text: std::ptr::null(),
                    num_steps: 0,
                    extra: if self.setup.extra.is_empty() { std::ptr::null() } else { self.extra.as_ptr() },
                };
                unsafe { (self.generate_cfg)(self.tts, c.as_ptr(), &g, std::ptr::null(), std::ptr::null_mut()) }
            }
            None => unsafe { (self.generate)(self.tts, c.as_ptr(), self.setup.sid, self.setup.speed) },
        };
        if a.is_null() {
            return Err(crate::lang::tr("la voz no generó audio", "the voice produced no audio"));
        }
        let out = unsafe {
            let n = (*a).n.max(0) as usize;
            let v = if n == 0 || (*a).samples.is_null() { Vec::new() } else { std::slice::from_raw_parts((*a).samples, n).to_vec() };
            (v, (*a).sample_rate.max(1) as u32)
        };
        unsafe { (self.free_audio)(a) };
        Ok(out)
    }
}

impl Drop for Tts {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.tts) };
    }
}

// ── playing ──

/// Plays a piece until it ends or `turn` is no longer the current one. Through waveOut, not
/// PlaySound, because waveOut says which sample is playing: 30 times a second, the loudness
/// right there goes to `ON_PLAY`, so her mouth follows the sound itself, not a clock.
#[cfg(windows)]
fn play_blocking(samples: &[f32], rate: u32, turn: u64) {
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Media::Audio::*;
    use windows_sys::Win32::Media::{MMTIME, TIME_BYTES, TIME_MS, TIME_SAMPLES};
    let pcm: Vec<i16> = samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
    let levels = loudness(samples, rate);
    let step = (rate / MOUTH_FPS).max(1) as usize;
    let fmt = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_PCM as u16, nChannels: 1, nSamplesPerSec: rate, nAvgBytesPerSec: rate * 2, nBlockAlign: 2, wBitsPerSample: 16, cbSize: 0 };
    let hdr_size = std::mem::size_of::<WAVEHDR>() as u32;
    unsafe {
        let mut out: HWAVEOUT = std::mem::zeroed();
        if waveOutOpen(&mut out, WAVE_MAPPER, &fmt, 0, 0, CALLBACK_NULL) != 0 {
            return;
        }
        let mut hdr: WAVEHDR = std::mem::zeroed();
        hdr.lpData = pcm.as_ptr() as *mut u8;
        hdr.dwBufferLength = (pcm.len() * 2) as u32;
        if waveOutPrepareHeader(out, &mut hdr, hdr_size) == 0 && waveOutWrite(out, &mut hdr, hdr_size) == 0 {
            // A device that never moves must not hold the voice thread forever.
            let deadline = Instant::now() + Duration::from_secs_f32(samples.len() as f32 / rate as f32 + 2.0);
            loop {
                let mut t: MMTIME = std::mem::zeroed();
                t.wType = TIME_SAMPLES;
                waveOutGetPosition(out, &mut t, std::mem::size_of::<MMTIME>() as u32);
                // The driver may answer in another unit than the one asked for.
                let pos = match t.wType {
                    TIME_SAMPLES => t.u.sample as usize,
                    TIME_BYTES => t.u.cb as usize / 2,
                    TIME_MS => t.u.ms as usize * rate as usize / 1000,
                    _ => 0,
                };
                if pos >= samples.len() || TURN.load(Ordering::Acquire) != turn || Instant::now() > deadline {
                    break;
                }
                tell(levels.get(pos / step).copied().unwrap_or(0.0));
                std::thread::sleep(Duration::from_millis(1000 / MOUTH_FPS as u64));
            }
        }
        // Reset first: a buffer still playing cannot be unprepared.
        waveOutReset(out);
        waveOutUnprepareHeader(out, &mut hdr, hdr_size);
        waveOutClose(out);
    }
    tell(0.0);
}

#[cfg(not(windows))]
fn play_blocking(_samples: &[f32], _rate: u32, _turn: u64) {}

// ── the voice thread ──

enum Job {
    Say(Setup, String, u64),
    Warm(Setup),
}

/// Bumped by every new utterance and by `stop`: older pieces are dropped, unplayed.
static TURN: AtomicU64 = AtomicU64::new(0);
/// Told the loudness of what is playing, `MOUTH_FPS` times a second, and 0 when it ends or is
/// cut: the chibi moves her mouth with it.
static ON_PLAY: OnceLock<Box<dyn Fn(f32) + Send + Sync>> = OnceLock::new();

/// How often the loudness is measured and told.
pub const MOUTH_FPS: u32 = 30;

/// Her voice is playing right now (someone speaking over it makes her stop).
static PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_playing() -> bool {
    PLAYING.load(Ordering::Acquire)
}

/// The RMS of each 1/`MOUTH_FPS` s of the audio.
pub fn loudness(samples: &[f32], rate: u32) -> Vec<f32> {
    let step = (rate / MOUTH_FPS).max(1) as usize;
    samples.chunks(step).map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()).collect()
}

/// Where the loudness goes (set once, at start-up).
pub fn on_play(f: impl Fn(f32) + Send + Sync + 'static) {
    let _ = ON_PLAY.set(Box::new(f));
}

/// Told every piece she is about to say (samples and rate): the fine ear learns her voice from
/// it, so that when the microphone hears it back from the speakers it knows it is hers.
static ON_PIECE: OnceLock<Box<dyn Fn(&[f32], u32) + Send + Sync>> = OnceLock::new();

/// Where each synthesized piece goes (set once, at start-up).
pub fn on_piece(f: impl Fn(&[f32], u32) + Send + Sync + 'static) {
    let _ = ON_PIECE.set(Box::new(f));
}

fn tell(level: f32) {
    if let Some(f) = ON_PLAY.get() {
        f(level);
    }
}
/// Reported to the app: why the last utterance failed, if it did.
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

fn worker() -> &'static Mutex<Sender<Job>> {
    static TX: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        let (ptx, prx) = channel::<Piece>();
        std::thread::spawn(move || synth_loop(rx, ptx));
        std::thread::spawn(move || {
            for (turn, samples, rate) in prx {
                if turn == TURN.load(Ordering::Acquire) {
                    PLAYING.store(true, Ordering::Release);
                    play_blocking(&samples, rate, turn);
                    PLAYING.store(false, Ordering::Release);
                }
            }
        });
        Mutex::new(tx)
    })
}

/// The voice `s`, loaded once and kept: only a different voice is loaded again (the speed is
/// just a setting).
fn ensure<'a>(voice: &'a mut Option<Tts>, s: &Setup) -> Result<&'a Tts, String> {
    let same = |a: &Setup| Setup { speed: s.speed, ..a.clone() } == *s;
    if !voice.as_ref().is_some_and(|v| same(&v.setup)) {
        *voice = None;
        *voice = Some(Tts::load(s)?);
    }
    let v = voice.as_mut().unwrap();
    v.setup.speed = s.speed;
    Ok(v)
}

/// A piece ready to play: its turn, its samples and their rate.
type Piece = (u64, Vec<f32>, u32);

fn synth_loop(rx: Receiver<Job>, play: Sender<Piece>) {
    let mut voice: Option<Tts> = None;
    let fail = |e: String| *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = e;
    while let Ok(job) = rx.recv() {
        let (s, text, turn) = match job {
            Job::Warm(s) => {
                if let Err(e) = ensure(&mut voice, &s) {
                    fail(e);
                }
                continue;
            }
            Job::Say(s, t, turn) => (s, t, turn),
        };
        let v = match ensure(&mut voice, &s) {
            Ok(v) => v,
            Err(e) => {
                fail(e);
                continue;
            }
        };
        fail(String::new());
        for piece in sentences(&text) {
            if TURN.load(Ordering::Acquire) != turn {
                break;
            }
            match v.synth(&piece) {
                Ok((samples, rate)) if !samples.is_empty() => {
                    if cfg!(debug_assertions) {
                        eprintln!("voz: {:.1} s de audio · «{}»", samples.len() as f32 / rate as f32, piece.chars().take(60).collect::<String>());
                    }
                    if let Some(f) = ON_PIECE.get() {
                        f(&samples, rate);
                    }
                    let _ = play.send((turn, samples, rate));
                }
                Ok(_) => {}
                Err(e) => {
                    fail(e);
                    break;
                }
            }
        }
    }
}

/// Says `text` out loud, cutting off whatever she was saying.
pub fn say(s: Setup, text: &str) {
    let text = speakable(text);
    if text.is_empty() {
        return;
    }
    let turn = TURN.fetch_add(1, Ordering::AcqRel) + 1;
    let _ = worker().lock().unwrap_or_else(|e| e.into_inner()).send(Job::Say(s, text, turn));
}

/// Loads the voice ahead of time, so the first reply does not wait for it.
pub fn warm(s: Setup) {
    let _ = worker().lock().unwrap_or_else(|e| e.into_inner()).send(Job::Warm(s));
}

pub fn stop() {
    TURN.fetch_add(1, Ordering::AcqRel);
}

pub fn last_error() -> String {
    LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// For the tests of `asr`: a line in this voice, without playing it.
#[cfg(test)]
pub(crate) fn synth_for_test(s: &Setup, text: &str) -> Result<(Vec<f32>, u32), String> {
    Tts::load(s)?.synth(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_matches_sherpa_onnx_1_13_8() {
        // Sizes of the C structs on x64, from c-api.h of that version.
        assert_eq!(std::mem::size_of::<VitsCfg>(), 56);
        assert_eq!(std::mem::size_of::<KokoroCfg>(), 64);
        assert_eq!(std::mem::size_of::<Audio>(), 16);
        assert_eq!(std::mem::offset_of!(ModelCfg, num_threads), 56);
        assert_eq!(std::mem::offset_of!(ModelCfg, matcha), 72);
        assert_eq!(std::mem::offset_of!(TtsCfg, rule_fsts), std::mem::size_of::<ModelCfg>());
        assert_eq!(std::mem::offset_of!(GenCfg, reference_audio), 16);
        assert_eq!(std::mem::offset_of!(GenCfg, extra), 48);
        assert_eq!(std::mem::size_of::<GenCfg>(), 56);
    }

    #[test]
    fn only_what_can_be_said_goes_to_the_voice() {
        assert_eq!(speakable("*Se ajusta las gafas.* Mira esto: https://ejemplo.com ✨"), "Mira esto: (enlace)");
        assert_eq!(speakable("Usa:\n```rust\nfn main() {}\n```\n¿Vale?"), "Usa: (código) ¿Vale?");
        assert_eq!(speakable("**Ojo**: el `build` falló."), "Ojo: el build falló.");
    }

    #[test]
    fn replies_are_cut_in_sentences_that_are_not_too_short() {
        let s = sentences("Sí. Eso compila, pero el test de red falla porque el puerto está ocupado. ¿Lo cierro? Hecho.");
        assert_eq!(s, vec!["Sí. Eso compila, pero el test de red falla porque el puerto está ocupado.", "¿Lo cierro? Hecho."]);
        assert_eq!(sentences("v1.13.8 es la versión"), vec!["v1.13.8 es la versión"], "a dot inside a word is not an end");
        let long = "uno, ".repeat(80);
        assert!(sentences(&long).iter().all(|p| p.chars().count() <= 230));
        assert!(sentences("…").is_empty());
    }

    #[test]
    fn wav_header_is_right() {
        let w = wav(&[0.0, 1.0, -1.0], 22050);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 22050);
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), 32767);
    }

    #[test]
    fn loudness_is_one_rms_per_frame() {
        // 0.1 s at 300 Hz: three frames of 10 samples, silent, full and half.
        let s: Vec<f32> = [0.0, 1.0, 0.5].iter().flat_map(|&a| (0..10).map(move |i| if i % 2 == 0 { a } else { -a })).collect();
        let l = loudness(&s, 300);
        assert_eq!(l.len(), 3);
        assert!(l[0] == 0.0 && (l[1] - 1.0).abs() < 1e-6 && (l[2] - 0.5).abs() < 1e-6);
        assert!(loudness(&[], 24000).is_empty());
    }

    /// Plays 0.3 s of silence, 0.3 s of a quiet tone and 0.3 s of silence, and checks the
    /// mouth hears the tone only while the sound card is on it. Makes a short beep, so it is
    /// run by hand: `cargo test --release -- --ignored mouth_follows`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn mouth_follows_the_sample_playing() {
        use std::time::Instant;
        static HEARD: Mutex<Vec<(f32, f32)>> = Mutex::new(Vec::new());
        static START: OnceLock<Instant> = OnceLock::new();
        on_play(|level| HEARD.lock().unwrap().push((START.get().unwrap().elapsed().as_secs_f32(), level)));
        let rate = 24000;
        let tone: Vec<f32> = (0..rate * 9 / 10)
            .map(|i| if (rate * 3 / 10..rate * 6 / 10).contains(&i) { 0.05 * (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() } else { 0.0 })
            .collect();
        let turn = TURN.load(Ordering::Acquire);
        START.set(Instant::now()).unwrap();
        play_blocking(&tone, rate, turn);
        let heard = HEARD.lock().unwrap().clone();
        let loud: Vec<f32> = heard.iter().filter(|(_, l)| *l > 0.01).map(|(t, _)| *t).collect();
        eprintln!("{} ticks, loud from {:.2} s to {:.2} s", heard.len(), loud.first().unwrap_or(&0.0), loud.last().unwrap_or(&0.0));
        assert!(heard.len() >= 20, "about 30 a second for 0.9 s");
        assert_eq!(heard.last().unwrap().1, 0.0, "it ends closed");
        // The tone plays from 0.3 to 0.6 s of audio; the device adds a little start-up delay.
        assert!(loud.len() >= 6, "the tone is heard");
        assert!(loud[0] >= 0.28 && *loud.last().unwrap() <= 0.75, "only while it plays: {loud:?}");
    }

    #[test]
    fn a_wav_reads_back() {
        let p = std::env::temp_dir().join(format!("kaltsit-wav-{}.wav", std::process::id()));
        std::fs::write(&p, wav(&[0.0, 0.5, -0.5, 1.0], 24000)).unwrap();
        let (s, r) = read_wav(&p).unwrap();
        assert_eq!(r, 24000);
        assert_eq!(s.len(), 4);
        assert!((s[1] - 0.5).abs() < 1e-3 && (s[2] + 0.5).abs() < 1e-3);
        let _ = std::fs::remove_file(&p);
    }

    /// With the voices downloaded (Modelos), they really speak, accents included.
    #[test]
    fn real_voices_speak_when_downloaded() {
        let m = Manifest::load();
        for id in ["voz-daniela", "voz-kokoro-dora", "voz-kaltsit-en"] {
            let Ok(s) = setup(&m, id, 1.0) else {
                eprintln!("{id}: no descargada, se salta");
                continue;
            };
            let t = Tts::load(&s).unwrap();
            let line = if s.idioma == "en" { "Tomorrow? Yes, and the year after that." } else { "¿Mañana? Sí: el año que viene, también." };
            let (a, rate) = t.synth(line).unwrap();
            let secs = a.len() as f32 / rate as f32;
            // Read with the accents, that is around two or three seconds; garbled text is far shorter.
            assert!(secs > 1.2 && secs < 6.0, "{id}: {secs} s");
            assert!(a.iter().any(|x| x.abs() > 0.05), "{id}: not silence");
        }
    }
}



