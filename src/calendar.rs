//! The user's calendar on Google or Outlook, if they connect it: the companions read what is in
//! it (it joins the shared agenda as `google` / `outlook`) and add their own plans to it (what
//! the user told them, with a line saying who added it). One connection for the household, in
//! `data/comun/calendario.json`; only the lead of each computer syncs, every few minutes.
//!
//! Connecting is OAuth for desktop apps, with PKCE and a one-time page on 127.0.0.1: the user
//! registers the app once (a Google Cloud OAuth client of type "Desktop", or an Azure app with
//! the "Mobile and desktop" platform and redirect `http://localhost`) and pastes its client id
//! here. The refresh token stays on this computer; access tokens live only in memory.

use crate::common::Plan;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const PROVIDERS: [&str; 2] = ["google", "outlook"];
/// How far ahead events are read.
const AHEAD_S: f64 = 30.0 * 86_400.0;
/// A plan without an end lasts this long in the calendar.
const EVENT_S: f64 = 3600.0;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Account {
    pub client_id: String,
    /// Google's desktop clients have one (it is not secret for an installed app); Outlook none.
    pub client_secret: String,
    /// Empty: not connected.
    pub refresh: String,
    /// The account connected, to show.
    pub cuenta: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Calendars {
    pub google: Account,
    pub outlook: Account,
    /// Plans already added to a calendar ("google|<unix>|<label>").
    pub empujados: Vec<String>,
    /// When it last synced (unix) and what went wrong then, if anything.
    pub ultimo: f64,
    pub error: String,
}

fn path(root: &Path) -> PathBuf {
    crate::common::dir(root).join("calendario.json")
}

pub fn load(root: &Path) -> Calendars {
    std::fs::read_to_string(path(root)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(root: &Path, c: &Calendars) {
    let _ = std::fs::create_dir_all(crate::common::dir(root));
    let _ = std::fs::write(path(root), serde_json::to_string_pretty(c).unwrap_or_default());
}

impl Calendars {
    pub fn account(&mut self, provider: &str) -> Result<&mut Account, String> {
        match provider {
            "google" => Ok(&mut self.google),
            "outlook" => Ok(&mut self.outlook),
            _ => Err(format!("no conozco el calendario «{provider}»")),
        }
    }

    /// What the app shows: no secrets.
    pub fn public_json(&self) -> Value {
        let acc = |a: &Account| json!({ "configurado": !a.client_id.is_empty(), "conectado": !a.refresh.is_empty(), "cuenta": a.cuenta, "client_id": a.client_id, "secreto": !a.client_secret.is_empty() });
        json!({ "google": acc(&self.google), "outlook": acc(&self.outlook), "ultimo": self.ultimo, "error": self.error, "anadidos": self.empujados.len() })
    }
}

// ── time ──

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// "2026-11-05T21:00:00Z".
pub fn rfc3339(unix: f64) -> String {
    let s = unix.round() as i64;
    let (y, m, d) = civil_from_days(s.div_euclid(86_400));
    let t = s.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

/// An RFC 3339 time ("…Z", "…-04:00", Graph's "2026-11-05T14:00:00.0000000" in UTC) or a bare
/// date (an all-day event: its local midnight).
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.len() == 10 {
        let l = crate::clock::Local { year: y as i32, month: mo as u32, day: d as u32, hour: 0, minute: 0, weekday: 0 };
        return crate::clock::from_local(&l);
    }
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19).unwrap_or(0));
    let mut unix = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se;
    let rest = &s[19.min(s.len())..];
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
        let oh = rest.get(1..3)?.parse::<i64>().ok()?;
        let om = rest.get(4..6).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
        let off = oh * 3600 + om * 60;
        unix -= if sign == '+' { off } else { -off };
    }
    Some(unix as f64)
}

// ── OAuth ──

fn random_b64(n: usize) -> String {
    use base64::Engine;
    let mut bytes = Vec::new();
    while bytes.len() < n {
        bytes.extend_from_slice(x25519_dalek::StaticSecret::random_from_rng(chacha20poly1305::aead::OsRng).as_bytes());
    }
    bytes.truncate(n);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE: the verifier kept here, and its S256 challenge sent to the browser.
pub fn pkce() -> (String, String) {
    use base64::Engine;
    use sha2::Digest;
    let verifier = random_b64(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                out.push(u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("3F"), 16).unwrap_or(b'?'));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A query parameter of a path ("/?code=…&state=…").
pub fn query(path: &str, key: &str) -> Option<String> {
    path.split_once('?')?.1.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| dec(v)))
}

fn scopes(provider: &str) -> &'static str {
    if provider == "google" {
        "https://www.googleapis.com/auth/calendar.events"
    } else {
        "offline_access Calendars.ReadWrite User.Read"
    }
}

fn token_url(provider: &str) -> &'static str {
    if provider == "google" {
        "https://oauth2.googleapis.com/token"
    } else {
        "https://login.microsoftonline.com/common/oauth2/v2.0/token"
    }
}

pub fn auth_url(provider: &str, client_id: &str, redirect: &str, challenge: &str, state: &str) -> String {
    let base = if provider == "google" { "https://accounts.google.com/o/oauth2/v2/auth" } else { "https://login.microsoftonline.com/common/oauth2/v2.0/authorize" };
    let mut u = format!(
        "{base}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        enc(client_id),
        enc(redirect),
        enc(scopes(provider))
    );
    if provider == "google" {
        u.push_str("&access_type=offline&prompt=consent");
    } else {
        u.push_str("&prompt=select_account");
    }
    u
}

fn post_form(url: &str, form: &[(&str, &str)]) -> Result<Value, String> {
    match ureq::post(url).timeout(Duration::from_secs(30)).send_form(form) {
        Ok(r) => r.into_json().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(_, r)) => {
            let v: Value = r.into_json().unwrap_or(json!({}));
            Err(v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("el servidor rechazó la petición").to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn open_browser(url: &str) -> Result<(), String> {
    std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Connects a calendar: the browser asks the user to sign in and accept; the answer comes back
/// to a page on this computer. Waits up to three minutes. Returns the account connected.
pub fn connect(root: &Path, provider: &str) -> Result<String, String> {
    let mut cal = load(root);
    let acc = cal.account(provider)?.clone();
    if acc.client_id.trim().is_empty() {
        return Err("falta el client id de la app registrada".into());
    }
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = server.server_addr().to_ip().map(|a| a.port()).ok_or("sin puerto")?;
    let host = if provider == "google" { "127.0.0.1" } else { "localhost" };
    let redirect = format!("http://{host}:{port}");
    let (verifier, challenge) = pkce();
    let state = random_b64(16);
    open_browser(&auth_url(provider, &acc.client_id, &redirect, &challenge, &state))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    let code = loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err("no llegó la respuesta del navegador (tres minutos)".into());
        }
        let Ok(Some(req)) = server.recv_timeout(left) else { continue };
        let url = req.url().to_string();
        let (code, error) = (query(&url, "code"), query(&url, "error"));
        if code.is_none() && error.is_none() {
            let _ = req.respond(tiny_http::Response::empty(404));
            continue;
        }
        let ok = code.is_some() && query(&url, "state").as_deref() == Some(state.as_str());
        let page = if ok { "Listo: el calendario quedó conectado. Puedes cerrar esta pestaña." } else { "No se conectó el calendario. Puedes cerrar esta pestaña y volver a intentarlo." };
        let html = format!("<!doctype html><meta charset=utf-8><title>Kal'tsit</title><body style=\"font:16px system-ui;background:#0f1416;color:#e6e9e3;display:grid;place-items:center;height:90vh\"><p>{page}</p>");
        let _ = req.respond(tiny_http::Response::from_string(html).with_header("Content-Type: text/html; charset=utf-8".parse::<tiny_http::Header>().unwrap()));
        if !ok {
            return Err(error.map(|e| format!("el calendario respondió: {e}")).unwrap_or_else(|| "respuesta inesperada".into()));
        }
        break code.unwrap();
    };
    let mut form = vec![("client_id", acc.client_id.as_str()), ("code", code.as_str()), ("code_verifier", verifier.as_str()), ("grant_type", "authorization_code"), ("redirect_uri", redirect.as_str())];
    if !acc.client_secret.is_empty() {
        form.push(("client_secret", acc.client_secret.as_str()));
    }
    if provider == "outlook" {
        form.push(("scope", scopes(provider)));
    }
    let v = post_form(token_url(provider), &form)?;
    let refresh = v["refresh_token"].as_str().ok_or("no llegó el token de actualización (¿faltó aceptar el acceso sin conexión?)")?.to_string();
    let access = v["access_token"].as_str().unwrap_or_default().to_string();
    remember_access(provider, &access, v["expires_in"].as_f64().unwrap_or(3600.0));
    let who = account_name(provider, &access).unwrap_or_default();
    let a = cal.account(provider)?;
    a.refresh = refresh;
    a.cuenta = who.clone();
    cal.error.clear();
    save(root, &cal);
    Ok(who)
}

pub fn disconnect(root: &Path, provider: &str) -> Result<(), String> {
    let mut cal = load(root);
    let a = cal.account(provider)?;
    a.refresh.clear();
    a.cuenta.clear();
    with_tokens(|t| t.remove(provider));
    save(root, &cal);
    Ok(())
}

static TOKENS: Mutex<Option<HashMap<String, (String, f64)>>> = Mutex::new(None);

fn with_tokens<R>(f: impl FnOnce(&mut HashMap<String, (String, f64)>) -> R) -> R {
    f(TOKENS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new))
}

fn remember_access(provider: &str, token: &str, expires_in: f64) {
    if !token.is_empty() {
        with_tokens(|t| t.insert(provider.into(), (token.into(), crate::clock::unix_now() + expires_in - 120.0)));
    }
}

/// A valid access token: the one in memory, or a new one from the refresh token.
fn access(provider: &str, acc: &Account) -> Result<String, String> {
    if let Some((t, until)) = with_tokens(|t| t.get(provider).cloned()) {
        if crate::clock::unix_now() < until {
            return Ok(t);
        }
    }
    let mut form = vec![("client_id", acc.client_id.as_str()), ("refresh_token", acc.refresh.as_str()), ("grant_type", "refresh_token")];
    if !acc.client_secret.is_empty() {
        form.push(("client_secret", acc.client_secret.as_str()));
    }
    if provider == "outlook" {
        form.push(("scope", scopes(provider)));
    }
    let v = post_form(token_url(provider), &form)?;
    let t = v["access_token"].as_str().ok_or("sin token de acceso")?.to_string();
    remember_access(provider, &t, v["expires_in"].as_f64().unwrap_or(3600.0));
    Ok(t)
}

fn get(url: &str, token: &str) -> Result<Value, String> {
    let r = ureq::get(url).timeout(Duration::from_secs(30)).set("Authorization", &format!("Bearer {token}")).set("Prefer", "outlook.timezone=\"UTC\"").call();
    match r {
        Ok(r) => r.into_json().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(code, r)) => Err(format!("{code}: {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>())),
        Err(e) => Err(e.to_string()),
    }
}

fn account_name(provider: &str, token: &str) -> Result<String, String> {
    if provider == "google" {
        // The calendar itself needs a wider permission than events; a list of events says
        // whose calendar it is (`summary` of the primary one is its owner's address).
        Ok(get("https://www.googleapis.com/calendar/v3/calendars/primary/events?maxResults=1", token)?["summary"].as_str().unwrap_or("").to_string())
    } else {
        let v = get("https://graph.microsoft.com/v1.0/me", token)?;
        Ok(v["mail"].as_str().or(v["userPrincipalName"].as_str()).unwrap_or("").to_string())
    }
}

// ── events ──

/// The events of the next weeks, as plans of `provider`, and whose calendar it is (Google says
/// it with them).
fn events(provider: &str, token: &str, from: f64, to: f64) -> Result<(Vec<Plan>, Option<String>), String> {
    let url = if provider == "google" {
        format!("https://www.googleapis.com/calendar/v3/calendars/primary/events?singleEvents=true&orderBy=startTime&maxResults=250&timeMin={}&timeMax={}", enc(&rfc3339(from)), enc(&rfc3339(to)))
    } else {
        format!("https://graph.microsoft.com/v1.0/me/calendarView?startDateTime={}&endDateTime={}&$top=250&$select=subject,bodyPreview,start,isAllDay", enc(&rfc3339(from)), enc(&rfc3339(to)))
    };
    let v = get(&url, token)?;
    Ok((parse_events(provider, &v), v["summary"].as_str().filter(|s| s.contains('@')).map(String::from)))
}

pub fn parse_events(provider: &str, v: &Value) -> Vec<Plan> {
    let list = if provider == "google" { &v["items"] } else { &v["value"] };
    list.as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter_map(|e| {
            let (label, text, start) = if provider == "google" {
                (e["summary"].as_str()?, e["description"].as_str().unwrap_or(""), e["start"]["dateTime"].as_str().or(e["start"]["date"].as_str())?)
            } else {
                (e["subject"].as_str()?, e["bodyPreview"].as_str().unwrap_or(""), e["start"]["dateTime"].as_str()?)
            };
            let start = if provider == "outlook" && e["isAllDay"].as_bool() == Some(true) { &start[..10.min(start.len())] } else { start };
            Some(Plan { u: parse_time(start)?, label: label.trim().to_string(), texto: text.chars().take(300).collect(), de: provider.into() })
        })
        .filter(|p| !p.label.is_empty())
        .collect()
}

/// The event a companion's plan becomes.
pub fn event_body(provider: &str, p: &Plan, by: &str) -> Value {
    let note = if p.texto.is_empty() { format!("Añadido por {by}.") } else { format!("{}\n\nAñadido por {by}.", p.texto) };
    if provider == "google" {
        json!({ "summary": p.label, "description": note, "start": { "dateTime": rfc3339(p.u) }, "end": { "dateTime": rfc3339(p.u + EVENT_S) } })
    } else {
        json!({ "subject": p.label, "body": { "contentType": "text", "content": note }, "start": { "dateTime": rfc3339(p.u).trim_end_matches('Z'), "timeZone": "UTC" }, "end": { "dateTime": rfc3339(p.u + EVENT_S).trim_end_matches('Z'), "timeZone": "UTC" } })
    }
}

fn create(provider: &str, token: &str, body: &Value) -> Result<(), String> {
    let url = if provider == "google" { "https://www.googleapis.com/calendar/v3/calendars/primary/events" } else { "https://graph.microsoft.com/v1.0/me/events" };
    match ureq::post(url).timeout(Duration::from_secs(30)).set("Authorization", &format!("Bearer {token}")).send_json(body.clone()) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, r)) => Err(format!("{code}: {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>())),
        Err(e) => Err(e.to_string()),
    }
}

/// The same plan as an event already there: the same minute and a name that says the same.
pub fn already_there(p: &Plan, there: &[Plan]) -> bool {
    let a = p.label.to_lowercase();
    there.iter().any(|e| {
        let b = e.label.to_lowercase();
        (e.u - p.u).abs() < 60.0 && (a == b || a.contains(&b) || b.contains(&a))
    })
}

/// What to add: future plans of the companions here, not added before nor already there.
pub fn to_add<'a>(provider: &str, mine: &'a [Plan], there: &[Plan], done: &[String], now: f64) -> Vec<&'a Plan> {
    mine.iter().filter(|p| p.u > now && !done.contains(&push_key(provider, p)) && !already_there(p, there)).collect()
}

pub fn push_key(provider: &str, p: &Plan) -> String {
    format!("{provider}|{}|{}", p.u.round() as i64, p.label.to_lowercase())
}

/// One round: each connected calendar's events into the agenda, and the plans of the
/// companions here (`here`: id and name) into the calendar. Whether the agenda changed.
pub fn sync(root: &Path, here: &[(&str, &str)]) -> bool {
    let mut cal = load(root);
    let now = crate::clock::unix_now();
    let mut changed = false;
    let mut errors = Vec::new();
    let agenda = crate::common::bank(root).agenda;
    for provider in PROVIDERS {
        let acc = cal.account(provider).map(|a| a.clone()).unwrap_or_default();
        if acc.refresh.is_empty() {
            continue;
        }
        let mut round = || -> Result<bool, String> {
            let token = access(provider, &acc)?;
            let (there, owner) = events(provider, &token, now - 86_400.0, now + AHEAD_S)?;
            if let Some(o) = owner.filter(|_| acc.cuenta.is_empty()) {
                if let Ok(a) = cal.account(provider) {
                    a.cuenta = o;
                }
            }
            let mut changed = agenda.get(provider) != Some(&there);
            if changed {
                crate::common::export_agenda(root, provider, &there);
            }
            for (id, name) in here {
                let Some(mine) = agenda.get(*id) else { continue };
                for p in to_add(provider, mine, &there, &cal.empujados, now) {
                    create(provider, &token, &event_body(provider, p, name))?;
                    cal.empujados.push(push_key(provider, p));
                    changed = true;
                }
            }
            Ok(changed)
        };
        match round() {
            Ok(c) => changed |= c,
            Err(e) => errors.push(format!("{provider}: {e}")),
        }
    }
    let keep = cal.empujados.len().saturating_sub(500);
    cal.empujados.drain(..keep);
    cal.ultimo = now;
    cal.error = errors.join(" · ");
    save(root, &cal);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against the calendar connected in this install's `data` (read only): whose it is and what
    /// is in it.
    #[test]
    #[ignore]
    fn the_connected_calendar_answers() {
        let root = Path::new("data");
        let cal = load(root);
        for provider in PROVIDERS {
            let acc = cal.clone().account(provider).unwrap().clone();
            if acc.refresh.is_empty() {
                continue;
            }
            let token = access(provider, &acc).unwrap();
            let now = crate::clock::unix_now();
            let (there, owner) = events(provider, &token, now - 86_400.0, now + AHEAD_S).unwrap();
            eprintln!("{provider}: cuenta {:?} · nombre {:?}", owner, account_name(provider, &token));
            for p in &there {
                eprintln!("  {} · {}", crate::clock::to_local(p.u).map(|l| crate::clock::iso(&l)).unwrap_or_default(), p.label);
            }
        }
    }

    #[test]
    fn times_go_both_ways() {
        assert_eq!(rfc3339(0.0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_793_836_800.0), "2026-11-05T00:00:00Z");
        assert_eq!(parse_time("2026-11-05T00:00:00Z"), Some(1_793_836_800.0));
        assert_eq!(parse_time("2026-11-04T20:00:00-04:00"), Some(1_793_836_800.0), "an offset");
        assert_eq!(parse_time("2026-11-05T00:00:00.0000000"), Some(1_793_836_800.0), "Graph's, in UTC");
        assert_eq!(parse_time("2026-11-05T05:30:00+05:30"), Some(1_793_836_800.0));
        for u in [951_782_400.0, 1_709_164_800.0, 4_102_444_800.0] {
            assert_eq!(parse_time(&rfc3339(u)), Some(u), "{u}: leap days and far years");
        }
        assert!(parse_time("2026-11-05").is_some(), "an all-day event");
        assert_eq!(parse_time("mañana"), None);
    }

    #[test]
    fn events_of_both_calendars_are_read() {
        let g = json!({ "items": [
            { "summary": "Dentista", "description": "Llevar la tarjeta", "start": { "dateTime": "2026-11-05T10:00:00-04:00" } },
            { "summary": "Feriado", "start": { "date": "2026-11-06" } },
            { "start": { "dateTime": "2026-11-05T10:00:00Z" } }
        ] });
        let p = parse_events("google", &g);
        assert_eq!(p.len(), 2, "an event without a name is skipped");
        assert_eq!((p[0].label.as_str(), p[0].u, p[0].de.as_str()), ("Dentista", 1_793_887_200.0, "google"));
        let o = json!({ "value": [{ "subject": "Reunión", "bodyPreview": "", "isAllDay": false, "start": { "dateTime": "2026-11-05T14:00:00.0000000", "timeZone": "UTC" } }] });
        assert_eq!(parse_events("outlook", &o)[0].u, 1_793_836_800.0 + 14.0 * 3600.0);
    }

    #[test]
    fn a_plan_is_added_once_and_not_on_top_of_the_same_event() {
        let plan = |u: f64, l: &str| Plan { u, label: l.into(), texto: String::new(), de: "kaltsit".into() };
        let now = 1_000.0;
        let mine = vec![plan(5_000.0, "Dentista"), plan(9_000.0, "Cena con Ana"), plan(500.0, "Pasado")];
        let there = vec![Plan { de: "google".into(), ..plan(5_020.0, "Dentista (Dr. Ruiz)") }];
        let add = to_add("google", &mine, &there, &[], now);
        assert_eq!(add.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Cena con Ana"]);
        assert!(to_add("google", &mine, &there, &[push_key("google", &mine[1])], now).is_empty());
        assert_eq!(to_add("outlook", &mine, &[], &[push_key("google", &mine[1])], now).len(), 2, "each calendar on its own");
    }

    #[test]
    fn the_event_says_who_added_it() {
        let p = Plan { u: 1_793_836_800.0, label: "Cena".into(), texto: "En casa".into(), de: "lappland".into() };
        let g = event_body("google", &p, "Lappland");
        assert_eq!(g["start"]["dateTime"], "2026-11-05T00:00:00Z");
        assert_eq!(g["end"]["dateTime"], "2026-11-05T01:00:00Z");
        assert!(g["description"].as_str().unwrap().ends_with("Añadido por Lappland."));
        let o = event_body("outlook", &p, "Lappland");
        assert_eq!(o["start"], json!({ "dateTime": "2026-11-05T00:00:00", "timeZone": "UTC" }));
    }

    #[test]
    fn the_sign_in_page_carries_pkce_and_comes_back_here() {
        let (v, c) = pkce();
        assert!(v.len() >= 43 && c.len() == 43, "RFC 7636 lengths");
        assert_ne!(v, pkce().0);
        let u = auth_url("google", "id.apps", "http://127.0.0.1:5000", &c, "st");
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5000") && u.contains("code_challenge_method=S256") && u.contains("access_type=offline"));
        assert!(auth_url("outlook", "abc", "http://localhost:5000", &c, "st").contains("scope=offline_access%20Calendars.ReadWrite%20User.Read"));
        assert_eq!(query("/?code=4%2F0Ab_x&state=st", "code").as_deref(), Some("4/0Ab_x"));
        assert_eq!(query("/?error=access_denied", "code"), None);
    }

    #[test]
    fn secrets_never_reach_the_app() {
        let mut c = Calendars::default();
        c.google = Account { client_id: "id".into(), client_secret: "sec".into(), refresh: "ref".into(), cuenta: "ana@x".into() };
        let s = c.public_json().to_string();
        assert!(!s.contains("sec\"") && !s.contains("ref"), "{s}");
        assert_eq!(c.public_json()["google"]["conectado"], true);
    }
}
