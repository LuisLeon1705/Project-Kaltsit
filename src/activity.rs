//! Her memory of what you work on. Every few seconds: which program is in front, its window's
//! title and how long since the last key or mouse movement (Windows tells all three for free,
//! no model involved). Consecutive samples of the same window make a session; a long gap without
//! input is an absence. Sessions go to a log per person and per day
//! (`data/actividad/<persona>/<AAAA-MM-DD>.jsonl`); her memory only gets the long sessions and
//! one summary a day. When someone comes back after an absence, she can tell them where they
//! left off.
//!
//! Never recorded: private/incognito windows, her own windows and the programs in
//! `Local::actividad.excluir`.

use crate::clock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How often the window in front is read.
pub const SAMPLE_S: f64 = 5.0;
/// No input for this long: idle (the session closes at the last input).
pub const IDLE_S: f64 = 120.0;
/// Idle this long and back: an absence worth telling where they left off.
pub const AWAY_S: f64 = 30.0 * 60.0;
/// A session shorter than this is a glance (alt-tab, a notification): not kept.
const MIN_SESSION_S: f64 = 10.0;
/// A session this long goes into her memory as well.
pub const MEMORABLE_S: f64 = 20.0 * 60.0;

/// What was in front at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub app: String,
    pub title: String,
    /// Seconds since the last key or mouse movement.
    pub idle_s: f64,
}

/// Some time on one window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub inicio: f64,
    pub fin: f64,
    pub app: String,
    pub titulo: String,
    /// Seconds with input on it (fin - inicio minus the idle bits).
    pub activo: f64,
}

/// What the tracker noticed.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A session ended (another window, idle, or something private came to the front).
    Closed(Session),
    /// Back after `away_s` without input; they left at `left`.
    Back { away_s: f64, left: f64 },
}

/// Never recorded: incognito/private windows, her own windows, excluded programs.
pub fn private(app: &str, title: &str, excluded: &[String]) -> bool {
    let t = title.to_lowercase();
    let a = app.to_lowercase();
    const PRIVATE: [&str; 6] = ["inprivate", "incognito", "incógnito", "private browsing", "navegación privada", "ventana privada"];
    PRIVATE.iter().any(|p| t.contains(p)) || a == "kaltsit" || t.starts_with("kal'tsit") || excluded.iter().any(|x| !x.trim().is_empty() && a == x.trim().to_lowercase())
}

/// The part of a title that names what is open: without unsaved-changes marks.
pub fn clean_title(t: &str) -> String {
    let t = t.trim().trim_start_matches(['●', '•', '*']).trim();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A friendlier name for a few common programs (the executable's name otherwise).
pub fn app_name(exe_stem: &str) -> String {
    match exe_stem.to_lowercase().as_str() {
        "code" => "Visual Studio Code",
        "chrome" => "Chrome",
        "msedge" => "Edge",
        "firefox" => "Firefox",
        "explorer" => "Explorador de archivos",
        "windowsterminal" => "Terminal",
        "powershell" | "pwsh" => "PowerShell",
        "cmd" => "Símbolo del sistema",
        "devenv" => "Visual Studio",
        "blender" => "Blender",
        "discord" => "Discord",
        "spotify" => "Spotify",
        "javaw" | "minecraft" => "Minecraft",
        "winword" => "Word",
        "excel" => "Excel",
        "powerpnt" => "PowerPoint",
        "obsidian" => "Obsidian",
        "telegram" => "Telegram",
        "steam" => "Steam",
        _ => return exe_stem.to_string(),
    }
    .to_string()
}

/// Turns samples into sessions and absences.
#[derive(Default)]
pub struct Tracker {
    current: Option<Session>,
    last_t: Option<f64>,
    away_from: Option<f64>,
}

impl Tracker {
    /// One sample at `t` (unix seconds). `None`: nothing to record now (private, locked, no window).
    pub fn feed(&mut self, t: f64, s: Option<Sample>) -> Vec<Event> {
        let mut out = Vec::new();
        let dt = self.last_t.map_or(0.0, |l| (t - l).clamp(0.0, 2.0 * SAMPLE_S));
        self.last_t = Some(t);
        let Some(s) = s else {
            self.close(t, &mut out);
            return out;
        };
        if s.idle_s >= IDLE_S {
            // Gone since the last input: the session ends there.
            let last_input = t - s.idle_s;
            if let Some(c) = &mut self.current {
                c.fin = c.fin.min(last_input).max(c.inicio);
            }
            self.close_at_fin(&mut out);
            self.away_from.get_or_insert(last_input);
            return out;
        }
        if let Some(left) = self.away_from.take() {
            if t - left >= AWAY_S {
                out.push(Event::Back { away_s: t - left, left });
            }
        }
        let title = clean_title(&s.title);
        match &mut self.current {
            Some(c) if c.app == s.app && c.titulo == title => {
                c.fin = t;
                c.activo += dt;
            }
            _ => {
                self.close(t, &mut out);
                self.current = Some(Session { inicio: t, fin: t, app: s.app, titulo: title, activo: 0.0 });
            }
        }
        out
    }

    fn close(&mut self, t: f64, out: &mut Vec<Event>) {
        if let Some(c) = &mut self.current {
            c.fin = c.fin.max(t.min(c.fin + SAMPLE_S));
        }
        self.close_at_fin(out);
    }

    fn close_at_fin(&mut self, out: &mut Vec<Event>) {
        if let Some(c) = self.current.take() {
            if c.activo >= MIN_SESSION_S {
                out.push(Event::Closed(c));
            }
        }
    }
}

// ── the log ──

/// A person's folder name: their name, safe for a path ("principal" if empty).
pub fn folder_of(person: &str) -> String {
    let s: String = person.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.trim_matches('_').is_empty() { "principal".into() } else { s }
}

fn day_key(unix: f64) -> Option<String> {
    let l = clock::to_local(unix)?;
    Some(format!("{:04}-{:02}-{:02}", l.year, l.month, l.day))
}

fn person_dir(data: &Path, person: &str) -> PathBuf {
    data.join("actividad").join(folder_of(person))
}

/// Appends a session to its day's log.
pub fn append(data: &Path, person: &str, s: &Session) -> Result<(), String> {
    use std::io::Write;
    let dir = person_dir(data, person);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let day = day_key(s.inicio).ok_or("fecha fuera de rango")?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{day}.jsonl"))).map_err(|e| e.to_string())?;
    writeln!(f, "{}", serde_json::to_string(s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// The sessions of one day ("AAAA-MM-DD"), in order.
pub fn day(data: &Path, person: &str, day: &str) -> Vec<Session> {
    let ok = day.len() == 10 && day.chars().all(|c| c.is_ascii_digit() || c == '-');
    let text = if ok { std::fs::read_to_string(person_dir(data, person).join(format!("{day}.jsonl"))).unwrap_or_default() } else { String::new() };
    let mut v: Vec<Session> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    v.sort_by(|a, b| a.inicio.total_cmp(&b.inicio));
    v
}

/// Every day with a log, and the active seconds in it, oldest first.
pub fn days(data: &Path, person: &str) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = std::fs::read_dir(person_dir(data, person))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".jsonl").map(String::from))
        .map(|d| {
            let total = day(data, person, &d).iter().map(|s| s.activo).sum();
            (d, total)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The sessions of the last `hours` before `until`, from the logs of that day and the one before.
pub fn before(data: &Path, person: &str, until: f64, hours: f64) -> Vec<Session> {
    let from = until - hours * 3600.0;
    let mut v: Vec<Session> = [day_key(from), day_key(until)].into_iter().flatten().collect::<std::collections::BTreeSet<_>>().iter().flat_map(|d| day(data, person, d)).filter(|s| s.fin > from && s.inicio <= until).collect();
    v.sort_by(|a, b| a.inicio.total_cmp(&b.inicio));
    v
}

/// Time per program and window, most first: (app, seconds, [(title, seconds)] most first).
pub fn by_app(sessions: &[Session]) -> Vec<(String, f64, Vec<(String, f64)>)> {
    let mut apps: Vec<(String, f64, Vec<(String, f64)>)> = Vec::new();
    for s in sessions {
        let i = match apps.iter().position(|a| a.0 == s.app) {
            Some(i) => i,
            None => {
                apps.push((s.app.clone(), 0.0, Vec::new()));
                apps.len() - 1
            }
        };
        apps[i].1 += s.activo;
        match apps[i].2.iter_mut().find(|t| t.0 == s.titulo) {
            Some(t) => t.1 += s.activo,
            None => apps[i].2.push((s.titulo.clone(), s.activo)),
        }
    }
    for a in &mut apps {
        a.2.sort_by(|x, y| y.1.total_cmp(&x.1));
    }
    apps.sort_by(|x, y| y.1.total_cmp(&x.1));
    apps
}

/// Where they left off: the last thing they were on, then what took most of the time before.
pub fn left_off(sessions: &[Session]) -> Vec<Session> {
    let Some(last) = sessions.iter().max_by(|a, b| a.fin.total_cmp(&b.fin)).cloned() else { return Vec::new() };
    let mut out = vec![last.clone()];
    let mut rest: Vec<&Session> = sessions.iter().filter(|s| !(s.app == last.app && s.titulo == last.titulo)).collect();
    rest.sort_by(|a, b| b.activo.total_cmp(&a.activo));
    for s in rest {
        if out.len() >= 3 {
            break;
        }
        if !out.iter().any(|o| o.app == s.app && o.titulo == s.titulo) {
            out.push(s.clone());
        }
    }
    out
}

/// "2 h 05 min", "35 min", "menos de un minuto".
pub fn hm(secs: f64, l: crate::lang::Lang) -> String {
    let m = (secs / 60.0).round() as i64;
    match (m, l) {
        (0, crate::lang::Lang::Es) => "menos de un minuto".into(),
        (0, crate::lang::Lang::En) => "under a minute".into(),
        (m, _) if m < 60 => format!("{m} min"),
        (m, _) => format!("{} h {:02} min", m / 60, m % 60),
    }
}

// ── reading the window in front (Windows) ──

#[cfg(windows)]
pub fn sample() -> Option<Sample> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};
    unsafe {
        let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        let idle_s = if GetLastInputInfo(&mut li) != 0 { GetTickCount().wrapping_sub(li.dwTime) as f64 / 1000.0 } else { 0.0 };
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        let mut exe = String::new();
        if !process.is_null() {
            let mut path = [0u16; 1024];
            let mut len = path.len() as u32;
            if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len) != 0 {
                exe = String::from_utf16_lossy(&path[..len as usize]);
            }
            CloseHandle(process);
        }
        let stem = Path::new(&exe).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if stem.is_empty() && title.is_empty() {
            return None;
        }
        Some(Sample { app: app_name(&stem), title, idle_s })
    }
}

#[cfg(not(windows))]
pub fn sample() -> Option<Sample> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(app: &str, title: &str, idle: f64) -> Option<Sample> {
        Some(Sample { app: app.into(), title: title.into(), idle_s: idle })
    }

    /// The real window in front, read for 15 s (run by hand: it shows what is open).
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn reads_the_real_window_in_front() {
        for _ in 0..3 {
            let s = sample();
            eprintln!("{s:?}");
            assert!(s.is_some_and(|s| !s.app.is_empty()));
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }

    #[test]
    fn samples_become_sessions_and_an_absence() {
        let mut t = Tracker::default();
        let mut ev = Vec::new();
        let mut at = 1000.0;
        for _ in 0..60 {
            ev.extend(t.feed(at, s("Visual Studio Code", "● hearing.rs - Kaltsit - Visual Studio Code", 1.0)));
            at += 5.0;
        }
        // A glance at a notification: too short to keep.
        ev.extend(t.feed(at, s("Discord", "Discord", 0.5)));
        at += 5.0;
        for _ in 0..20 {
            ev.extend(t.feed(at, s("Chrome", "docs.rs - Google Chrome", 2.0)));
            at += 5.0;
        }
        // Gone for 40 minutes: idle grows until they come back.
        let left = at;
        while at < left + 40.0 * 60.0 {
            ev.extend(t.feed(at, s("Chrome", "docs.rs - Google Chrome", at - left)));
            at += 5.0;
        }
        ev.extend(t.feed(at, s("Visual Studio Code", "hearing.rs - Kaltsit - Visual Studio Code", 0.5)));
        let closed: Vec<&Session> = ev.iter().filter_map(|e| if let Event::Closed(s) = e { Some(s) } else { None }).collect();
        assert_eq!(closed.len(), 2, "VS Code and Chrome; Discord was a glance: {closed:?}");
        assert_eq!(closed[0].titulo, "hearing.rs - Kaltsit - Visual Studio Code", "the unsaved mark goes");
        assert!((closed[0].activo - 295.0).abs() < 1.0, "{}", closed[0].activo);
        assert!(closed[1].fin <= left + 1.0, "Chrome ends at the last input, not when idle was noticed");
        let back: Vec<&Event> = ev.iter().filter(|e| matches!(e, Event::Back { .. })).collect();
        assert_eq!(back.len(), 1);
        if let Event::Back { away_s, left: l } = back[0] {
            assert!(*away_s >= AWAY_S && (l - left).abs() < 6.0);
        }
    }

    #[test]
    fn a_short_break_is_not_an_absence_and_private_windows_are_skipped() {
        let mut t = Tracker::default();
        let mut ev = Vec::new();
        for k in 0..10 {
            ev.extend(t.feed(k as f64 * 5.0, s("Code", "a.rs", 1.0)));
        }
        ev.extend(t.feed(50.0 + 300.0, s("Code", "a.rs", 300.0)));
        ev.extend(t.feed(50.0 + 305.0, s("Code", "a.rs", 0.2)));
        assert!(!ev.iter().any(|e| matches!(e, Event::Back { .. })), "5 minutes is a break");
        assert!(private("Chrome", "Nueva pestaña - Google Chrome (Incógnito)", &[]));
        assert!(private("Edge", "InPrivate - Microsoft Edge", &[]));
        assert!(private("KeePassXC", "Base de datos", &["keepassxc".into()]));
        assert!(private("kaltsit", "Kal'tsit · STONE", &[]));
        assert!(!private("Chrome", "docs.rs - Google Chrome", &["keepassxc".into()]));
    }

    #[test]
    fn the_log_is_kept_per_person_and_day_and_tells_where_they_left_off() {
        let data = std::env::temp_dir().join(format!("kaltsit-actividad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let noon = clock::parse("2026-10-30 12:00").unwrap();
        let ses = |a: f64, mins: f64, app: &str, t: &str| Session { inicio: noon + a * 60.0, fin: noon + (a + mins) * 60.0, app: app.into(), titulo: t.into(), activo: mins * 60.0 };
        for x in [ses(0.0, 50.0, "Code", "hearing.rs"), ses(50.0, 10.0, "Chrome", "docs.rs"), ses(60.0, 30.0, "Code", "openmic.rs"), ses(90.0, 5.0, "Discord", "general")] {
            append(&data, "Ana López", &x).unwrap();
        }
        append(&data, "Otra", &ses(0.0, 5.0, "Code", "x.rs")).unwrap();
        assert_eq!(day(&data, "Ana López", "2026-10-30").len(), 4);
        assert_eq!(days(&data, "Ana López"), vec![("2026-10-30".to_string(), 95.0 * 60.0)]);
        assert!(day(&data, "Ana López", "../../voces").is_empty(), "no way out of the folder");
        let apps = by_app(&day(&data, "Ana López", "2026-10-30"));
        assert_eq!((apps[0].0.as_str(), apps[0].1), ("Code", 80.0 * 60.0));
        assert_eq!(apps[0].2[0].0, "hearing.rs");
        let left = left_off(&before(&data, "Ana López", noon + 95.0 * 60.0, 2.0));
        let names: Vec<&str> = left.iter().map(|s| s.titulo.as_str()).collect();
        assert_eq!(names, vec!["general", "hearing.rs", "openmic.rs"], "the last thing, then what took the most");
        let _ = std::fs::remove_dir_all(&data);
    }
}
