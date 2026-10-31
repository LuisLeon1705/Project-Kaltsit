//! Her fine ear: what the microphone heard is cleaned (GTCRN), cut down to where someone speaks
//! (Silero VAD), and every voice in it is recognised by its voiceprint (CAM++) before Whisper
//! writes it down. A long take, or a short one with more than one voice, is split by speaker
//! first (pyannote). Her own voice, coming back through the speakers, is recognised too and
//! dropped. All through sherpa-onnx's C library, the same DLL as her voice and her dictation.
//!
//! Every stage is optional: without the fine ear downloaded, dictation is plain Whisper.

use crate::catalog::Manifest;
use serde::{Deserialize, Serialize};
use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Everything here runs at Whisper's rate (the microphone opens at it).
pub const RATE: u32 = crate::asr::RATE;
/// A take longer than this is split by speaker before anything else.
const LONG_TAKE_S: f32 = 8.0;
/// Below this cosine similarity a voice is somebody else. Measured on three real people
/// (`voiceprints_tell_real_people_apart`): the same person ≥ 0.74, two different ≤ 0.55.
pub const SAME_VOICE: f32 = 0.65;
/// A match this sure also refines the stored voiceprint (a weaker one would make it drift).
const LEARN_FROM: f32 = 0.75;
/// Below `SAME_VOICE`, a voice this close is still probably that one when no other comes near
/// (`MARGIN`). A real quiet voice through a real microphone scored 0.58–0.59 against its own
/// stored voiceprint, and other people at most 0.17 against it (`a_real_take_against_the_
/// stored_voices`): with 0.65 alone, every take of it became a new unknown voice.
const PROBABLY: f32 = 0.5;
const MARGIN: f32 = 0.2;
/// …and it also refines the voiceprint when nothing else is within this, so the stored voice
/// adapts to the real microphone.
const LEARN_MARGIN: f32 = 0.3;
const LEARN_PROBABLE: f32 = 0.55;
/// Only this far from every voice is a voice new; in between, she cannot tell whose it is.
const NEW_BELOW: f32 = 0.4;
/// Less voice than this gives no usable voiceprint: a 2.5 s take with 0.9 s of speech scored
/// 0.32 against the same person, below two different people (measured in the tests).
const MIN_PRINT_S: f32 = 1.5;
/// Is 1.6 s of voice hers? Measured (`measure_own_voice_windows`): her voice's windows score
/// 0.54–0.65 against her voiceprint, someone else's at most 0.41. Lower than `SAME_VOICE` on
/// purpose: cutting herself off is worse than being slow to let someone interrupt her.
const OWN_VOICE: f32 = 0.5;
/// Pieces of her own voice averaged into her voiceprint before it is left alone.
const OWN_PIECES: u32 = 12;
/// Her own voiceprint's name in the store.
pub const OWN_NAME: &str = "Kal'tsit";

/// What to load: the library and the fine ear's folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub dll: PathBuf,
    pub dir: PathBuf,
}

/// The fine ear, if it and the engine are downloaded.
pub fn setup(m: &Manifest) -> Option<Setup> {
    let root = m.root();
    let ear = m.elementos.iter().find(|i| i.oido.as_ref().is_some_and(|o| o.motor == "voces") && i.installed(&root))?;
    let engine = m.elementos.iter().filter(|i| i.servidor.as_deref().is_some_and(|s| s.ends_with(".dll"))).find(|i| i.installed(&root))?;
    Some(Setup { dll: root.join(engine.servidor.as_deref()?), dir: root.join(&ear.oido.as_ref()?.carpeta) })
}

// ── the C API of sherpa-onnx 1.13.8 (c-api.h): VAD, denoiser, speaker embedding, diarization ──

type Str = *const c_char;

#[repr(C)]
#[derive(Clone, Copy)]
struct SileroCfg {
    model: Str,
    threshold: f32,
    min_silence_duration: f32,
    min_speech_duration: f32,
    window_size: i32,
    max_speech_duration: f32,
}

#[repr(C)]
struct VadCfg {
    silero_vad: SileroCfg,
    sample_rate: i32,
    num_threads: i32,
    provider: Str,
    debug: i32,
    /// SherpaOnnxTenVadModelConfig has the same fields as Silero's; unused (zeroed).
    ten_vad: SileroCfg,
}

#[repr(C)]
struct SpeechSegment {
    start: i32,
    samples: *const f32,
    n: i32,
}

#[repr(C)]
struct EmbCfg {
    model: Str,
    num_threads: i32,
    debug: i32,
    provider: Str,
}

#[repr(C)]
struct PyannoteCfg {
    model: Str,
    window_shift_ratio: f32,
}

#[repr(C)]
struct SegCfg {
    pyannote: PyannoteCfg,
    num_threads: i32,
    debug: i32,
    provider: Str,
}

#[repr(C)]
struct ClusterCfg {
    num_clusters: i32,
    threshold: f32,
    compute_confidence: i32,
}

#[repr(C)]
struct DiarCfg {
    segmentation: SegCfg,
    embedding: EmbCfg,
    clustering: ClusterCfg,
    min_duration_on: f32,
    min_duration_off: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DiarSeg {
    start: f32,
    end: f32,
    speaker: i32,
    confidence: f32,
}

#[repr(C)]
struct GtcrnCfg {
    model: Str,
}

#[repr(C)]
struct DpdfNetCfg {
    model: Str,
    attenuation_limit_db: f32,
}

#[repr(C)]
struct DenoiserCfg {
    gtcrn: GtcrnCfg,
    num_threads: i32,
    debug: i32,
    provider: Str,
    dpdfnet: DpdfNetCfg,
}

#[repr(C)]
struct Denoised {
    samples: *const f32,
    n: i32,
    sample_rate: i32,
}

type P = *const c_void;

/// The functions used, looked up once.
struct Api {
    vad_create: unsafe extern "C" fn(*const VadCfg, f32) -> P,
    vad_destroy: unsafe extern "C" fn(P),
    vad_accept: unsafe extern "C" fn(P, *const f32, i32),
    vad_empty: unsafe extern "C" fn(P) -> i32,
    vad_front: unsafe extern "C" fn(P) -> *const SpeechSegment,
    vad_pop: unsafe extern "C" fn(P),
    vad_flush: unsafe extern "C" fn(P),
    vad_reset: unsafe extern "C" fn(P),
    vad_detected: unsafe extern "C" fn(P) -> i32,
    segment_destroy: unsafe extern "C" fn(*const SpeechSegment),
    den_create: unsafe extern "C" fn(*const DenoiserCfg) -> P,
    den_destroy: unsafe extern "C" fn(P),
    den_run: unsafe extern "C" fn(P, *const f32, i32, i32) -> *const Denoised,
    denoised_destroy: unsafe extern "C" fn(*const Denoised),
    emb_create: unsafe extern "C" fn(*const EmbCfg) -> P,
    emb_destroy: unsafe extern "C" fn(P),
    emb_dim: unsafe extern "C" fn(P) -> i32,
    emb_stream: unsafe extern "C" fn(P) -> P,
    emb_ready: unsafe extern "C" fn(P, P) -> i32,
    emb_compute: unsafe extern "C" fn(P, P) -> *const f32,
    emb_free: unsafe extern "C" fn(*const f32),
    stream_accept: unsafe extern "C" fn(P, i32, *const f32, i32),
    stream_finished: unsafe extern "C" fn(P),
    stream_destroy: unsafe extern "C" fn(P),
    diar_create: unsafe extern "C" fn(*const DiarCfg) -> P,
    diar_destroy: unsafe extern "C" fn(P),
    diar_process: unsafe extern "C" fn(P, *const f32, i32) -> P,
    diar_segments: unsafe extern "C" fn(P) -> i32,
    diar_sorted: unsafe extern "C" fn(P) -> *const DiarSeg,
    diar_free_segments: unsafe extern "C" fn(*const DiarSeg),
    diar_free_result: unsafe extern "C" fn(P),
}

#[cfg(windows)]
fn api(dll: &Path) -> Result<&'static Api, String> {
    use crate::voice::{load_library, symbol};
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| {
        let lib = load_library(dll)?;
        macro_rules! f {
            ($name:literal) => {
                unsafe { std::mem::transmute::<*const c_void, _>(symbol(lib, $name)?) }
            };
        }
        Ok(Api {
            vad_create: f!("SherpaOnnxCreateVoiceActivityDetector"),
            vad_destroy: f!("SherpaOnnxDestroyVoiceActivityDetector"),
            vad_accept: f!("SherpaOnnxVoiceActivityDetectorAcceptWaveform"),
            vad_empty: f!("SherpaOnnxVoiceActivityDetectorEmpty"),
            vad_front: f!("SherpaOnnxVoiceActivityDetectorFront"),
            vad_pop: f!("SherpaOnnxVoiceActivityDetectorPop"),
            vad_flush: f!("SherpaOnnxVoiceActivityDetectorFlush"),
            vad_reset: f!("SherpaOnnxVoiceActivityDetectorReset"),
            vad_detected: f!("SherpaOnnxVoiceActivityDetectorDetected"),
            segment_destroy: f!("SherpaOnnxDestroySpeechSegment"),
            den_create: f!("SherpaOnnxCreateOfflineSpeechDenoiser"),
            den_destroy: f!("SherpaOnnxDestroyOfflineSpeechDenoiser"),
            den_run: f!("SherpaOnnxOfflineSpeechDenoiserRun"),
            denoised_destroy: f!("SherpaOnnxDestroyDenoisedAudio"),
            emb_create: f!("SherpaOnnxCreateSpeakerEmbeddingExtractor"),
            emb_destroy: f!("SherpaOnnxDestroySpeakerEmbeddingExtractor"),
            emb_dim: f!("SherpaOnnxSpeakerEmbeddingExtractorDim"),
            emb_stream: f!("SherpaOnnxSpeakerEmbeddingExtractorCreateStream"),
            emb_ready: f!("SherpaOnnxSpeakerEmbeddingExtractorIsReady"),
            emb_compute: f!("SherpaOnnxSpeakerEmbeddingExtractorComputeEmbedding"),
            emb_free: f!("SherpaOnnxSpeakerEmbeddingExtractorDestroyEmbedding"),
            stream_accept: f!("SherpaOnnxOnlineStreamAcceptWaveform"),
            stream_finished: f!("SherpaOnnxOnlineStreamInputFinished"),
            stream_destroy: f!("SherpaOnnxDestroyOnlineStream"),
            diar_create: f!("SherpaOnnxCreateOfflineSpeakerDiarization"),
            diar_destroy: f!("SherpaOnnxDestroyOfflineSpeakerDiarization"),
            diar_process: f!("SherpaOnnxOfflineSpeakerDiarizationProcess"),
            diar_segments: f!("SherpaOnnxOfflineSpeakerDiarizationResultGetNumSegments"),
            diar_sorted: f!("SherpaOnnxOfflineSpeakerDiarizationResultSortByStartTime"),
            diar_free_segments: f!("SherpaOnnxOfflineSpeakerDiarizationDestroySegment"),
            diar_free_result: f!("SherpaOnnxOfflineSpeakerDiarizationDestroyResult"),
        })
    })
    .as_ref()
    .map_err(|e| e.clone())
}

#[cfg(not(windows))]
fn api(_dll: &Path) -> Result<&'static Api, String> {
    Err(crate::lang::tr("el oído fino solo funciona en Windows", "the fine ear only works on Windows"))
}

/// The four models, loaded. Lives behind `FINE`.
struct Fine {
    api: &'static Api,
    vad: P,
    denoiser: P,
    embedder: P,
    diarizer: P,
    setup: Setup,
}

// The raw pointers are only used while holding the `FINE` lock.
unsafe impl Send for Fine {}

impl Fine {
    fn load(s: &Setup) -> Result<Fine, String> {
        let api = api(&s.dll)?;
        let path = |name: &str| -> Result<CString, String> {
            let p = s.dir.join(name);
            if p.exists() {
                Ok(crate::voice::cstr(&p))
            } else {
                Err(crate::lang::tr(format!("falta {}", p.display()), format!("{} is missing", p.display())))
            }
        };
        let (vad_m, den_m, emb_m, seg_m) = (path("silero_vad.onnx")?, path("gtcrn_simple.onnx")?, path("3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx")?, path(crate::catalog::SEGMENTATION)?);
        let cpu = CString::new("cpu").unwrap();
        unsafe {
            let mut vc: VadCfg = std::mem::zeroed();
            vc.silero_vad = SileroCfg { model: vad_m.as_ptr(), threshold: 0.5, min_silence_duration: 0.35, min_speech_duration: 0.25, window_size: 512, max_speech_duration: 20.0 };
            vc.sample_rate = RATE as i32;
            vc.num_threads = 1;
            vc.provider = cpu.as_ptr();
            let mut dc: DenoiserCfg = std::mem::zeroed();
            dc.gtcrn.model = den_m.as_ptr();
            dc.num_threads = 2;
            dc.provider = cpu.as_ptr();
            let ec = EmbCfg { model: emb_m.as_ptr(), num_threads: 2, debug: 0, provider: cpu.as_ptr() };
            let diar = DiarCfg {
                segmentation: SegCfg { pyannote: PyannoteCfg { model: seg_m.as_ptr(), window_shift_ratio: 0.0 }, num_threads: 2, debug: 0, provider: cpu.as_ptr() },
                embedding: EmbCfg { model: emb_m.as_ptr(), num_threads: 2, debug: 0, provider: cpu.as_ptr() },
                // How many people is unknown: clusters closer than the threshold are one.
                clustering: ClusterCfg { num_clusters: -1, threshold: 0.5, compute_confidence: 0 },
                min_duration_on: 0.3,
                min_duration_off: 0.5,
            };
            // The library copies every string while creating each object.
            let fine = Fine { api, vad: (api.vad_create)(&vc, 60.0), denoiser: (api.den_create)(&dc), embedder: (api.emb_create)(&ec), diarizer: (api.diar_create)(&diar), setup: s.clone() };
            if fine.vad.is_null() || fine.denoiser.is_null() || fine.embedder.is_null() || fine.diarizer.is_null() {
                return Err(crate::lang::tr(format!("sherpa-onnx no pudo cargar el oído fino de {}", s.dir.display()), format!("sherpa-onnx could not load the fine ear from {}", s.dir.display())));
            }
            Ok(fine)
        }
    }

    /// The same audio with the noise taken out.
    fn denoise(&self, x: &[f32], rate: u32) -> Vec<f32> {
        if x.is_empty() {
            return Vec::new();
        }
        unsafe {
            let d = (self.api.den_run)(self.denoiser, x.as_ptr(), x.len() as i32, rate as i32);
            if d.is_null() || (*d).samples.is_null() {
                return x.to_vec();
            }
            let out = std::slice::from_raw_parts((*d).samples, (*d).n as usize).to_vec();
            (self.api.denoised_destroy)(d);
            out
        }
    }

    /// Where someone speaks: (first sample, samples) of each stretch of speech.
    fn speech(&self, x: &[f32]) -> Vec<(usize, Vec<f32>)> {
        let a = self.api;
        let mut out = Vec::new();
        unsafe {
            (a.vad_reset)(self.vad);
            for chunk in x.chunks(512) {
                (a.vad_accept)(self.vad, chunk.as_ptr(), chunk.len() as i32);
            }
            (a.vad_flush)(self.vad);
            while (a.vad_empty)(self.vad) == 0 {
                let s = (a.vad_front)(self.vad);
                if !s.is_null() && !(*s).samples.is_null() {
                    out.push(((*s).start.max(0) as usize, std::slice::from_raw_parts((*s).samples, (*s).n as usize).to_vec()));
                }
                if !s.is_null() {
                    (a.segment_destroy)(s);
                }
                (a.vad_pop)(self.vad);
            }
        }
        out
    }

    /// The voiceprint of `x`, unit length; none if it is too short to tell.
    fn embed(&self, x: &[f32], rate: u32) -> Option<Vec<f32>> {
        let a = self.api;
        unsafe {
            let stream = (a.emb_stream)(self.embedder);
            if stream.is_null() {
                return None;
            }
            (a.stream_accept)(stream, rate as i32, x.as_ptr(), x.len() as i32);
            (a.stream_finished)(stream);
            let v = if (a.emb_ready)(self.embedder, stream) != 0 {
                let dim = (a.emb_dim)(self.embedder) as usize;
                let e = (a.emb_compute)(self.embedder, stream);
                let v = (!e.is_null()).then(|| std::slice::from_raw_parts(e, dim).to_vec());
                if !e.is_null() {
                    (a.emb_free)(e);
                }
                v
            } else {
                None
            };
            (a.stream_destroy)(stream);
            v.map(unit)
        }
    }

    /// Who speaks when: (start s, end s, speaker) sorted by time.
    fn diarize(&self, x: &[f32]) -> Vec<(f32, f32, i32)> {
        let a = self.api;
        unsafe {
            let r = (a.diar_process)(self.diarizer, x.as_ptr(), x.len() as i32);
            if r.is_null() {
                return Vec::new();
            }
            let n = (a.diar_segments)(r).max(0) as usize;
            let segs = (a.diar_sorted)(r);
            let out = if segs.is_null() { Vec::new() } else { std::slice::from_raw_parts(segs, n).iter().map(|s| (s.start, s.end, s.speaker)).collect() };
            if !segs.is_null() {
                (a.diar_free_segments)(segs);
            }
            (a.diar_free_result)(r);
            out
        }
    }
}

impl Drop for Fine {
    fn drop(&mut self) {
        unsafe {
            (self.api.vad_destroy)(self.vad);
            (self.api.den_destroy)(self.denoiser);
            (self.api.emb_destroy)(self.embedder);
            (self.api.diar_destroy)(self.diarizer);
        }
    }
}

static FINE: Mutex<Option<Fine>> = Mutex::new(None);

fn with_fine<T>(s: &Setup, f: impl FnOnce(&Fine) -> T) -> Result<T, String> {
    let mut fine = FINE.lock().unwrap_or_else(|e| e.into_inner());
    if fine.as_ref().is_none_or(|x| x.setup != *s) {
        *fine = None;
        *fine = Some(Fine::load(s)?);
    }
    Ok(f(fine.as_ref().expect("loaded above")))
}

// ── voiceprints ──

fn unit(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// Cosine similarity of two unit vectors.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// A voice she has heard: named, or still "voz desconocida N" until someone says whose it is.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Voice {
    pub id: u32,
    pub nombre: String,
    /// Somebody said whose voice this is (or it is hers).
    pub conocida: bool,
    /// Her own voice, from her speakers: never a message.
    #[serde(default)]
    pub propia: bool,
    /// The mean of its voiceprints, unit length.
    pub huella: Vec<f32>,
    /// How many takes went into it.
    pub n: u32,
    /// How this voice usually sounds (mean of its takes): what "louder than usual" is measured
    /// against.
    #[serde(default)]
    pub habitual: Option<Prosody>,
    #[serde(default)]
    pub habitual_n: u32,
}

/// Every voice she knows, kept in `data/voces.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Voices {
    pub voces: Vec<Voice>,
    /// Unknown voices ever made: their numbers are never reused (one named and merged away
    /// must not hand its name to the next).
    #[serde(default)]
    pub desconocidas: u32,
}

/// Who a take is: the voice, how sure, and whether it was new. `id` 0: she cannot tell.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub id: u32,
    pub score: f32,
    pub new: bool,
}

impl Voices {
    pub fn path(data: &Path) -> PathBuf {
        data.join("voces.json")
    }

    pub fn load(data: &Path) -> Voices {
        std::fs::read_to_string(Self::path(data)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, data: &Path) -> Result<(), String> {
        let _ = std::fs::create_dir_all(data);
        std::fs::write(Self::path(data), serde_json::to_string_pretty(self).unwrap_or_default()).map_err(|e| e.to_string())
    }

    pub fn get(&self, id: u32) -> Option<&Voice> {
        self.voces.iter().find(|v| v.id == id)
    }

    pub(crate) fn next_id(&self) -> u32 {
        self.voces.iter().map(|v| v.id).max().unwrap_or(0) + 1
    }

    /// Whose voice `print` is: surely a stored one (`SAME_VOICE`), probably one (`PROBABLY`, with
    /// no other within `MARGIN`), nobody she can tell (id 0), or, far from every voice
    /// (`NEW_BELOW`), a new one, kept as unknown so the next take of that person is recognised
    /// as it until someone names it.
    pub fn identify(&mut self, print: &[f32]) -> Match {
        let mut scored: Vec<(u32, f32)> = self.voces.iter().map(|v| (v.id, similarity(&v.huella, print))).collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let second = scored.get(1).map_or(-1.0, |x| x.1);
        if let Some(&(id, score)) = scored.first() {
            let margin = score - second;
            if score >= SAME_VOICE || (score >= PROBABLY && margin >= MARGIN) {
                let v = self.voces.iter_mut().find(|v| v.id == id).expect("scored from the list");
                if !v.propia && (score >= LEARN_FROM || (score >= LEARN_PROBABLE && margin >= LEARN_MARGIN)) {
                    v.learn(print);
                }
                return Match { id, score, new: false };
            }
            if score >= NEW_BELOW {
                return Match { id: 0, score, new: false };
            }
        }
        let id = self.next_id();
        self.desconocidas += 1;
        let k = self.desconocidas;
        let nombre = crate::lang::tr(format!("voz desconocida {k}"), format!("unknown voice {k}"));
        self.voces.push(Voice { id, nombre, conocida: false, propia: false, huella: print.to_vec(), n: 1, habitual: None, habitual_n: 0 });
        Match { id, score: 1.0, new: true }
    }

    /// Adds a piece of her own voice to her voiceprint (until it has enough).
    pub fn learn_own(&mut self, print: &[f32]) {
        match self.voces.iter_mut().find(|v| v.propia) {
            Some(v) if v.n >= OWN_PIECES => {}
            Some(v) => v.learn(print),
            None => {
                let id = self.next_id();
                self.voces.push(Voice { id, nombre: OWN_NAME.into(), conocida: true, propia: true, huella: print.to_vec(), n: 1, habitual: None, habitual_n: 0 });
            }
        }
    }

    /// Someone said whose voice `id` is. If that name already has a voice, both become one.
    pub fn name(&mut self, id: u32, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::lang::tr("falta el nombre", "the name is missing"));
        }
        let Some(i) = self.voces.iter().position(|v| v.id == id) else { return Err(crate::lang::tr("no conozco esa voz", "I don't know that voice")) };
        if self.voces[i].propia {
            return Err(crate::lang::tr("esa es su propia voz", "that is her own voice"));
        }
        if let Some(j) = self.voces.iter().position(|v| v.id != id && v.conocida && v.nombre.eq_ignore_ascii_case(name)) {
            let gone = self.voces.remove(i);
            let j = if j > i { j - 1 } else { j };
            let keep = &mut self.voces[j];
            let total = (keep.n + gone.n) as f32;
            let mixed: Vec<f32> = keep.huella.iter().zip(&gone.huella).map(|(a, b)| (a * keep.n as f32 + b * gone.n as f32) / total).collect();
            keep.huella = unit(mixed);
            keep.n += gone.n;
            return Ok(());
        }
        let v = &mut self.voces[i];
        v.nombre = name.to_string();
        v.conocida = true;
        Ok(())
    }
}

impl Voice {
    /// One more take of how it sounds, into its habitual mean (only takes with enough voice).
    pub fn hear_prosody(&mut self, p: &Prosody) {
        if p.voiced < MIN_PRINT_S {
            return;
        }
        let n = self.habitual_n.min(50) as f32;
        let h = self.habitual.unwrap_or(*p);
        let mix = |a: f32, b: f32| (a * n + b) / (n + 1.0);
        self.habitual = Some(Prosody { db: mix(h.db, p.db), f0: mix(h.f0, p.f0), f0_var: mix(h.f0_var, p.f0_var), voiced: p.voiced });
        self.habitual_n += 1;
    }

    fn learn(&mut self, print: &[f32]) {
        let n = self.n as f32;
        let mixed: Vec<f32> = self.huella.iter().zip(print).map(|(a, b)| (a * n + b) / (n + 1.0)).collect();
        self.huella = unit(mixed);
        self.n += 1;
    }
}

// ── listening ──

/// One person's words in a take.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Turn {
    /// The voice's id in `data/voces.json`; 0 when the fine ear is not there to tell.
    pub voz: u32,
    pub nombre: String,
    pub conocida: bool,
    /// Similarity to the stored voiceprint (1 for a voice heard for the first time).
    pub seguridad: f32,
    pub texto: String,
    /// Seconds from the start of the take.
    pub inicio: f32,
    /// A voice she does not know (or could not tell) keeps a clip of this turn, to hear whose
    /// it is (`keep_clip`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip: Option<String>,
}

/// Turns of the take, split by speaker, with her own voice dropped. `data` is where the
/// voiceprints are kept. Without the fine ear it is plain Whisper, as one turn of nobody.
///
/// The cleaned audio only decides where someone speaks. Whisper, the voiceprints and the
/// split by speaker get the microphone's own samples: measured (`the_cleaner_measured_against_
/// noise`), GTCRN's artefacts cost Whisper more words than the noise it takes out (41 → 24 of
/// 64 at 0 dB, 57 → 50 at 20 dB) and pull the voiceprint away from the speaker's.
pub fn listen(ear: &crate::asr::Setup, fine: Option<&Setup>, data: &Path, samples: &[f32]) -> Result<Vec<Turn>, String> {
    let Some(fine) = fine else {
        let text = crate::asr::transcribe(ear, samples, RATE)?;
        return Ok(if text.is_empty() { Vec::new() } else { vec![Turn { voz: 0, nombre: String::new(), conocida: false, seguridad: 0.0, texto: text, inicio: 0.0, clip: None }] });
    };
    let pieces = with_fine(fine, |f| {
        let speech = f.speech(&f.denoise(samples, RATE));
        let total: usize = speech.iter().map(|(_, s)| s.len()).sum();
        let mut pieces: Vec<(f32, Vec<f32>, Option<Vec<f32>>)> = Vec::new();
        if total as f32 / RATE as f32 > LONG_TAKE_S {
            // Long: pyannote says who speaks when; each speaker's voiceprint from all of theirs.
            let segs = merge_turns(f.diarize(samples));
            let mut prints: std::collections::HashMap<i32, Option<Vec<f32>>> = std::collections::HashMap::new();
            for &(_, _, who) in &segs {
                prints.entry(who).or_insert_with(|| {
                    let all: Vec<f32> = segs.iter().filter(|s| s.2 == who).flat_map(|&(a, b, _)| slice(samples, a, b).iter().copied()).collect();
                    enough(&all).then(|| f.embed(&all, RATE)).flatten()
                });
            }
            for (a, b, who) in segs {
                pieces.push((a, slice(samples, a, b).to_vec(), prints[&who].clone()));
            }
        } else {
            // Short: one voiceprint per stretch of speech; neighbours of the same voice merge.
            for (start, cleaned) in speech {
                let s = &samples[start.min(samples.len())..(start + cleaned.len()).min(samples.len())];
                let print = enough(s).then(|| f.embed(s, RATE)).flatten();
                let at = start as f32 / RATE as f32;
                match pieces.last_mut() {
                    Some((_, prev, Some(p))) if print.as_ref().is_none_or(|q| similarity(p, q) >= SAME_VOICE) => prev.extend_from_slice(s),
                    _ => pieces.push((at, s.to_vec(), print)),
                }
            }
            // A stretch too short for a voiceprint takes its neighbour's.
            for i in 0..pieces.len() {
                if pieces[i].2.is_none() {
                    pieces[i].2 = pieces.iter().filter_map(|p| p.2.clone()).next();
                }
            }
        }
        pieces
    })?;
    let mut voices = Voices::load(data);
    let mut turns = Vec::new();
    for (at, audio, print) in pieces {
        let (id, score) = match &print {
            Some(p) => {
                let m = voices.identify(p);
                (m.id, m.score)
            }
            None => (0, 0.0),
        };
        let v = voices.get(id).cloned();
        if v.as_ref().is_some_and(|v| v.propia) {
            continue;
        }
        let text = crate::asr::transcribe(ear, &audio, RATE)?;
        if text.is_empty() {
            continue;
        }
        let known = v.as_ref().is_some_and(|v| v.conocida);
        let clip = if known { None } else { keep_clip(data, id, &audio) };
        turns.push(Turn { voz: id, nombre: v.as_ref().map(|v| v.nombre.clone()).unwrap_or_default(), conocida: known, seguridad: score, texto: text, inicio: at, clip });
    }
    voices.save(data)?;
    Ok(turns)
}

/// Her own voice, as it goes out: its voiceprint is learnt so that when the microphone hears
/// it back from the speakers, it is not taken for anybody's message.
pub fn learn_own(fine: &Setup, data: &Path, samples: &[f32], rate: u32) {
    // Under a second and a half there is not enough voice to tell.
    if (samples.len() as f32) < rate as f32 * 1.5 {
        return;
    }
    let mut voices = Voices::load(data);
    if voices.voces.iter().any(|v| v.propia && v.n >= OWN_PIECES) {
        return;
    }
    if let Ok(Some(print)) = with_fine(fine, |f| f.embed(samples, rate)) {
        voices.learn_own(&print);
        let _ = voices.save(data);
    }
}

// ── clips of the voices she does not know ──

/// Clips kept of each unknown voice, and of takes she could not tell (voice 0): the newest.
const CLIPS_PER_VOICE: usize = 5;
const CLIPS_UNTOLD: usize = 10;

fn clips_root(data: &Path) -> PathBuf {
    data.join("voces")
}

/// Keeps `x` as a clip of voice `voz` (0: nobody she could tell), so whoever hears it can say
/// whose it is: `data/voces/<voz>/<unix ms>.wav`, only the newest few. Returns its name.
pub fn keep_clip(data: &Path, voz: u32, x: &[f32]) -> Option<String> {
    let dir = clips_root(data).join(voz.to_string());
    std::fs::create_dir_all(&dir).ok()?;
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_millis();
    let name = format!("{ms}.wav");
    std::fs::write(dir.join(&name), crate::download::wav(x, RATE)).ok()?;
    let mut all: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "wav")).collect();
    all.sort();
    let keep = if voz == 0 { CLIPS_UNTOLD } else { CLIPS_PER_VOICE };
    for old in all.iter().take(all.len().saturating_sub(keep)) {
        let _ = std::fs::remove_file(old);
    }
    Some(format!("{voz}/{name}"))
}

/// The file of a clip named by `keep_clip`; nothing else (no way out of `data/voces`).
pub fn clip_path(data: &Path, clip: &str) -> Option<PathBuf> {
    let (voz, file) = clip.split_once('/')?;
    let stem = file.strip_suffix(".wav")?;
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    (digits(voz) && digits(stem)).then(|| clips_root(data).join(voz).join(file)).filter(|p| p.is_file())
}

/// Forgets the clips of a voice that no longer exists (merged into another).
pub fn drop_clips(data: &Path, voz: u32) {
    if voz != 0 {
        let _ = std::fs::remove_dir_all(clips_root(data).join(voz.to_string()));
    }
}

/// Someone heard a clip and said whose voice it is: its voiceprint goes to the voice of that
/// name (a new one if there is none) and the clip, registered, is deleted. Returns the voice.
pub fn enroll_clip(fine: &Setup, data: &Path, clip: &str, name: &str) -> Result<u32, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::lang::tr("falta el nombre", "the name is missing"));
    }
    let path = clip_path(data, clip).ok_or_else(|| crate::lang::tr("ese clip ya no existe", "that clip no longer exists"))?;
    let (x, r) = crate::voice::read_wav(&path)?;
    // A second of voice is the least anybody can be told from (the voice was named by a person,
    // so less than MIN_PRINT_S is accepted here).
    if (x.len() as f32) < r as f32 {
        return Err(crate::lang::tr("el clip es demasiado corto para reconocer esa voz", "the clip is too short to recognise that voice"));
    }
    let print = with_fine(fine, |f| f.embed(&x, r))?.ok_or_else(|| crate::lang::tr("no se pudo sacar la huella del clip", "no voiceprint could be taken from the clip"))?;
    let mut voices = Voices::load(data);
    if voices.voces.iter().any(|v| v.propia && v.nombre.eq_ignore_ascii_case(name)) {
        return Err(crate::lang::tr("esa es su propia voz", "that is her own voice"));
    }
    let id = match voices.voces.iter_mut().find(|v| v.conocida && !v.propia && v.nombre.eq_ignore_ascii_case(name)) {
        Some(v) => {
            v.learn(&print);
            v.id
        }
        None => {
            let id = voices.next_id();
            voices.voces.push(Voice { id, nombre: name.to_string(), conocida: true, propia: false, huella: print, n: 1, habitual: None, habitual_n: 0 });
            id
        }
    };
    voices.save(data)?;
    let _ = std::fs::remove_file(&path);
    Ok(id)
}

// ── the open microphone ──

/// A VAD of its own for the open microphone: fed as the audio comes, it hands back each stretch
/// of speech once a pause closes it.
pub struct Live {
    api: &'static Api,
    vad: P,
}

// Used from the open microphone's thread only.
unsafe impl Send for Live {}

impl Live {
    pub fn new(s: &Setup) -> Result<Live, String> {
        let api = api(&s.dll)?;
        let model = crate::voice::cstr(&s.dir.join("silero_vad.onnx"));
        let cpu = CString::new("cpu").unwrap();
        unsafe {
            let mut vc: VadCfg = std::mem::zeroed();
            // A pause of 0.6 s closes a stretch; the turn ends later, after a longer silence.
            vc.silero_vad = SileroCfg { model: model.as_ptr(), threshold: 0.5, min_silence_duration: 0.6, min_speech_duration: 0.25, window_size: 512, max_speech_duration: 25.0 };
            vc.sample_rate = RATE as i32;
            vc.num_threads = 1;
            vc.provider = cpu.as_ptr();
            let vad = (api.vad_create)(&vc, 60.0);
            if vad.is_null() {
                return Err(crate::lang::tr("sherpa-onnx no pudo cargar el detector de voz", "sherpa-onnx could not load the voice detector"));
            }
            Ok(Live { api, vad })
        }
    }

    /// Feeds what the microphone just heard; returns the stretches of speech it closed.
    pub fn accept(&mut self, x: &[f32]) -> Vec<Vec<f32>> {
        let a = self.api;
        let mut out = Vec::new();
        unsafe {
            (a.vad_accept)(self.vad, x.as_ptr(), x.len() as i32);
            while (a.vad_empty)(self.vad) == 0 {
                let seg = (a.vad_front)(self.vad);
                if !seg.is_null() {
                    if !(*seg).samples.is_null() {
                        out.push(std::slice::from_raw_parts((*seg).samples, (*seg).n as usize).to_vec());
                    }
                    (a.segment_destroy)(seg);
                }
                (a.vad_pop)(self.vad);
            }
        }
        out
    }

    /// Someone is speaking right now.
    pub fn speaking(&self) -> bool {
        unsafe { (self.api.vad_detected)(self.vad) != 0 }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        unsafe { (self.api.vad_destroy)(self.vad) };
    }
}

/// Whose voice a stretch is, as dictation reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Who {
    pub voz: u32,
    pub nombre: String,
    pub conocida: bool,
    pub propia: bool,
    pub seguridad: f32,
    /// How this voice usually sounds, if she has heard it enough.
    pub habitual: Option<Prosody>,
}

/// Whose voice `x` is (none if it is too short to tell); a new voice is kept as unknown, and a
/// sure match also learns how this take sounded.
pub fn recognise(fine: &Setup, data: &Path, x: &[f32], sounds: &Prosody) -> Result<Option<Who>, String> {
    if !enough(x) {
        return Ok(None);
    }
    let Some(print) = with_fine(fine, |f| f.embed(x, RATE))? else { return Ok(None) };
    let mut voices = Voices::load(data);
    let m = voices.identify(&print);
    let Some(v) = voices.voces.iter_mut().find(|v| v.id == m.id) else { return Ok(None) };
    let before = v.habitual;
    if !v.propia && (m.new || m.score >= LEARN_FROM) {
        v.hear_prosody(sounds);
    }
    let who = Who { voz: v.id, nombre: v.nombre.clone(), conocida: v.conocida, propia: v.propia, seguridad: m.score, habitual: before };
    voices.save(data)?;
    Ok(Some(who))
}

/// Is `x` her own voice? None while it cannot tell (too short, or her voice not learnt yet):
/// she must not cut herself off because she does not know her own voice yet.
pub fn is_own(fine: &Setup, data: &Path, x: &[f32]) -> Option<bool> {
    if !enough(x) {
        return None;
    }
    let voices = Voices::load(data);
    let own = voices.voces.iter().find(|v| v.propia)?;
    let print = with_fine(fine, |f| f.embed(x, RATE)).ok()??;
    Some(similarity(&own.huella, &print) >= OWN_VOICE)
}

/// How a stretch sounds: loudness and pitch of its voiced part.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Prosody {
    /// RMS of the voiced frames, dBFS.
    pub db: f32,
    /// Median pitch of the voiced frames, Hz.
    pub f0: f32,
    /// Spread of the pitch, in semitones (flat ~1, lively 3+).
    pub f0_var: f32,
    /// Seconds of voiced speech.
    pub voiced: f32,
}

/// Loudness and pitch of `x` (16 kHz): 40 ms frames every 20 ms, voiced where the normalised
/// autocorrelation has a clear peak between 60 and 400 Hz.
pub fn prosody(x: &[f32]) -> Prosody {
    const FRAME: usize = 640;
    const HOP: usize = 320;
    let (min_lag, max_lag) = ((RATE / 400) as usize, (RATE / 60) as usize);
    let (mut f0s, mut energy) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i + FRAME <= x.len() {
        let f = &x[i..i + FRAME];
        let e = f.iter().map(|v| v * v).sum::<f32>() / FRAME as f32;
        i += HOP;
        // Below -45 dBFS it is silence or breath, not voice.
        if e < 10f32.powf(-4.5) {
            continue;
        }
        let r0: f32 = f.iter().map(|v| v * v).sum();
        let (mut best, mut lag) = (0.0f32, 0usize);
        for l in min_lag..=max_lag.min(FRAME - 1) {
            let r: f32 = (0..FRAME - l).map(|k| f[k] * f[k + l]).sum::<f32>() / r0;
            if r > best {
                best = r;
                lag = l;
            }
        }
        if best > 0.5 && lag > 0 {
            f0s.push(RATE as f32 / lag as f32);
            energy.push(e);
        }
    }
    if f0s.is_empty() {
        return Prosody::default();
    }
    let db = 10.0 * (energy.iter().sum::<f32>() / energy.len() as f32).log10();
    let mut sorted = f0s.clone();
    sorted.sort_by(f32::total_cmp);
    let f0 = sorted[sorted.len() / 2];
    let semis: Vec<f32> = f0s.iter().map(|f| 12.0 * (f / f0).log2()).collect();
    let mean = semis.iter().sum::<f32>() / semis.len() as f32;
    let f0_var = (semis.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / semis.len() as f32).sqrt();
    Prosody { db, f0, f0_var, voiced: f0s.len() as f32 * HOP as f32 / RATE as f32 }
}

/// The voiceprint of `x`, for measurements in tests.
#[cfg(test)]
pub fn tests_embed(fine: &Setup, x: &[f32]) -> Option<Vec<f32>> {
    with_fine(fine, |f| f.embed(x, RATE)).ok()?
}

/// Enough voice for a voiceprint.
fn enough(x: &[f32]) -> bool {
    x.len() as f32 >= MIN_PRINT_S * RATE as f32
}

/// Consecutive pieces of the same speaker, one turn.
fn merge_turns(segs: Vec<(f32, f32, i32)>) -> Vec<(f32, f32, i32)> {
    let mut out: Vec<(f32, f32, i32)> = Vec::new();
    for s in segs {
        match out.last_mut() {
            Some(last) if last.2 == s.2 => last.1 = s.1,
            _ => out.push(s),
        }
    }
    out
}

fn slice(x: &[f32], a: f32, b: f32) -> &[f32] {
    let i = ((a.max(0.0) * RATE as f32) as usize).min(x.len());
    let j = ((b.max(0.0) * RATE as f32) as usize).clamp(i, x.len());
    &x[i..j]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_matches_sherpa_onnx_1_13_8() {
        // Sizes of the C structs on x64, worked out from c-api.h of that version.
        use std::mem::size_of;
        assert_eq!(size_of::<SileroCfg>(), 32);
        assert_eq!(size_of::<VadCfg>(), 88);
        assert_eq!(std::mem::offset_of!(VadCfg, provider), 40);
        assert_eq!(std::mem::offset_of!(VadCfg, ten_vad), 56);
        assert_eq!(size_of::<SpeechSegment>(), 24);
        assert_eq!(size_of::<EmbCfg>(), 24);
        assert_eq!(size_of::<SegCfg>(), 32);
        assert_eq!(size_of::<ClusterCfg>(), 12);
        assert_eq!(size_of::<DiarCfg>(), 80);
        assert_eq!(std::mem::offset_of!(DiarCfg, clustering), 56);
        assert_eq!(size_of::<DiarSeg>(), 16);
        assert_eq!(size_of::<DenoiserCfg>(), 40);
        assert_eq!(std::mem::offset_of!(DenoiserCfg, dpdfnet), 24);
        assert_eq!(size_of::<Denoised>(), 16);
    }

    fn v(xs: &[f32]) -> Vec<f32> {
        unit(xs.to_vec())
    }

    #[test]
    fn an_unknown_voice_is_kept_and_recognised_the_next_time() {
        let mut vs = Voices::default();
        let a = vs.identify(&v(&[1.0, 0.0, 0.0]));
        assert!(a.new);
        let again = vs.identify(&v(&[0.95, 0.1, 0.0]));
        assert_eq!((again.id, again.new), (a.id, false));
        let b = vs.identify(&v(&[0.0, 1.0, 0.0]));
        assert!(b.new && b.id != a.id, "an orthogonal voice is someone else");
        assert_eq!(vs.voces.len(), 2);
        assert!(!vs.get(a.id).unwrap().conocida);
    }

    /// The real case: a quiet voice at 0.58 of its own voiceprint, nobody else near, is that
    /// voice (probably), not a new unknown one; halfway from everyone, she cannot tell; far
    /// from everyone, it is new.
    #[test]
    fn a_real_microphone_take_is_probably_its_voice_not_a_new_one() {
        let mut vs = Voices::default();
        let ana = vs.identify(&v(&[1.0, 0.0, 0.0, 0.0])).id;
        vs.name(ana, "Ana").unwrap();
        vs.learn_own(&v(&[0.0, 0.0, 0.0, 1.0]));
        // cos 0.58 to Ana, 0.0 to her.
        let take = v(&[0.58, (1.0f32 - 0.58 * 0.58).sqrt(), 0.0, 0.0]);
        let before = vs.clone();
        let m = vs.identify(&take);
        assert_eq!((m.id, m.new), (ana, false));
        assert!((m.score - 0.58).abs() < 1e-3);
        assert_eq!(vs.voces.len(), 2, "no new unknown voice");
        assert!(vs.get(ana).unwrap().huella != before.get(ana).unwrap().huella, "nobody else near: Ana's voiceprint learns from it");
        let mut vs = before;
        let unsure = vs.identify(&v(&[0.45, 0.893, 0.0, 0.0]));
        assert_eq!(unsure.id, 0, "0.45: she cannot tell");
        let new = vs.identify(&v(&[0.1, 0.0, 0.995, 0.0]));
        assert!(new.new);
        // Ana was unknown voice 1 before she was named: numbers are not reused.
        assert_eq!(vs.get(new.id).unwrap().nombre, crate::lang::tr("voz desconocida 2", "unknown voice 2"));
    }

    #[test]
    fn an_unknown_voices_number_is_never_reused() {
        let mut vs = Voices::default();
        let a = vs.identify(&v(&[1.0, 0.0, 0.0])).id;
        vs.name(a, "Ana").unwrap();
        let b = vs.identify(&v(&[0.0, 1.0, 0.0])).id;
        assert_eq!(vs.get(b).unwrap().nombre, crate::lang::tr("voz desconocida 2", "unknown voice 2"), "the first one was named: the next is the second");
    }

    #[test]
    fn naming_a_voice_and_merging_two_takes_of_the_same_person() {
        let mut vs = Voices::default();
        let a = vs.identify(&v(&[1.0, 0.0, 0.0])).id;
        vs.name(a, "Ana").unwrap();
        assert!(vs.get(a).unwrap().conocida);
        // Same person with a cold: another unknown voice, then named Ana too → one voice.
        let b = vs.identify(&v(&[0.2, 1.0, 0.0])).id;
        assert_ne!(a, b);
        vs.name(b, "ana").unwrap();
        assert_eq!(vs.voces.len(), 1);
        assert_eq!(vs.voces[0].n, 2);
        assert!((vs.voces[0].huella.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn her_own_voice_is_learnt_and_cannot_be_renamed() {
        let mut vs = Voices::default();
        for _ in 0..20 {
            vs.learn_own(&v(&[0.0, 0.0, 1.0]));
        }
        let own = vs.voces.iter().find(|x| x.propia).unwrap().clone();
        assert_eq!(own.n, OWN_PIECES);
        assert_eq!(vs.identify(&v(&[0.05, 0.0, 1.0])).id, own.id);
        assert!(vs.name(own.id, "Ana").is_err());
    }

    // ── with the real models (each test skips if they are not downloaded) ──

    /// Linear resampling, enough to bring test recordings to 16 kHz.
    fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
        if from == to {
            return x.to_vec();
        }
        let n = (x.len() as u64 * to as u64 / from as u64) as usize;
        (0..n)
            .map(|i| {
                let p = i as f64 * from as f64 / to as f64;
                let (k, f) = (p as usize, (p - p.floor()) as f32);
                x.get(k).copied().unwrap_or(0.0) * (1.0 - f) + x.get(k + 1).copied().unwrap_or(0.0) * f
            })
            .collect()
    }

    fn wav16(path: &str) -> Option<Vec<f32>> {
        let (s, r) = crate::voice::read_wav(Path::new(path)).ok()?;
        Some(resample(&s, r, RATE))
    }

    fn secs(x: &[f32], a: f32, b: f32) -> Vec<f32> {
        slice(x, a, b).to_vec()
    }

    fn spoken16(m: &Manifest, voice: &str, line: &str) -> Option<Vec<f32>> {
        let s = crate::voice::setup(m, voice, 1.0).ok()?;
        let (x, r) = crate::voice::synth_for_test(&s, line).ok()?;
        Some(resample(&x, r, RATE))
    }

    fn ready() -> Option<(Manifest, Setup)> {
        let m = Manifest::load();
        let s = setup(&m);
        if s.is_none() {
            eprintln!("oído fino no descargado, se salta");
        }
        s.map(|s| (m, s))
    }

    const BRIA: &str = "modelos/voz/sherpa-onnx-pocket-tts-int8-2026-01-26/test_wavs/bria.wav";
    const HIBIKI: &str = "modelos/voz/sherpa-onnx-pocket-tts-int8-2026-01-26/test_wavs/sample_fr_hibiki_crepes.wav";
    const GAME: &str = "modelos/voz/kaltsit/referencia.wav";

    /// Three real people (two recordings each, from different parts of the take): the same
    /// person is above `SAME_VOICE`, two different people below it.
    #[test]
    #[cfg(windows)]
    fn voiceprints_tell_real_people_apart() {
        let Some((_, s)) = ready() else { return };
        let (Some(bria), Some(hibiki), Some(game)) = (wav16(BRIA), wav16(HIBIKI), wav16(GAME)) else {
            eprintln!("faltan las grabaciones de prueba, se salta");
            return;
        };
        let takes = [("bria A", secs(&bria, 1.0, 7.0)), ("bria B", secs(&bria, 25.0, 31.0)), ("hibiki A", secs(&hibiki, 2.0, 8.0)), ("hibiki B", secs(&hibiki, 30.0, 36.0)), ("juego A", secs(&game, 0.0, 5.0)), ("juego B", secs(&game, 5.0, 10.0))];
        let prints: Vec<Vec<f32>> = takes.iter().map(|(_, x)| with_fine(&s, |f| f.embed(x, RATE)).unwrap().expect("6 s are enough for a voiceprint")).collect();
        let (mut worst_same, mut worst_other) = (1.0f32, -1.0f32);
        for i in 0..takes.len() {
            let row: Vec<String> = (0..takes.len()).map(|j| format!("{:5.2}", similarity(&prints[i], &prints[j]))).collect();
            eprintln!("{:9} {}", takes[i].0, row.join(" "));
            for j in i + 1..takes.len() {
                let sim = similarity(&prints[i], &prints[j]);
                if i / 2 == j / 2 {
                    worst_same = worst_same.min(sim);
                } else {
                    worst_other = worst_other.max(sim);
                }
            }
        }
        eprintln!("misma persona, la peor: {worst_same:.2} · personas distintas, la más parecida: {worst_other:.2} · umbral {SAME_VOICE}");
        assert!(worst_same >= SAME_VOICE && worst_other < SAME_VOICE);
        // A dictation is often shorter: the same, with 2.5 s takes (reported, not asserted).
        let short = [secs(&bria, 1.0, 3.5), secs(&bria, 25.0, 27.5), secs(&hibiki, 2.0, 4.5), secs(&hibiki, 30.0, 32.5), secs(&game, 0.5, 3.0), secs(&game, 6.0, 8.5)];
        let sp: Vec<Option<Vec<f32>>> = short.iter().map(|x| with_fine(&s, |f| f.embed(x, RATE)).unwrap()).collect();
        let (mut same, mut other) = (1.0f32, -1.0f32);
        for i in 0..6 {
            for j in i + 1..6 {
                if let (Some(a), Some(b)) = (&sp[i], &sp[j]) {
                    let x = similarity(a, b);
                    if i / 2 == j / 2 { same = same.min(x) } else { other = other.max(x) }
                }
            }
        }
        eprintln!("tomas de 2,5 s: misma persona, la peor {same:.2} · distintas, la más parecida {other:.2}");
        // The bad one has little voice in it: with MIN_PRINT_S of speech or more, short takes
        // still keep people apart (a take with less is never identified on its own).
        let voiced: Vec<bool> = short.iter().map(|x| with_fine(&s, |f| f.speech(x)).unwrap().iter().map(|(_, v)| v.len()).sum::<usize>() as f32 >= MIN_PRINT_S * RATE as f32).collect();
        let (mut same, mut other) = (1.0f32, -1.0f32);
        for i in 0..6 {
            for j in i + 1..6 {
                if let (true, true, Some(a), Some(b)) = (voiced[i], voiced[j], &sp[i], &sp[j]) {
                    let x = similarity(a, b);
                    if i / 2 == j / 2 { same = same.min(x) } else { other = other.max(x) }
                }
            }
        }
        eprintln!("tomas de 2,5 s con ≥ {MIN_PRINT_S} s de voz: misma persona, la peor {same:.2} · distintas, la más parecida {other:.2}");
        assert!(same >= SAME_VOICE && other < SAME_VOICE);
    }

    /// The cleaner, measured: a clean Spanish sentence between two seconds of silence, with
    /// noise (white plus mains hum) at 0, 10 and 20 dB SNR. For each, the noisy and the cleaned
    /// take: SNR against the clean one, words Whisper gets right, how close the voiceprint stays
    /// to the clean one's, and how much of the silence the VAD wrongly takes for speech.
    #[test]
    #[cfg(windows)]
    fn the_cleaner_measured_against_noise() {
        let Some((m, s)) = ready() else { return };
        let line = "Mañana por la mañana tengo que revisar el informe de ventas y llamar a mi hermana.";
        let Some(voice) = spoken16(&m, "voz-daniela", line) else { return eprintln!("voz-daniela no descargada, se salta") };
        let Ok(ear) = crate::asr::setup(&m, "es") else { return eprintln!("oído no descargado, se salta") };
        let pad = vec![0.0; 2 * RATE as usize];
        let clean: Vec<f32> = [pad.clone(), voice.clone(), pad.clone()].concat();
        let (v0, v1) = (pad.len(), pad.len() + voice.len());
        let power = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let raw: Vec<f32> = (0..clean.len())
            .map(|i| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((seed >> 33) as f32 / (1u64 << 31) as f32 - 0.5) + 0.3 * (i as f32 * 50.0 * std::f32::consts::TAU / RATE as f32).sin()
            })
            .collect();
        let snr = |x: &[f32]| {
            let n = x.len().min(clean.len());
            let err: f32 = (0..n).map(|i| (x[i] - clean[i]).powi(2)).sum::<f32>() / n as f32;
            10.0 * (power(&clean[v0..v1]) / err).log10()
        };
        let words = |t: &str| t.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().map(String::from).collect::<Vec<_>>();
        let said = words(line);
        let hits = |t: &str| {
            let got = words(t);
            said.iter().filter(|w| got.contains(w)).count()
        };
        let print = |x: &[f32]| with_fine(&s, |f| f.embed(x, RATE)).unwrap().unwrap();
        let p0 = print(&voice);
        // Seconds of the 4 s of silence that the VAD keeps as speech.
        let false_speech = |x: &[f32]| {
            let segs = with_fine(&s, |f| f.speech(x)).unwrap();
            segs.iter().map(|(a, seg)| { let b = a + seg.len(); (b.min(v0).saturating_sub(*a) + b.saturating_sub((*a).max(v1))) as f32 }).sum::<f32>() / RATE as f32
        };
        eprintln!("  dB | toma    |  SNR  | palabras | huella | silencio tomado por voz");
        let mut rows = Vec::new();
        for db in [0.0f32, 10.0, 20.0] {
            let k = (power(&clean[v0..v1]) / power(&raw) / 10f32.powf(db / 10.0)).sqrt();
            let noisy: Vec<f32> = clean.iter().zip(&raw).map(|(c, n)| c + n * k).collect();
            let cleaned = with_fine(&s, |f| f.denoise(&noisy, RATE)).unwrap();
            let mut row = Vec::new();
            for (name, x) in [("ruido", &noisy), ("limpia", &cleaned)] {
                let (h, sim, fs) = (hits(&crate::asr::transcribe(&ear, x, RATE).unwrap()), similarity(&p0, &print(&x[v0..v1.min(x.len())])), false_speech(x));
                eprintln!("  {db:2} | {name:7} | {:5.1} | {h:2}/{:2}    | {sim:5.2}  | {fs:.1} s", snr(x), said.len());
                row.push((snr(x), h, sim, fs));
            }
            rows.push(row);
        }
        // What the pipeline relies on: the cleaner really removes noise.
        assert!(rows.iter().all(|r| r[1].0 > r[0].0), "the cleaner raises the SNR at every level");
        // Whisper on both, four noises per level (one run varies by a couple of words).
        if std::env::var("KALTSIT_BARRIDO").is_err() {
            return;
        }
        for db in [0.0f32, 10.0, 20.0] {
            let (mut a, mut b) = (0, 0);
            for k in 0..4u64 {
                let mut seed = 0x9E37_79B9_7F4A_7C15u64 ^ (k * 0x1234_5678_9ABC);
                let noise: Vec<f32> = (0..clean.len())
                    .map(|i| {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                        ((seed >> 33) as f32 / (1u64 << 31) as f32 - 0.5) + 0.3 * (i as f32 * 50.0 * std::f32::consts::TAU / RATE as f32).sin()
                    })
                    .collect();
                let g = (power(&clean[v0..v1]) / power(&noise) / 10f32.powf(db / 10.0)).sqrt();
                let noisy: Vec<f32> = clean.iter().zip(&noise).map(|(c, n)| c + n * g).collect();
                let cleaned = with_fine(&s, |f| f.denoise(&noisy, RATE)).unwrap();
                a += hits(&crate::asr::transcribe(&ear, &noisy, RATE).unwrap());
                b += hits(&crate::asr::transcribe(&ear, &cleaned, RATE).unwrap());
            }
            eprintln!("  {db:2} dB, 4 ruidos: Whisper con ruido {a}/{} · limpiado {b}/{}", 4 * said.len(), 4 * said.len());
        }
    }

    /// A long take with two people (a real recording around a TTS voice): split by speaker, the
    /// two outer turns are the same voice and the middle one another; a second take of the
    /// first person is recognised as the same voice.
    #[test]
    #[cfg(windows)]
    fn a_long_take_is_split_by_speaker() {
        let Some((m, s)) = ready() else { return };
        let Ok(ear) = crate::asr::setup(&m, "es") else { return eprintln!("oído no descargado, se salta") };
        let Some(bria) = wav16(BRIA) else { return eprintln!("falta bria.wav, se salta") };
        let Some(dani) = spoken16(&m, "voz-daniela", "Hola, soy otra persona y estoy hablando en medio de la grabación, así que deberías notar que mi voz es distinta.") else { return };
        let gap = vec![0.0; RATE as usize / 2];
        let take: Vec<f32> = [secs(&bria, 1.0, 7.0), gap.clone(), dani, gap, secs(&bria, 10.0, 16.0)].concat();
        let data = std::env::temp_dir().join(format!("kaltsit-voces-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let t0 = std::time::Instant::now();
        let turns = listen(&ear, Some(&s), &data, &take).unwrap();
        eprintln!("{:.1} s de audio en {:?}", take.len() as f32 / RATE as f32, t0.elapsed());
        for t in &turns {
            eprintln!("  {:5.1} s · voz {} ({}) · {:.2} · {}", t.inicio, t.voz, t.nombre, t.seguridad, t.texto);
        }
        let ids: Vec<u32> = turns.iter().map(|t| t.voz).collect();
        assert!(turns.iter().all(|t| t.clip.as_ref().is_some_and(|c| clip_path(&data, c).is_some())), "each unknown voice kept a clip");
        assert!(ids.len() >= 3, "three turns at least: {ids:?}");
        assert_eq!(ids.first(), ids.last(), "the first and last turn are the same person");
        assert!(ids.iter().any(|&i| i != ids[0]), "someone else in between");
        // Later, a short take of the first person alone: the same voice, not a new one.
        let again = listen(&ear, Some(&s), &data, &secs(&bria, 30.0, 36.0)).unwrap();
        eprintln!("otra vez: voz {} · {:.2}", again[0].voz, again[0].seguridad);
        assert_eq!(again[0].voz, ids[0]);
        let _ = std::fs::remove_dir_all(&data);
    }

    /// Her voice, learnt from what she says, is dropped when the microphone hears it back;
    /// somebody else's voice in the same conditions is not.
    #[test]
    #[cfg(windows)]
    fn her_own_voice_from_the_speakers_is_not_a_message() {
        let Some((m, s)) = ready() else { return };
        let Ok(ear) = crate::asr::setup(&m, "en") else { return eprintln!("oído no descargado, se salta") };
        let data = std::env::temp_dir().join(format!("kaltsit-propia-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let voice = crate::voice::setup(&m, "voz-kaltsit-en", 1.0);
        let Ok(voice) = voice else { return eprintln!("voz-kaltsit-en no descargada, se salta") };
        for line in ["Doctor. The reports are on your desk, and none of them are good.", "I will not repeat myself. Rest, or you will be useless tomorrow.", "Rhodes Island has survived worse than this. So will you."] {
            let (x, r) = crate::voice::synth_for_test(&voice, line).unwrap();
            learn_own(&s, &data, &x, r);
        }
        let (x, r) = crate::voice::synth_for_test(&voice, "Leave the samples there; I will look at them after the meeting.").unwrap();
        let heard_her = listen(&ear, Some(&s), &data, &resample(&x, r, RATE)).unwrap();
        let other = listen(&ear, Some(&s), &data, &secs(&wav16(BRIA).unwrap(), 1.0, 7.0)).unwrap();
        eprintln!("ella: {heard_her:?}\notra persona: {:?}", other.iter().map(|t| (&t.nombre, t.seguridad, &t.texto)).collect::<Vec<_>>());
        assert!(heard_her.is_empty(), "her own voice is dropped");
        assert_eq!(other.len(), 1);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn prosody_measures_pitch_and_loudness() {
        let tone = |hz: f32, amp: f32| -> Vec<f32> { (0..RATE as usize).map(|i| amp * (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin()).collect() };
        let a = prosody(&tone(200.0, 0.1));
        assert!((a.f0 - 200.0).abs() < 5.0, "f0 {}", a.f0);
        assert!(a.f0_var < 0.3, "a steady tone barely moves: {}", a.f0_var);
        let b = prosody(&tone(200.0, 0.4));
        assert!((b.db - a.db - 12.0).abs() < 0.5, "four times the amplitude is +12 dB: {} vs {}", b.db, a.db);
        // A tone sliding up an octave spreads its pitch.
        let mut ph = 0.0f32;
        let slide: Vec<f32> = (0..RATE as usize)
            .map(|i| {
                ph += 150.0 * 2f32.powf(i as f32 / RATE as f32) * std::f32::consts::TAU / RATE as f32;
                0.2 * ph.sin()
            })
            .collect();
        assert!(prosody(&slide).f0_var > 2.0);
        assert_eq!(prosody(&vec![0.0; RATE as usize]), Prosody::default(), "silence is not voice");
    }

    /// Real voices: a woman's pitch where it should be, and the measures stable between two
    /// parts of the same recording.
    #[test]
    #[cfg(windows)]
    fn prosody_of_real_voices() {
        let Some(bria) = wav16(BRIA) else { return eprintln!("falta bria.wav, se salta") };
        let (a, b) = (prosody(&secs(&bria, 1.0, 7.0)), prosody(&secs(&bria, 25.0, 31.0)));
        eprintln!("bria A {a:?}\nbria B {b:?}");
        assert!((150.0..300.0).contains(&a.f0) && (150.0..300.0).contains(&b.f0));
        assert!((a.db - b.db).abs() < 4.0 && (12.0 * (a.f0 / b.f0).log2()).abs() < 2.0, "the same voice reads alike");
    }

    /// A real voice (path in KALTSIT_WAV, 16-bit WAV): its level, what the VAD keeps, and what
    /// every downloaded Whisper writes, with how long it takes. Run by hand.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn a_real_voice_through_every_whisper() {
        let Ok(path) = std::env::var("KALTSIT_WAV") else { return };
        let Some((m, s)) = ready() else { return };
        let (x, r) = crate::voice::read_wav(Path::new(&path)).unwrap();
        let x = resample(&x, r, RATE);
        let rms = 10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).log10();
        let speech: f32 = with_fine(&s, |f| f.speech(&x)).unwrap().iter().map(|(_, v)| v.len() as f32).sum::<f32>() / RATE as f32;
        eprintln!("{:.1} s · media {rms:.1} dB · voz (VAD) {speech:.1} s · {:?}", x.len() as f32 / RATE as f32, prosody(&x));
        let base = crate::asr::setup(&m, "es").unwrap();
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(m.root().join("asr")).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.join("tokens.txt").exists() || p.is_dir() && std::fs::read_dir(p).is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().ends_with("tokens.txt")))).collect();
        dirs.sort();
        for dir in dirs {
            let ear = crate::asr::Setup { dir: dir.clone(), ..base.clone() };
            let t0 = std::time::Instant::now();
            let first = crate::asr::transcribe(&ear, &x, RATE).unwrap();
            let load = t0.elapsed();
            let t1 = std::time::Instant::now();
            let _ = crate::asr::transcribe(&ear, &x, RATE).unwrap();
            eprintln!("{:32} carga+1ª {load:?} · después {:?} · «{first}»", dir.file_name().unwrap().to_string_lossy(), t1.elapsed());
        }
    }

    /// A real recording (KALTSIT_WAV) and other people's voices against the voiceprints in
    /// data/voces.json: how far a real take of a stored voice is from that voice, against how
    /// far everyone else is. Run by hand.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn a_real_take_against_the_stored_voices() {
        let Ok(path) = std::env::var("KALTSIT_WAV") else { return };
        let Some((_, s)) = ready() else { return };
        let store = Voices::load(Path::new("data"));
        let (x, r) = crate::voice::read_wav(Path::new(&path)).unwrap();
        let take = resample(&x, r, RATE);
        let speech: Vec<f32> = with_fine(&s, |f| f.speech(&take)).unwrap().into_iter().flat_map(|(_, v)| v).collect();
        let mut probes: Vec<(String, Vec<f32>)> = vec![("grabación entera".into(), take.clone()), ("solo la voz (VAD)".into(), speech)];
        if let (Some(bria), Some(hibiki), Some(game)) = (wav16(BRIA), wav16(HIBIKI), wav16(GAME)) {
            for (name, w) in [("bria", &bria), ("hibiki", &hibiki), ("juego", &game)] {
                for k in 0..3 {
                    let a = 2.0 + k as f32 * 2.5;
                    probes.push((format!("{name} {a:.1}-{:.1} s", a + 2.0), secs(w, a, a + 2.0)));
                }
            }
        }
        for (name, x) in probes {
            let Some(p) = with_fine(&s, |f| f.embed(&x, RATE)).unwrap() else {
                eprintln!("{name:22} sin huella");
                continue;
            };
            let row: Vec<String> = store.voces.iter().map(|v| format!("{} {:.2}", v.nombre, similarity(&v.huella, &p))).collect();
            eprintln!("{name:22} ({:.1} s) {}", x.len() as f32 / RATE as f32, row.join(" · "));
        }
    }

    #[test]
    fn clips_are_kept_few_and_only_inside_their_folder() {
        let data = std::env::temp_dir().join(format!("kaltsit-clips-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let x = vec![0.1f32; RATE as usize];
        let mut names = Vec::new();
        for _ in 0..CLIPS_PER_VOICE + 3 {
            names.push(keep_clip(&data, 7, &x).unwrap());
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        let left = std::fs::read_dir(data.join("voces").join("7")).unwrap().count();
        assert_eq!(left, CLIPS_PER_VOICE, "only the newest few");
        assert!(clip_path(&data, names.last().unwrap()).is_some());
        assert!(clip_path(&data, &names[0]).is_none(), "the oldest went");
        for bad in ["../voces.json", "7/../../x.wav", "7/abc.wav", "7/123.exe", "/etc/passwd", "7\\\\123.wav"] {
            assert!(clip_path(&data, bad).is_none(), "{bad}");
        }
        drop_clips(&data, 7);
        assert!(!data.join("voces").join("7").exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    /// A real voice she could not tell, kept as a clip and named by whoever heard it: registered
    /// from that clip, another take of the same person is then recognised, and someone else is not.
    #[test]
    #[cfg(windows)]
    fn a_voice_registered_from_its_clip_is_recognised_afterwards() {
        let Some((_, s)) = ready() else { return };
        let (Some(bria), Some(hibiki)) = (wav16(BRIA), wav16(HIBIKI)) else { return eprintln!("faltan las grabaciones, se salta") };
        let data = std::env::temp_dir().join(format!("kaltsit-enroll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let clip = keep_clip(&data, 0, &secs(&bria, 1.0, 4.0)).unwrap();
        let id = enroll_clip(&s, &data, &clip, "Bria").unwrap();
        assert!(clip_path(&data, &clip).is_none(), "a registered clip is deleted");
        let print = |x: &[f32]| with_fine(&s, |f| f.embed(x, RATE)).unwrap().unwrap();
        let mut vs = Voices::load(&data);
        let later = vs.identify(&print(&secs(&bria, 25.0, 31.0)));
        let other = vs.identify(&print(&secs(&hibiki, 2.0, 8.0)));
        eprintln!("bria más tarde: voz {} · {:.2} · hibiki: voz {} · {:.2}", later.id, later.score, other.id, other.score);
        assert_eq!(later.id, id);
        assert_ne!(other.id, id);
        // A second clip with the same name adds to that voice instead of making another.
        let clip2 = keep_clip(&data, 0, &secs(&bria, 10.0, 13.0)).unwrap();
        assert_eq!(enroll_clip(&s, &data, &clip2, "bria").unwrap(), id);
        assert_eq!(Voices::load(&data).get(id).unwrap().n, 2);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn turns_of_the_same_speaker_merge_and_slices_stay_inside() {
        assert_eq!(merge_turns(vec![(0.0, 1.0, 0), (1.2, 2.0, 0), (2.5, 3.0, 1), (3.1, 4.0, 0)]), vec![(0.0, 2.0, 0), (2.5, 3.0, 1), (3.1, 4.0, 0)]);
        let x = vec![0.0; RATE as usize];
        assert_eq!(slice(&x, 0.5, 9.0).len(), RATE as usize / 2);
        assert!(slice(&x, 2.0, 3.0).is_empty());
    }
}
