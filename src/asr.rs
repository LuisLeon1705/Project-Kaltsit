//! Her ear: dictation. The microphone is read with winmm's `waveIn` (16 kHz, mono) while the
//! button of the chat is held, and what was said is transcribed by Whisper through sherpa-onnx's
//! C library, the same `sherpa-onnx-c-api.dll` that already gives her the voice. Nothing leaves
//! the computer, and the microphone is only open between pressing and releasing the button.

use crate::catalog::Manifest;
use std::ffi::{c_char, c_void, CStr, CString};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

/// Whisper wants 16 kHz; the microphone is opened at that rate.
pub const RATE: u32 = 16000;
/// A hold longer than this is cut: Whisper's window is 30 s.
const MAX_SECONDS: usize = 30;

/// What to load: the library, the model's folder and the language it listens in.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub dll: PathBuf,
    pub dir: PathBuf,
    /// "es" | "en"
    pub lang: String,
}

/// The first downloaded ear of the catalog (the list puts the better one first), with the
/// downloaded engine, listening in `lang`. Errors say what is missing.
pub fn setup(m: &Manifest, lang: &str) -> Result<Setup, String> {
    let root = m.root();
    let ear = m
        .elementos
        .iter()
        .find(|i| i.oido.as_ref().is_some_and(|o| o.motor == "whisper") && i.installed(&root))
        .ok_or_else(|| crate::lang::tr("no hay ningún modelo de oído descargado (Modelos → Oído)", "there is no ear model downloaded (Models → Ear)"))?;
    let engine = m
        .elementos
        .iter()
        .filter(|i| i.servidor.as_deref().is_some_and(|s| s.ends_with(".dll")))
        .find(|i| i.installed(&root))
        .ok_or_else(|| crate::lang::tr("falta el motor de voz (Modelos → sherpa-onnx)", "the voice engine is missing (Models → sherpa-onnx)"))?;
    let spec = ear.oido.as_ref().expect("filtered above");
    Ok(Setup { dll: root.join(engine.servidor.as_deref().unwrap_or_default()), dir: root.join(&spec.carpeta), lang: if lang == "en" { "en".into() } else { "es".into() } })
}

// ── the C API of sherpa-onnx 1.13.8 (c-api.h), only what offline recognition needs ──

type Str = *const c_char;

#[repr(C)]
struct One {
    model: Str,
}

#[repr(C)]
struct Transducer {
    encoder: Str,
    decoder: Str,
    joiner: Str,
}

#[repr(C)]
struct Whisper {
    encoder: Str,
    decoder: Str,
    language: Str,
    task: Str,
    tail_paddings: i32,
    enable_token_timestamps: i32,
    enable_segment_timestamps: i32,
}

#[repr(C)]
struct SenseVoice {
    model: Str,
    language: Str,
    use_itn: i32,
}

#[repr(C)]
struct Moonshine {
    preprocessor: Str,
    encoder: Str,
    uncached_decoder: Str,
    cached_decoder: Str,
    merged_decoder: Str,
}

#[repr(C)]
struct FireRed {
    encoder: Str,
    decoder: Str,
}

#[repr(C)]
struct Canary {
    encoder: Str,
    decoder: Str,
    src_lang: Str,
    tgt_lang: Str,
    use_pnc: i32,
}

#[repr(C)]
struct FunAsrNano {
    encoder_adaptor: Str,
    llm: Str,
    embedding: Str,
    tokenizer: Str,
    system_prompt: Str,
    user_prompt: Str,
    max_new_tokens: i32,
    temperature: f32,
    top_p: f32,
    seed: i32,
    language: Str,
    itn: i32,
    hotwords: Str,
}

#[repr(C)]
struct Qwen3 {
    conv_frontend: Str,
    encoder: Str,
    decoder: Str,
    tokenizer: Str,
    max_total_len: i32,
    max_new_tokens: i32,
    temperature: f32,
    top_p: f32,
    seed: i32,
    hotwords: Str,
}

#[repr(C)]
struct Cohere {
    encoder: Str,
    decoder: Str,
    language: Str,
    use_punct: i32,
    use_itn: i32,
}

#[repr(C)]
struct ModelCfg {
    transducer: Transducer,
    paraformer: One,
    nemo_ctc: One,
    whisper: Whisper,
    tdnn: One,
    tokens: Str,
    num_threads: i32,
    debug: i32,
    provider: Str,
    model_type: Str,
    modeling_unit: Str,
    bpe_vocab: Str,
    telespeech_ctc: Str,
    sense_voice: SenseVoice,
    moonshine: Moonshine,
    fire_red_asr: FireRed,
    dolphin: One,
    zipformer_ctc: One,
    canary: Canary,
    wenet_ctc: One,
    omnilingual: One,
    medasr: One,
    funasr_nano: FunAsrNano,
    fire_red_asr_ctc: One,
    qwen3_asr: Qwen3,
    cohere_transcribe: Cohere,
}

#[repr(C)]
struct Feat {
    sample_rate: i32,
    feature_dim: i32,
}

#[repr(C)]
struct Lm {
    model: Str,
    scale: f32,
}

#[repr(C)]
struct Homophone {
    dict_dir: Str,
    lexicon: Str,
    rule_fsts: Str,
}

#[repr(C)]
struct RecognizerCfg {
    feat: Feat,
    model: ModelCfg,
    lm: Lm,
    decoding_method: Str,
    max_active_paths: i32,
    hotwords_file: Str,
    hotwords_score: f32,
    rule_fsts: Str,
    rule_fars: Str,
    blank_penalty: f32,
    hr: Homophone,
}

/// SherpaOnnxOfflineRecognizerResult (here Answer): only `text` is read.
#[repr(C)]
struct Answer {
    text: Str,
    timestamps: *const f32,
    count: i32,
    tokens: Str,
    tokens_arr: *const Str,
    json: Str,
    lang: Str,
}

type CreateFn = unsafe extern "C" fn(*const RecognizerCfg) -> *const c_void;
type DestroyFn = unsafe extern "C" fn(*const c_void);
type StreamFn = unsafe extern "C" fn(*const c_void) -> *const c_void;
type AcceptFn = unsafe extern "C" fn(*const c_void, i32, *const f32, i32);
type DecodeFn = unsafe extern "C" fn(*const c_void, *const c_void);
type ResultFn = unsafe extern "C" fn(*const c_void) -> *const Answer;
type FreeResultFn = unsafe extern "C" fn(*const Answer);

/// A loaded recognizer. Lives behind `EAR`.
struct Ear {
    recognizer: *const c_void,
    destroy: DestroyFn,
    create_stream: StreamFn,
    destroy_stream: DestroyFn,
    accept: AcceptFn,
    decode: DecodeFn,
    result: ResultFn,
    free_result: FreeResultFn,
    setup: Setup,
}

// The raw pointer is only used while holding the `EAR` lock.
unsafe impl Send for Ear {}

impl Ear {
    #[cfg(windows)]
    fn load(s: &Setup) -> std::result::Result<Ear, String> {
        use crate::voice::{cstr, find, load_library, symbol};
        // The library stays loaded for the life of the app, as in the voice.
        static LIB: OnceLock<std::result::Result<isize, String>> = OnceLock::new();
        let lib = LIB.get_or_init(|| load_library(&s.dll)).clone()?;
        let (create, destroy, create_stream, destroy_stream, accept, decode, result, free_result) = unsafe {
            (
                std::mem::transmute::<*const c_void, CreateFn>(symbol(lib, "SherpaOnnxCreateOfflineRecognizer")?),
                std::mem::transmute::<*const c_void, DestroyFn>(symbol(lib, "SherpaOnnxDestroyOfflineRecognizer")?),
                std::mem::transmute::<*const c_void, StreamFn>(symbol(lib, "SherpaOnnxCreateOfflineStream")?),
                std::mem::transmute::<*const c_void, DestroyFn>(symbol(lib, "SherpaOnnxDestroyOfflineStream")?),
                std::mem::transmute::<*const c_void, AcceptFn>(symbol(lib, "SherpaOnnxAcceptWaveformOffline")?),
                std::mem::transmute::<*const c_void, DecodeFn>(symbol(lib, "SherpaOnnxDecodeOfflineStream")?),
                std::mem::transmute::<*const c_void, ResultFn>(symbol(lib, "SherpaOnnxGetOfflineStreamResult")?),
                std::mem::transmute::<*const c_void, FreeResultFn>(symbol(lib, "SherpaOnnxDestroyOfflineRecognizerResult")?),
            )
        };
        // int8 files first: same text, half the size, faster on CPU.
        let file = |suffixes: &[&str]| -> std::result::Result<CString, String> {
            suffixes
                .iter()
                .find_map(|x| find(&s.dir, x))
                .map(|p| cstr(&p))
                .ok_or_else(|| crate::lang::tr(format!("falta {} en {}", suffixes[0], s.dir.display()), format!("{} is missing in {}", suffixes[0], s.dir.display())))
        };
        let encoder = file(&["encoder.int8.onnx", "encoder.onnx"])?;
        let decoder = file(&["decoder.int8.onnx", "decoder.onnx"])?;
        let tokens = file(&["tokens.txt"])?;
        let language = CString::new(s.lang.as_str()).unwrap_or_default();
        let task = CString::new("transcribe").unwrap();
        let provider = CString::new("cpu").unwrap();
        let method = CString::new("greedy_search").unwrap();
        // Zero is "not set": the library fills in its defaults (16 kHz, 80 features).
        let mut cfg: RecognizerCfg = unsafe { std::mem::zeroed() };
        cfg.model.whisper.encoder = encoder.as_ptr();
        cfg.model.whisper.decoder = decoder.as_ptr();
        cfg.model.whisper.language = language.as_ptr();
        cfg.model.whisper.task = task.as_ptr();
        cfg.model.tokens = tokens.as_ptr();
        cfg.model.num_threads = 4;
        cfg.model.provider = provider.as_ptr();
        cfg.decoding_method = method.as_ptr();
        // The library copies every string while creating the recognizer.
        let recognizer = unsafe { create(&cfg) };
        if recognizer.is_null() {
            return Err(crate::lang::tr(format!("sherpa-onnx no pudo cargar el oído de {}", s.dir.display()), format!("sherpa-onnx could not load the ear from {}", s.dir.display())));
        }
        Ok(Ear { recognizer, destroy, create_stream, destroy_stream, accept, decode, result, free_result, setup: s.clone() })
    }

    #[cfg(not(windows))]
    fn load(_s: &Setup) -> std::result::Result<Ear, String> {
        Err(crate::lang::tr("el oído solo funciona en Windows", "the ear only works on Windows"))
    }

    fn transcribe(&self, samples: &[f32], rate: u32) -> std::result::Result<String, String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        unsafe {
            let stream = (self.create_stream)(self.recognizer);
            if stream.is_null() {
                return Err(crate::lang::tr("el oído no pudo abrir un flujo", "the ear could not open a stream"));
            }
            (self.accept)(stream, rate as i32, samples.as_ptr(), samples.len() as i32);
            (self.decode)(self.recognizer, stream);
            let r = (self.result)(stream);
            let text = if r.is_null() || (*r).text.is_null() { String::new() } else { CStr::from_ptr((*r).text).to_string_lossy().trim().to_string() };
            if !r.is_null() {
                (self.free_result)(r);
            }
            (self.destroy_stream)(stream);
            Ok(text)
        }
    }
}

impl Drop for Ear {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.recognizer) };
    }
}

static EAR: Mutex<Option<Ear>> = Mutex::new(None);

/// Transcribes `samples` (mono, in [-1, 1]). The recognizer is loaded the first time and kept
/// while the model and the language stay the same.
pub fn transcribe(s: &Setup, samples: &[f32], rate: u32) -> std::result::Result<String, String> {
    let mut ear = EAR.lock().unwrap_or_else(|e| e.into_inner());
    if ear.as_ref().is_none_or(|e| e.setup != *s) {
        *ear = None;
        *ear = Some(Ear::load(s)?);
    }
    let text = ear.as_ref().expect("loaded above").transcribe(samples, rate)?;
    // Whisper sometimes hears only punctuation (".", "…") in a breath: that is nothing said.
    Ok(if text.chars().any(char::is_alphanumeric) { text } else { String::new() })
}

/// Transcribes a 16-bit PCM WAV file.
#[cfg(test)]
fn transcribe_file(s: &Setup, path: &Path) -> std::result::Result<String, String> {
    let (samples, rate) = crate::voice::read_wav(path)?;
    transcribe(s, &samples, rate)
}

// ── the microphone ──

/// The microphone, open until `finish`. Samples are collected on their own thread.
pub struct Recording {
    stop: Arc<AtomicBool>,
    join: JoinHandle<Vec<f32>>,
}

impl Recording {
    /// Closes the microphone and returns what it heard (16 kHz mono).
    pub fn finish(self) -> Vec<f32> {
        self.stop.store(true, Ordering::Relaxed);
        self.join.join().unwrap_or_default()
    }
}

static REC: Mutex<Option<Recording>> = Mutex::new(None);

/// The open microphone, read until it is closed (or dropped).
pub struct Stream {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Opens the microphone for good: each 100 ms heard (16 kHz mono) goes to `sink`.
pub fn open_stream(sink: Box<dyn FnMut(&[f32]) + Send>) -> std::result::Result<Stream, String> {
    mic::stream(sink)
}

/// Opens the microphone. Starting twice keeps the first recording.
pub fn start_recording() -> std::result::Result<(), String> {
    let mut rec = REC.lock().unwrap_or_else(|e| e.into_inner());
    if rec.is_none() {
        *rec = Some(mic::open()?);
    }
    Ok(())
}

/// Closes the microphone and returns what it heard; empty if it was not recording.
pub fn stop_recording() -> Vec<f32> {
    let rec = REC.lock().unwrap_or_else(|e| e.into_inner()).take();
    rec.map(Recording::finish).unwrap_or_default()
}

#[cfg(windows)]
mod mic {
    use super::{Recording, Stream, MAX_SECONDS, RATE};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc::channel, Arc};
    use std::time::Duration;

    const WAVE_MAPPER: u32 = u32::MAX;
    const WAVE_FORMAT_PCM: u16 = 1;
    const WHDR_DONE: u32 = 1;
    const BUFFERS: usize = 4;
    /// 100 ms of audio per buffer.
    const CHUNK: usize = (RATE / 10) as usize;

    #[repr(C)]
    struct WaveFormat {
        tag: u16,
        channels: u16,
        rate: u32,
        bytes_per_sec: u32,
        align: u16,
        bits: u16,
        size: u16,
    }

    #[repr(C)]
    struct WaveHdr {
        data: *mut u8,
        length: u32,
        recorded: u32,
        user: usize,
        flags: u32,
        loops: u32,
        next: *mut WaveHdr,
        reserved: usize,
    }

    #[link(name = "winmm")]
    extern "system" {
        fn waveInOpen(h: *mut isize, device: u32, fmt: *const WaveFormat, callback: usize, instance: usize, flags: u32) -> u32;
        fn waveInPrepareHeader(h: isize, hdr: *mut WaveHdr, size: u32) -> u32;
        fn waveInUnprepareHeader(h: isize, hdr: *mut WaveHdr, size: u32) -> u32;
        fn waveInAddBuffer(h: isize, hdr: *mut WaveHdr, size: u32) -> u32;
        fn waveInStart(h: isize) -> u32;
        fn waveInStop(h: isize) -> u32;
        fn waveInReset(h: isize) -> u32;
        fn waveInClose(h: isize) -> u32;
    }

    const HDR: u32 = std::mem::size_of::<WaveHdr>() as u32;

    /// Reads the default input device, handing each 100 ms of it (in [-1, 1]) to `sink`, until
    /// `stop` is set.
    fn record(stop: &AtomicBool, opened: &std::sync::mpsc::Sender<Result<(), String>>, sink: &mut dyn FnMut(&[f32])) {
        let fmt = WaveFormat { tag: WAVE_FORMAT_PCM, channels: 1, rate: RATE, bytes_per_sec: RATE * 2, align: 2, bits: 16, size: 0 };
        let mut h: isize = 0;
        let r = unsafe { waveInOpen(&mut h, WAVE_MAPPER, &fmt, 0, 0, 0) };
        if r != 0 {
            let _ = opened.send(Err(crate::lang::tr(format!("no se pudo abrir el micrófono (error {r}): ¿está conectado y con permiso en Configuración → Privacidad?"), format!("could not open the microphone (error {r}): is it connected and allowed in Settings → Privacy?"))));
            return;
        }
        let mut data: Vec<Vec<i16>> = (0..BUFFERS).map(|_| vec![0i16; CHUNK]).collect();
        let mut hdrs: Vec<WaveHdr> = data
            .iter_mut()
            .map(|d| WaveHdr { data: d.as_mut_ptr().cast(), length: (CHUNK * 2) as u32, recorded: 0, user: 0, flags: 0, loops: 0, next: std::ptr::null_mut(), reserved: 0 })
            .collect();
        unsafe {
            for hdr in hdrs.iter_mut() {
                waveInPrepareHeader(h, hdr, HDR);
                waveInAddBuffer(h, hdr, HDR);
            }
            let r = waveInStart(h);
            if r != 0 {
                let _ = opened.send(Err(crate::lang::tr(format!("el micrófono no arrancó (error {r})"), format!("the microphone did not start (error {r})"))));
                waveInReset(h);
                for hdr in hdrs.iter_mut() {
                    waveInUnprepareHeader(h, hdr, HDR);
                }
                waveInClose(h);
                return;
            }
            let _ = opened.send(Ok(()));
            let mut chunk: Vec<f32> = Vec::with_capacity(CHUNK);
            let mut drain = |h: isize, hdrs: &mut Vec<WaveHdr>, refill: bool| {
                for hdr in hdrs.iter_mut() {
                    if hdr.flags & WHDR_DONE != 0 {
                        let n = hdr.recorded as usize / 2;
                        let s = std::slice::from_raw_parts(hdr.data.cast::<i16>(), n);
                        chunk.clear();
                        chunk.extend(s.iter().map(|&x| x as f32 / 32768.0));
                        sink(&chunk);
                        hdr.flags &= !WHDR_DONE;
                        hdr.recorded = 0;
                        if refill {
                            waveInAddBuffer(h, hdr, HDR);
                        }
                    }
                }
            };
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(20));
                drain(h, &mut hdrs, true);
            }
            waveInStop(h);
            // Reset hands back every buffer, the half-filled one included.
            waveInReset(h);
            drain(h, &mut hdrs, false);
            for hdr in hdrs.iter_mut() {
                waveInUnprepareHeader(h, hdr, HDR);
            }
            waveInClose(h);
        }
    }

    /// Push-to-talk: everything heard until `finish` (cut at MAX_SECONDS).
    pub fn open() -> Result<Recording, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let flag = stop.clone();
        let join = std::thread::spawn(move || {
            let mut samples: Vec<f32> = Vec::new();
            record(&flag, &tx, &mut |c| {
                if samples.len() < MAX_SECONDS * RATE as usize {
                    samples.extend_from_slice(c);
                }
            });
            samples
        });
        started(rx, &stop)?;
        Ok(Recording { stop, join })
    }

    /// Open microphone: every 100 ms heard goes to `sink` until the stream is closed.
    pub fn stream(mut sink: Box<dyn FnMut(&[f32]) + Send>) -> Result<Stream, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let flag = stop.clone();
        let join = std::thread::spawn(move || record(&flag, &tx, &mut *sink));
        started(rx, &stop)?;
        Ok(Stream { stop, join: Some(join) })
    }

    /// Waits for the recording thread to say whether the device opened.
    fn started(rx: std::sync::mpsc::Receiver<Result<(), String>>, stop: &AtomicBool) -> Result<(), String> {
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err(crate::lang::tr("el micrófono no respondió", "the microphone did not answer"))
            }
        }
    }
}

#[cfg(not(windows))]
mod mic {
    use super::{Recording, Stream};

    pub fn open() -> Result<Recording, String> {
        Err(crate::lang::tr("el micrófono solo funciona en Windows", "the microphone only works on Windows"))
    }

    pub fn stream(_sink: Box<dyn FnMut(&[f32]) + Send>) -> Result<Stream, String> {
        Err(crate::lang::tr("el micrófono solo funciona en Windows", "the microphone only works on Windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_matches_sherpa_onnx_1_13_8() {
        // Sizes and offsets of the C structs on x64, worked out from c-api.h of that version.
        assert_eq!(std::mem::size_of::<Whisper>(), 48);
        assert_eq!(std::mem::offset_of!(Whisper, tail_paddings), 32);
        assert_eq!(std::mem::size_of::<SenseVoice>(), 24);
        assert_eq!(std::mem::size_of::<Canary>(), 40);
        assert_eq!(std::mem::size_of::<Cohere>(), 32);
        assert_eq!(std::mem::size_of::<FunAsrNano>(), 88);
        assert_eq!(std::mem::size_of::<Qwen3>(), 64);
        // ModelCfg: transducer 24, paraformer 8, nemo 8, whisper 48, tdnn 8 → tokens at 96;
        // the last field (cohere_transcribe, 32 bytes) ends at 504.
        assert_eq!(std::mem::offset_of!(ModelCfg, whisper), 40);
        assert_eq!(std::mem::offset_of!(ModelCfg, tokens), 96);
        assert_eq!(std::mem::offset_of!(ModelCfg, num_threads), 104);
        assert_eq!(std::mem::offset_of!(ModelCfg, provider), 112);
        assert_eq!(std::mem::offset_of!(ModelCfg, sense_voice), 152);
        assert_eq!(std::mem::offset_of!(ModelCfg, moonshine), 176);
        assert_eq!(std::mem::offset_of!(ModelCfg, canary), 248);
        assert_eq!(std::mem::offset_of!(ModelCfg, funasr_nano), 312);
        assert_eq!(std::mem::offset_of!(ModelCfg, qwen3_asr), 408);
        assert_eq!(std::mem::offset_of!(ModelCfg, cohere_transcribe), 472);
        assert_eq!(std::mem::size_of::<ModelCfg>(), 504);
        assert_eq!(std::mem::offset_of!(RecognizerCfg, model), 8);
        assert_eq!(std::mem::offset_of!(RecognizerCfg, lm), 8 + 504);
        assert_eq!(std::mem::offset_of!(RecognizerCfg, decoding_method), 8 + 504 + 16);
        assert_eq!(std::mem::offset_of!(RecognizerCfg, hotwords_file), 544);
        assert_eq!(std::mem::offset_of!(RecognizerCfg, blank_penalty), 576);
        assert_eq!(std::mem::offset_of!(RecognizerCfg, hr), 584);
        assert_eq!(std::mem::size_of::<RecognizerCfg>(), 608);
        assert_eq!(std::mem::size_of::<Answer>(), 56);
    }

    /// Builds a WAV with her own voice, the one she speaks with, and returns the line said.
    #[cfg(windows)]
    fn spoken_wav(m: &Manifest, id: &str, line: &str, path: &Path) -> Option<()> {
        let s = crate::voice::setup(m, id, 1.0).ok()?;
        let a = crate::voice::synth_for_test(&s, line).ok()?;
        std::fs::write(path, crate::download::wav(&a.0, a.1)).ok()
    }

    /// With the ear and a voice downloaded (Modelos), what her voice says is transcribed back.
    #[test]
    #[cfg(windows)]
    fn real_ear_hears_her_own_voice_when_downloaded() {
        let m = Manifest::load();
        let lines = [("es", "voz-daniela", "Mañana por la mañana tengo que revisar el informe de ventas y llamar a mi hermana."), ("en", "voz-kaltsit-en", "Tomorrow morning I have to review the sales report and call my sister.")];
        for (lang, voice, line) in lines {
            let Ok(s) = setup(&m, lang) else {
                eprintln!("oído no descargado, se salta");
                return;
            };
            let wav = std::env::temp_dir().join(format!("kaltsit-ear-{lang}-{}.wav", std::process::id()));
            if spoken_wav(&m, voice, line, &wav).is_none() {
                eprintln!("{voice}: no descargada, se salta");
                continue;
            }
            let t0 = std::time::Instant::now();
            let heard = transcribe_file(&s, &wav).unwrap();
            let first = t0.elapsed();
            let (samples, rate) = crate::voice::read_wav(&wav).unwrap();
            let t1 = std::time::Instant::now();
            let _ = transcribe(&s, &samples, rate).unwrap();
            eprintln!("[{lang}] audio {:.1} s · primera vez (carga incluida) {first:?} · después {:?}\n  dicho:     {line}\n  transcrito: {heard}", samples.len() as f32 / rate as f32, t1.elapsed());
            let _ = std::fs::remove_file(&wav);
            let norm = |t: &str| t.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().map(String::from).collect::<Vec<_>>();
            let (said, got) = (norm(line), norm(&heard));
            let hits = said.iter().filter(|w| got.contains(w)).count();
            assert!(hits * 10 >= said.len() * 6, "[{lang}] {hits}/{} words: «{heard}»", said.len());
        }
    }

    /// Needs a microphone: with `KALTSIT_MIC_TEST=1` it records one second and only checks that
    /// the device opens and closes cleanly.
    #[test]
    fn the_microphone_opens_and_closes() {
        if std::env::var("KALTSIT_MIC_TEST").is_err() {
            return;
        }
        start_recording().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let a = stop_recording();
        eprintln!("micrófono: {} muestras en 1 s", a.len());
        assert!(a.len() > 12000 && a.len() < 20000);
    }
}
