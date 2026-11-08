//! Telegram: each companion has a bot of her own (its token from @BotFather, in her local.json),
//! so the user can write to her from the phone and she answers there, in her own chat. The bot
//! listens with long polling (no server, nothing open to the internet) and only on the computer
//! where she thinks; it answers one account only, linked once with a code shown in the app.
//! Her plans are announced there a while before; sometimes she answers with a voice note.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// Her bot, in her local.json. The token never leaves this computer except to Telegram.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Telegram {
    pub token: String,
    /// The bot's @name (from getMe), to show.
    pub bot: String,
    /// The linked chat (the user's private chat with the bot); 0: none yet.
    pub chat: i64,
    /// Who it is on Telegram, to show.
    pub usuario: String,
    /// Her plans, announced there a while before.
    pub avisos: bool,
    /// How long before (minutes).
    pub aviso_min: u32,
    /// Chance that an answer also goes as a voice note (0..1).
    pub audio: f32,
    /// What she says at home (in the chat, on her own) also goes to Telegram: "nunca",
    /// "ausente" (when nobody has used the computer for a while) or "siempre".
    pub espejo: String,
}

impl Default for Telegram {
    fn default() -> Self {
        Telegram { token: String::new(), bot: String::new(), chat: 0, usuario: String::new(), avisos: true, aviso_min: 30, audio: 0.15, espejo: "ausente".into() }
    }
}

/// A text message that reached the bot.
#[derive(Clone, Debug, PartialEq)]
pub struct Incoming {
    pub update: i64,
    pub chat: i64,
    pub from: String,
    pub text: String,
    /// A private chat (groups are never answered).
    pub private: bool,
}

fn api(token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{token}/{method}")
}

/// Telegram's answer: `result` when ok, its description otherwise. The token never goes into an
/// error (it is part of the URL).
fn result(r: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    let v: Value = match r {
        Ok(r) => r.into_json().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(_, r)) => r.into_json().unwrap_or(json!({})),
        Err(ureq::Error::Transport(t)) => return Err(format!("Telegram no responde ({})", t.kind())),
    };
    if v["ok"].as_bool() == Some(true) {
        Ok(v["result"].clone())
    } else {
        Err(format!("Telegram: {}", v["description"].as_str().unwrap_or("error")))
    }
}

/// The bot's @name: also checks that the token is right.
pub fn get_me(token: &str) -> Result<String, String> {
    if token.trim().is_empty() || token.contains(['/', '?', '#', ' ']) {
        return Err("ese token no parece de BotFather (123456:ABC…)".into());
    }
    let v = result(ureq::get(&api(token, "getMe")).timeout(Duration::from_secs(15)).call())?;
    Ok(v["username"].as_str().unwrap_or("").to_string())
}

/// Waits up to `wait` seconds for messages after `offset`.
pub fn get_updates(token: &str, offset: i64, wait: u64) -> Result<Vec<Incoming>, String> {
    let url = format!("{}?offset={offset}&timeout={wait}&allowed_updates=%5B%22message%22%5D", api(token, "getUpdates"));
    let v = result(ureq::get(&url).timeout(Duration::from_secs(wait + 15)).call())?;
    Ok(parse_updates(&v))
}

pub fn parse_updates(v: &Value) -> Vec<Incoming> {
    v.as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter_map(|u| {
            let m = &u["message"];
            Some(Incoming {
                update: u["update_id"].as_i64()?,
                chat: m["chat"]["id"].as_i64()?,
                from: [m["from"]["first_name"].as_str(), m["from"]["username"].as_str().map(|s| s.trim_start_matches('@'))]
                    .into_iter()
                    .flatten()
                    .next()
                    .unwrap_or("")
                    .to_string(),
                text: m["text"].as_str().unwrap_or("").to_string(),
                private: m["chat"]["type"].as_str() == Some("private"),
            })
        })
        .collect()
}

/// The last update seen, so the next poll starts after it.
pub fn next_offset(msgs: &[Incoming], offset: i64) -> i64 {
    msgs.iter().map(|m| m.update + 1).max().unwrap_or(offset).max(offset)
}

pub fn send_message(token: &str, chat: i64, text: &str) -> Result<(), String> {
    // Telegram's limit is 4096 characters.
    let text: String = text.chars().take(4000).collect();
    result(ureq::post(&api(token, "sendMessage")).timeout(Duration::from_secs(20)).send_json(json!({ "chat_id": chat, "text": text }))).map(|_| ())
}

/// "Typing…" in the chat while she thinks (it lasts 5 s).
pub fn typing(token: &str, chat: i64) {
    let _ = ureq::post(&api(token, "sendChatAction")).timeout(Duration::from_secs(10)).send_json(json!({ "chat_id": chat, "action": "typing" }));
}

/// A voice note: OGG with Opus inside.
pub fn send_voice(token: &str, chat: i64, ogg: &[u8]) -> Result<(), String> {
    let boundary = format!("kaltsit{:x}", crate::clock::unix_now().to_bits());
    let body = multipart(&boundary, &[("chat_id", None, chat.to_string().as_bytes())], ("voice", "voz.ogg", "audio/ogg", ogg));
    result(
        ureq::post(&api(token, "sendVoice"))
            .timeout(Duration::from_secs(60))
            .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
            .send_bytes(&body),
    )
    .map(|_| ())
}

/// A multipart/form-data body: plain fields, then one file.
pub fn multipart(boundary: &str, fields: &[(&str, Option<&str>, &[u8])], file: (&str, &str, &str, &[u8])) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, _, value) in fields {
        out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
        out.extend_from_slice(value);
        out.extend_from_slice(b"\r\n");
    }
    let (name, filename, mime, bytes) = file;
    out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes());
    out.extend_from_slice(bytes);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

// ── linking ──

/// A 6-digit code from a random value, to type to the bot.
pub fn code_of(seed: u64) -> String {
    format!("{:06}", seed % 1_000_000)
}

/// Whether a message carries the code ("123456", "/start 123456", "123 456").
pub fn has_code(text: &str, code: &str) -> bool {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    !code.is_empty() && digits == code
}

// ── voice notes ──

/// ffmpeg, to turn her voice into a voice note (Telegram wants Opus): on the PATH or where
/// winget leaves it.
pub fn ffmpeg() -> Option<std::path::PathBuf> {
    let on_path = std::process::Command::new("ffmpeg").arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    if on_path {
        return Some("ffmpeg".into());
    }
    let packages = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").ok()?).join("Microsoft").join("WinGet").join("Packages");
    std::fs::read_dir(packages).ok()?.flatten().filter(|e| e.file_name().to_string_lossy().contains("FFmpeg")).find_map(|e| {
        std::fs::read_dir(e.path()).ok()?.flatten().map(|d| d.path().join("bin").join("ffmpeg.exe")).find(|p| p.is_file())
    })
}

/// A WAV as a voice note (OGG/Opus, mono, 48 kHz).
pub fn to_voice_note(ffmpeg: &std::path::Path, wav: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let dir = std::env::temp_dir();
    let id = format!("kaltsit-tg-{}-{:x}", std::process::id(), crate::clock::unix_now().to_bits());
    let (src, dst) = (dir.join(format!("{id}.wav")), dir.join(format!("{id}.ogg")));
    std::fs::File::create(&src).and_then(|mut f| f.write_all(wav)).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-v", "error", "-y", "-i"]).arg(&src).args(["-ac", "1", "-ar", "48000", "-c:a", "libopus", "-b:a", "32k"]).arg(&dst);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let ok = cmd.status().map_err(|e| e.to_string())?.success();
    let out = std::fs::read(&dst);
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
    if !ok {
        return Err("ffmpeg no pudo convertir la voz".into());
    }
    out.map_err(|e| e.to_string())
}

// ── what she says at home ──

/// Nobody has touched the computer for this long: the user is away.
pub const AWAY_S: f64 = 300.0;
/// Away this long, she writes on her own (once per absence)…
pub const CHECKIN_S: f64 = 20.0 * 60.0;
/// …and never more often than this.
pub const CHECKIN_GAP_S: f64 = 2.0 * 3600.0;

/// Whether she writes now on her own: away long enough, a new absence (`since`: when it began,
/// against the one she already wrote for) and not too soon after the last time.
pub fn checks_in(mode: &str, idle: f64, since: f64, wrote_for: f64, last: f64, now: f64) -> bool {
    mode != "nunca" && idle >= CHECKIN_S && (since - wrote_for).abs() > 60.0 && now - last >= CHECKIN_GAP_S
}

/// Whether what she says at home goes to Telegram too, with the computer idle for `idle` s.
pub fn mirrors(mode: &str, idle: f64) -> bool {
    match mode {
        "siempre" => true,
        "ausente" => idle >= AWAY_S,
        _ => false,
    }
}

// ── reminders ──

/// The plans to announce now: in the next `minutes`, not announced yet (`done`: their keys).
pub fn due(plans: &[(f64, String)], now: f64, minutes: u32, done: &[String]) -> Vec<(f64, String)> {
    plans.iter().filter(|(u, label)| *u > now && *u - now <= minutes as f64 * 60.0 && !done.contains(&key(*u, label))).cloned().collect()
}

pub fn key(u: f64, label: &str) -> String {
    format!("{}|{}", u.round() as i64, label.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_are_read_and_the_offset_moves_on() {
        let v = json!([
            { "update_id": 7, "message": { "chat": { "id": 42, "type": "private" }, "from": { "first_name": "Ana", "username": "ana" }, "text": "/start 123456" } },
            { "update_id": 8, "message": { "chat": { "id": -5, "type": "group" }, "from": { "username": "otro" }, "text": "hola" } },
            { "update_id": 9, "edited_message": {} }
        ]);
        let m = parse_updates(&v);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], Incoming { update: 7, chat: 42, from: "Ana".into(), text: "/start 123456".into(), private: true });
        assert!(!m[1].private);
        assert_eq!(next_offset(&m, 0), 9);
        assert_eq!(next_offset(&[], 9), 9);
    }

    #[test]
    fn the_code_is_found_however_it_is_typed() {
        let code = code_of(98_765_4321);
        assert_eq!(code.len(), 6);
        for t in [code.clone(), format!("/start {code}"), format!("{} {}", &code[..3], &code[3..])] {
            assert!(has_code(&t, &code), "{t}");
        }
        assert!(!has_code("hola", &code));
        assert!(!has_code(&format!("{code}1"), &code));
        assert!(!has_code("", ""));
    }

    #[test]
    fn a_multipart_body_has_the_field_and_the_file() {
        let b = multipart("XX", &[("chat_id", None, b"42")], ("voice", "v.ogg", "audio/ogg", b"OggS"));
        let s = String::from_utf8_lossy(&b);
        assert!(s.starts_with("--XX\r\nContent-Disposition: form-data; name=\"chat_id\"\r\n\r\n42\r\n"));
        assert!(s.contains("name=\"voice\"; filename=\"v.ogg\"\r\nContent-Type: audio/ogg\r\n\r\nOggS\r\n--XX--\r\n"));
    }

    #[test]
    fn reminders_go_once_within_the_window() {
        let now = 1_000_000.0;
        let plans = vec![(now + 600.0, "Dentista".to_string()), (now + 7200.0, "Cena".to_string()), (now - 60.0, "Pasado".to_string())];
        let d = due(&plans, now, 30, &[]);
        assert_eq!(d, vec![(now + 600.0, "Dentista".to_string())]);
        assert!(due(&plans, now, 30, &[key(now + 600.0, "dentista")]).is_empty(), "announced already");
    }

    #[test]
    fn what_she_says_at_home_goes_out_only_as_chosen() {
        assert!(mirrors("siempre", 0.0));
        assert!(!mirrors("ausente", 30.0), "someone is at the computer");
        assert!(mirrors("ausente", AWAY_S + 1.0));
        assert!(!mirrors("nunca", 10_000.0));
        assert!(!mirrors("", 10_000.0));
        assert_eq!(Telegram::default().espejo, "ausente");
    }

    /// Away 20 minutes she writes, once for that absence, and not again within two hours.
    #[test]
    fn she_writes_once_when_the_user_is_away() {
        let now = 100_000.0;
        let since = now - 1300.0;
        assert!(!checks_in("ausente", 600.0, now - 600.0, 0.0, 0.0, now), "ten minutes is not long enough");
        assert!(checks_in("ausente", 1300.0, since, 0.0, 0.0, now));
        assert!(!checks_in("ausente", 1310.0, since, since, now - 10.0, now + 10.0), "the same absence");
        assert!(!checks_in("ausente", 900.0 + CHECKIN_S, now, since, now - 1800.0, now + 1800.0), "a new absence, too soon");
        assert!(checks_in("siempre", CHECKIN_S, now, since, now - CHECKIN_GAP_S, now));
        assert!(!checks_in("nunca", 99_999.0, since, 0.0, 0.0, now));
    }

    #[test]
    fn a_bad_token_is_refused_without_asking_telegram() {
        assert!(get_me("").is_err());
        assert!(get_me("12/34").is_err());
    }

    /// Her real voice, a whole answer, as a voice note (with the voice and ffmpeg downloaded).
    #[test]
    fn her_answer_becomes_a_voice_note() {
        let m = crate::catalog::Manifest::load();
        let (Ok(s), Some(f)) = (crate::voice::setup(&m, "voz-lappland-en", 1.0), ffmpeg()) else { return eprintln!("sin voz o sin ffmpeg, se salta") };
        let (x, r) = crate::voice::render(&s, "Ciao, Doctor. Two sentences, one note. Did you miss me?").unwrap();
        assert!(x.len() as f32 / r as f32 > 2.0, "both sentences are in it");
        let ogg = to_voice_note(&f, &crate::download::wav(&x, r)).unwrap();
        eprintln!("nota de voz: {:.1} s · {} KB", x.len() as f32 / r as f32, ogg.len() / 1024);
        assert!(ogg.windows(8).any(|w| w == b"OpusHead"));
    }

    /// With ffmpeg here, a second of her voice becomes a voice note Telegram accepts (OGG/Opus).
    #[test]
    fn a_wav_becomes_a_voice_note_when_ffmpeg_is_here() {
        let Some(f) = ffmpeg() else { return eprintln!("sin ffmpeg, se salta") };
        let tone: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let ogg = to_voice_note(&f, &crate::download::wav(&tone, 24_000)).unwrap();
        assert!(ogg.starts_with(b"OggS") && ogg.windows(8).any(|w| w == b"OpusHead"), "OGG with Opus inside");
    }
}
