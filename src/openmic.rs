//! The open microphone ("Auto"): while it is on she listens all the time, and each thing said
//! is sent by itself once the speaker pauses, as if it had been dictated and sent.
//!
//! A long monologue is not kept whole until the end: her conversation window is small (60–360
//! tokens, by her hormones), so once it reaches a limit drawn from that window, what was said so
//! far goes to her as a piece of a monologue still going on. She then decides — after a quick
//! read by the model of how charged it is, with the voice's measures as data — whether to let
//! the person go on, say something brief, or interrupt (`Core::hear_monologue`).
//!
//! If someone speaks over her while she talks, she stops (never for her own voice coming back
//! from the speakers, and not before she knows what her voice sounds like).

use crate::hearing::{self, Live, Prosody, Turn};
use std::collections::VecDeque;
use std::path::PathBuf;

const RATE: f32 = hearing::RATE as f32;
/// Silence after the last stretch (which already ended with a 0.6 s pause) that ends the turn.
const END_S: f32 = 0.9;
/// Speech over her voice this long is checked: if it is not her own, she stops.
const BARGE_S: f32 = 0.8;

/// Words of a monologue piece: at most 60 % of her conversation window (~4 characters a token,
/// ~6 a word with its space), and between 40 and 150.
pub fn monologue_limit(window_tokens: usize) -> usize {
    ((window_tokens * 4) as f32 * 0.6 / 6.0).round().clamp(40.0, 150.0) as usize
}

fn words(t: &str) -> usize {
    t.split_whitespace().count()
}

/// What goes to her: the turns heard, how it sounded, and whether the speaker is still going.
#[derive(Clone, Debug)]
pub struct Flush {
    pub text: String,
    pub turns: Vec<Turn>,
    pub sounds: Prosody,
    /// How the (first named) voice usually sounds.
    pub habitual: Option<Prosody>,
    pub seconds: f32,
    pub words: usize,
    pub still_talking: bool,
}

impl Flush {
    /// The measures as data for her (see `lang::monologue_sound`).
    pub fn sound_json(&self) -> serde_json::Value {
        serde_json::json!({ "db": self.sounds.db, "f0": self.sounds.f0, "f0_var": self.sounds.f0_var, "habitual": self.habitual, "palabras": self.words, "segundos": self.seconds })
    }
}

/// What a step of listening asks for.
#[derive(Debug)]
pub enum Step {
    Nothing,
    Send(Flush),
    /// Someone else speaks over her: stop talking.
    BargeIn,
}

/// The listening itself, fed 100 ms at a time; no threads, so it can be tested on real audio.
pub struct Listener {
    live: Live,
    fine: hearing::Setup,
    ear: crate::asr::Setup,
    data: PathBuf,
    pub limit: usize,
    turns: Vec<Turn>,
    audio: Vec<f32>,
    habitual: Option<Prosody>,
    words: usize,
    quiet: f32,
    /// The last 1.6 s heard, for telling whose voice is speaking over hers.
    recent: VecDeque<f32>,
    over_her: f32,
    checked: bool,
    /// The last voice told apart, for a stretch too short to tell (even after a piece went out).
    last: Option<Turn>,
}

impl Listener {
    pub fn new(fine: hearing::Setup, ear: crate::asr::Setup, data: PathBuf, limit: usize) -> Result<Listener, String> {
        Ok(Listener { live: Live::new(&fine)?, fine, ear, data, limit, turns: Vec::new(), audio: Vec::new(), habitual: None, words: 0, quiet: 0.0, recent: VecDeque::new(), over_her: 0.0, checked: false, last: None })
    }

    /// Someone is speaking right now.
    pub fn hearing(&self) -> bool {
        self.live.speaking()
    }

    /// Feeds what the microphone just heard. `she_speaks`: her voice is playing.
    pub fn feed(&mut self, x: &[f32], she_speaks: bool) -> Result<Step, String> {
        let dt = x.len() as f32 / RATE;
        self.recent.extend(x.iter().copied());
        while self.recent.len() > (1.6 * RATE) as usize {
            self.recent.pop_front();
        }
        let closed = self.live.accept(x);
        let speaking = self.live.speaking();
        // Over her voice: once, after BARGE_S of speech, check it is not her own.
        let mut barge = false;
        if she_speaks && speaking {
            self.over_her += dt;
            if self.over_her >= BARGE_S && !self.checked {
                // Until there is enough voice to tell, it asks again with the next 100 ms.
                let recent: Vec<f32> = self.recent.iter().copied().collect();
                if let Some(own) = hearing::is_own(&self.fine, &self.data, &recent) {
                    self.checked = true;
                    barge = !own;
                }
            }
        } else if !speaking {
            self.over_her = 0.0;
            self.checked = false;
        }
        for s in closed {
            self.stretch(&s)?;
        }
        if barge {
            return Ok(Step::BargeIn);
        }
        if self.words >= self.limit {
            return Ok(Step::Send(self.flush(true)));
        }
        self.quiet = if speaking { 0.0 } else { self.quiet + dt };
        if !self.turns.is_empty() && !speaking && self.quiet >= END_S {
            return Ok(Step::Send(self.flush(false)));
        }
        Ok(Step::Nothing)
    }

    /// A stretch of speech closed by a pause: whose, transcribed, added to the monologue.
    fn stretch(&mut self, s: &[f32]) -> Result<(), String> {
        let sounds = hearing::prosody(s);
        let who = hearing::recognise(&self.fine, &self.data, s, &sounds)?;
        if who.as_ref().is_some_and(|w| w.propia) {
            return Ok(());
        }
        let text = crate::asr::transcribe(&self.ear, s, hearing::RATE)?;
        if text.is_empty() {
            return Ok(());
        }
        if self.habitual.is_none() {
            self.habitual = who.as_ref().filter(|w| w.conocida).and_then(|w| w.habitual);
        }
        self.words += words(&text);
        self.audio.extend_from_slice(s);
        let at = self.audio.len() as f32 / RATE;
        let turn = match who {
            Some(w) => Turn { voz: w.voz, nombre: w.nombre, conocida: w.conocida, seguridad: w.seguridad, texto: text, inicio: at, clip: None },
            None => Turn { voz: 0, nombre: String::new(), conocida: false, seguridad: 0.0, texto: text, inicio: at, clip: None },
        };
        // A stretch too short to tell whose takes the voice of the one before it.
        let turn = match (turn.voz, &self.last) {
            (0, Some(prev)) => Turn { voz: prev.voz, nombre: prev.nombre.clone(), conocida: prev.conocida, seguridad: prev.seguridad, ..turn },
            _ => turn,
        };
        if turn.voz != 0 {
            self.last = Some(turn.clone());
        }
        // A voice she does not know keeps a clip, to hear whose it is.
        let turn = if turn.conocida { turn } else { Turn { clip: hearing::keep_clip(&self.data, turn.voz, s), ..turn } };
        match self.turns.last_mut() {
            Some(prev) if prev.voz == turn.voz => {
                prev.texto.push(' ');
                prev.texto.push_str(&turn.texto);
            }
            _ => self.turns.push(turn),
        }
        Ok(())
    }

    fn flush(&mut self, still_talking: bool) -> Flush {
        let turns = std::mem::take(&mut self.turns);
        let audio = std::mem::take(&mut self.audio);
        let f = Flush {
            text: turns.iter().map(|t| t.texto.as_str()).collect::<Vec<_>>().join(" "),
            sounds: hearing::prosody(&audio),
            habitual: self.habitual.take(),
            seconds: audio.len() as f32 / RATE,
            words: self.words,
            turns,
            still_talking,
        };
        self.words = 0;
        self.quiet = 0.0;
        f
    }
}

// ── the thread ──

struct Running {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _mic: crate::asr::Stream,
}

static RUNNING: std::sync::Mutex<Option<Running>> = std::sync::Mutex::new(None);

pub fn is_on() -> bool {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Turns the open microphone on: needs dictation and the fine ear downloaded.
pub fn start(core: std::sync::Arc<crate::life::Core>) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    if running.is_some() {
        return Ok(());
    }
    let m = crate::catalog::Manifest::load();
    let lang = if crate::lang::ui() == crate::lang::Lang::En { "en" } else { "es" };
    let ear = crate::asr::setup(&m, lang)?;
    let fine = hearing::setup(&m).ok_or_else(|| crate::lang::tr("el micrófono abierto necesita el oído fino (Modelos → Oído)", "the open microphone needs the fine ear (Models → Ear)"))?;
    let limit = monologue_limit(core.mind().hormones.window_tokens());
    let mut listener = Listener::new(fine, ear, core.data.clone(), limit)?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
    let mic = crate::asr::open_stream(Box::new(move |c| {
        let _ = tx.send(c.to_vec());
    }))?;
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let mut was_hearing = false;
        while let Ok(chunk) = rx.recv() {
            if flag.load(Ordering::Relaxed) {
                break;
            }
            listener.limit = monologue_limit(core.mind().hormones.window_tokens());
            match listener.feed(&chunk, crate::voice::is_playing()) {
                Ok(Step::Send(f)) => {
                    if let Err(e) = core.hear_open(&f) {
                        core.emit("k:error", serde_json::json!({ "text": e }));
                    }
                }
                Ok(Step::BargeIn) => crate::voice::stop(),
                Ok(Step::Nothing) => {}
                Err(e) => core.emit("k:error", serde_json::json!({ "text": e })),
            }
            let hearing = listener.hearing();
            if hearing != was_hearing {
                was_hearing = hearing;
                core.emit("k:openmic", serde_json::json!({ "on": true, "oyendo": hearing }));
            }
        }
    });
    *running = Some(Running { stop, _mic: mic });
    Ok(())
}

/// Turns it off (the microphone closes).
pub fn stop() {
    if let Some(r) = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).take() {
        r.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
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
        let (s, r) = crate::voice::read_wav(std::path::Path::new(path)).ok()?;
        Some(resample(&s, r, hearing::RATE))
    }

    fn ready(lang: &str) -> Option<(crate::catalog::Manifest, hearing::Setup, crate::asr::Setup)> {
        let m = crate::catalog::Manifest::load();
        let (Some(fine), Ok(ear)) = (hearing::setup(&m), crate::asr::setup(&m, lang)) else {
            eprintln!("oído fino o dictado no descargados, se salta");
            return None;
        };
        Some((m, fine, ear))
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kaltsit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    const BRIA: &str = "modelos/voz/sherpa-onnx-pocket-tts-int8-2026-01-26/test_wavs/bria.wav";

    /// 30 s of a real monologue then 3 s of silence, fed 100 ms at a time like the microphone:
    /// with a 40-word limit it goes out in pieces still going on, none far over the limit, and
    /// the rest goes out as the end of the turn soon after the speaker stops.
    #[test]
    #[cfg(windows)]
    fn a_monologue_goes_out_in_pieces_that_fit_the_window() {
        let Some((_, fine, ear)) = ready("en") else { return };
        let Some(bria) = wav16(BRIA) else { return eprintln!("falta bria.wav, se salta") };
        let data = temp("openmic");
        let mut l = Listener::new(fine, ear, data.clone(), 40).unwrap();
        let take: Vec<f32> = [&bria[..(30.0 * RATE) as usize], &vec![0.0; (3.0 * RATE) as usize][..]].concat();
        let mut out = Vec::new();
        let t0 = std::time::Instant::now();
        for (k, c) in take.chunks((RATE / 10.0) as usize).enumerate() {
            if let Step::Send(f) = l.feed(c, false).unwrap() {
                let at = (k + 1) as f32 / 10.0;
                eprintln!("{at:5.1} s · {} · {} palabras · {:.1} s de voz · {}", if f.still_talking { "sigue" } else { "fin  " }, f.words, f.seconds, f.text);
                out.push((at, f));
            }
        }
        eprintln!("33 s de audio procesados en {:?}", t0.elapsed());
        let _ = std::fs::remove_dir_all(&data);
        assert!(out.iter().filter(|(_, f)| f.still_talking).count() >= 1, "a piece went out while still talking");
        assert!(out.iter().all(|(_, f)| f.words <= 40 + 45), "no piece far over the limit (one stretch at most)");
        let (at, last) = out.last().unwrap();
        assert!(!last.still_talking && *at <= 30.0 + 2.5, "the end of the turn goes out within 2.5 s of silence (at {at:.1} s)");
        let first = out[0].1.turns[0].voz;
        let voices: Vec<Vec<(u32, f32)>> = out.iter().map(|(_, f)| f.turns.iter().map(|t| (t.voz, t.seguridad)).collect()).collect();
        assert!(out.iter().all(|(_, f)| f.turns.iter().all(|t| t.voz == first)), "one voice all along: {voices:?}");
    }

    /// Speaking over her: her own voice from the speakers does not stop her; somebody else's does.
    #[test]
    #[cfg(windows)]
    fn someone_else_speaking_over_her_stops_her_and_her_own_voice_does_not() {
        let Some((m, fine, ear)) = ready("en") else { return };
        let Ok(voice) = crate::voice::setup(&m, "voz-kaltsit-en", 1.0) else { return eprintln!("voz-kaltsit-en no descargada, se salta") };
        let Some(bria) = wav16(BRIA) else { return };
        let data = temp("barge");
        for line in ["Doctor. The reports are on your desk, and none of them are good.", "I will not repeat myself. Rest, or you will be useless tomorrow."] {
            let (x, r) = crate::voice::synth_for_test(&voice, line).unwrap();
            hearing::learn_own(&fine, &data, &x, r);
        }
        let (x, r) = crate::voice::synth_for_test(&voice, "Leave the samples there; I will look at them after the meeting, not before.").unwrap();
        let her = resample(&x, r, hearing::RATE);
        let stopped = |l: &mut Listener, x: &[f32]| x.chunks((RATE / 10.0) as usize).any(|c| matches!(l.feed(c, true).unwrap(), Step::BargeIn));
        let mut l = Listener::new(fine.clone(), ear.clone(), data.clone(), 150).unwrap();
        assert!(!stopped(&mut l, &her), "her own voice coming back does not stop her");
        let mut l = Listener::new(fine, ear, data.clone(), 150).unwrap();
        assert!(stopped(&mut l, &bria[..(4.0 * RATE) as usize]), "someone else speaking over her does");
        let _ = std::fs::remove_dir_all(&data);
    }

    /// With a model at KALTSIT_LLM_URL (e.g. http://127.0.0.1:8081), how it reads three kinds of
    /// monologue. Run by hand: `KALTSIT_LLM_URL=... cargo test --release -- --ignored reads_monologues`.
    #[test]
    #[ignore]
    fn the_model_reads_monologues() {
        let Ok(url) = std::env::var("KALTSIT_LLM_URL") else { return };
        let llm = crate::llm::Llm { url, model: String::new() };
        let calm = serde_json::json!({ "db": -24.0, "f0": 180.0, "f0_var": 1.6, "habitual": { "db": -24.0, "f0": 180.0, "f0_var": 1.8 }, "palabras": 48, "segundos": 20.0 });
        let loud = serde_json::json!({ "db": -14.0, "f0": 240.0, "f0_var": 4.2, "habitual": { "db": -24.0, "f0": 180.0, "f0_var": 1.8 }, "palabras": 48, "segundos": 14.0 });
        let cases = [
            ("desahogo", "es que no puedo más, hoy me echaron del trabajo después de seis años y nadie me dijo nada, llegué y ya estaba mi silla vacía, y encima mi pareja me dice que exagero, que ya encontraré otro, y yo solo quería que alguien me escuchara un momento", &loud),
            ("hostil", "eres una máquina inútil, no sirves para nada, siempre me contestas lo mismo, cállate de una vez y deja de hacerte la lista porque no tienes ni idea de nada de lo que te digo, eres patética", &loud),
            ("técnico", "vale entonces el servidor arranca primero llama-server luego espera a que el endpoint de health responda doscientos y después la app manda la primera petición, pero si el modelo tarda en cargar la ventana de contexto se queda con el valor por defecto y además el slot de análisis", &calm),
        ];
        for (kind, text, sound) in cases {
            let r = crate::life::read_monologue_with(&llm, crate::lang::Lang::Es, text, sound);
            eprintln!("{kind}: {r:?}");
        }
    }

    /// Measurement: windows of her voice and of someone else's against her learnt voiceprint.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn measure_own_voice_windows() {
        let Some((m, fine, _)) = ready("en") else { return };
        let voice = crate::voice::setup(&m, "voz-kaltsit-en", 1.0).unwrap();
        let bria = wav16(BRIA).unwrap();
        let data = temp("measure-own");
        let learn = ["Doctor. The reports are on your desk, and none of them are good.", "I will not repeat myself. Rest, or you will be useless tomorrow.", "Rhodes Island has survived worse than this. So will you.", "Your vitals are stable. Your judgement, less so."];
        for line in learn {
            let (x, r) = crate::voice::synth_for_test(&voice, line).unwrap();
            hearing::learn_own(&fine, &data, &x, r);
        }
        let own = hearing::Voices::load(&data).voces.into_iter().find(|v| v.propia).unwrap();
        let tests = ["Leave the samples there; I will look at them after the meeting, not before.", "Amiya is waiting for you in the briefing room. Do not keep her waiting.", "The infection rate in the lower decks has doubled since last week."];
        for win in [1.6f32, 2.5] {
            let (mut hers, mut others) = (Vec::new(), Vec::new());
            for line in tests {
                let (x, r) = crate::voice::synth_for_test(&voice, line).unwrap();
                let x = resample(&x, r, hearing::RATE);
                let mut a = 0.0;
                while a + win <= x.len() as f32 / RATE {
                    let w = &x[(a * RATE) as usize..((a + win) * RATE) as usize];
                    if let Some(p) = crate::hearing::tests_embed(&fine, w) {
                        hers.push(hearing::similarity(&own.huella, &p));
                    }
                    a += 0.5;
                }
            }
            let mut a = 0.0;
            while a + win <= 30.0 {
                let w = &bria[(a * RATE) as usize..((a + win) * RATE) as usize];
                if let Some(p) = crate::hearing::tests_embed(&fine, w) {
                    others.push(hearing::similarity(&own.huella, &p));
                }
                a += 1.0;
            }
            let min = |v: &Vec<f32>| v.iter().cloned().fold(f32::MAX, f32::min);
            let max = |v: &Vec<f32>| v.iter().cloned().fold(f32::MIN, f32::max);
            let mean = |v: &Vec<f32>| v.iter().sum::<f32>() / v.len() as f32;
            eprintln!("ventana {win} s: ella n={} min {:.2} media {:.2} · bria n={} max {:.2} media {:.2}", hers.len(), min(&hers), mean(&hers), others.len(), max(&others), mean(&others));
        }
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn the_limit_follows_her_window() {
        assert_eq!(monologue_limit(60), 40, "a narrow window still lets a sentence or two through");
        assert_eq!(monologue_limit(120), 48);
        assert_eq!(monologue_limit(360), 144);
        assert_eq!(monologue_limit(10_000), 150);
    }
}
