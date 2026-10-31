//! The model, as a process this app owns: started when this computer becomes her brain and
//! stopped when it hands the brain over (so the VRAM is free for the one that thinks now).
//! Its output goes to a log file, so «Modelos → Motor» can show why it did not start; the
//! engines on this computer are found and offered there.

use crate::local::Llama;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The last llama-server this app launched.
#[derive(Clone, Debug)]
pub struct Launch {
    pub args: Vec<String>,
    pub pid: u32,
    /// Unix seconds.
    pub at: f64,
}

pub struct Motor {
    /// Can change while she runs (another model chosen in «Modelos»); the port stays.
    cfg: Mutex<Llama>,
    port: u16,
    child: Mutex<Option<Child>>,
    last: Mutex<Option<Launch>>,
}

// ── arguments ──

fn flag_pos(args: &[String], names: &[&str]) -> Option<usize> {
    args.iter().position(|a| names.contains(&a.as_str()))
}

/// The context size in `args` (`-c` / `--ctx-size`).
pub fn ctx_of(args: &[String]) -> Option<u32> {
    flag_pos(args, &["-c", "--ctx-size"]).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok())
}

pub fn with_ctx(args: &[String], ctx: u32) -> Vec<String> {
    let mut a = args.to_vec();
    match flag_pos(&a, &["-c", "--ctx-size"]) {
        Some(i) if i + 1 < a.len() => a[i + 1] = ctx.to_string(),
        _ => a.extend(["-c".to_string(), ctx.to_string()]),
    }
    a
}

const NGL: [&str; 3] = ["-ngl", "--gpu-layers", "--n-gpu-layers"];
const FIT_TARGET: [&str; 2] = ["-fitt", "--fit-target"];

fn without(args: &[String], names: &[&str]) -> Vec<String> {
    let mut a = args.to_vec();
    while let Some(i) = flag_pos(&a, names) {
        a.drain(i..(i + 2).min(a.len()));
    }
    a
}

/// GPU layers decided by llama.cpp itself (`--fit`, which measures the free VRAM exactly):
/// no `-ngl`, and a 512 MiB margin instead of its default 1 GiB.
pub fn gpu_auto(args: &[String]) -> bool {
    flag_pos(args, &NGL).is_none()
}

pub fn with_gpu_auto(args: &[String], auto: bool) -> Vec<String> {
    let mut a = without(&without(args, &NGL), &FIT_TARGET);
    if auto {
        a.extend(["-fitt".to_string(), "512".to_string()]);
    } else {
        a.splice(0..0, ["-ngl".to_string(), "99".to_string()]);
    }
    a
}

// ── the engines on this computer ──

/// llama-server.exe files found: the configured one, the downloaded one, Chats' and the PATH's.
pub fn engines(configured: &str) -> Vec<(String, &'static str)> {
    let mut found: Vec<(String, &'static str)> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut add = |p: String, origin: &'static str| {
        if let Ok(c) = std::fs::canonicalize(&p) {
            if c.is_file() && !seen.contains(&c) {
                seen.push(c);
                found.push((p, origin));
            }
        }
    };
    add(configured.to_string(), "configurado");
    add(r"modelos\bin\llama\llama-server.exe".to_string(), "descargado");
    add(r"..\Chats\models\bin\llama\llama-server.exe".to_string(), "Chats");
    if let Some(out) = quiet("where", &["llama-server"]) {
        for l in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
            add(l.to_string(), "PATH");
        }
    }
    found
}

fn quiet(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(cmd);
    c.args(args).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    out.status.success().then(|| format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// "build 11633" of a llama-server (asked once per file).
pub fn engine_version(path: &str) -> String {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(path) {
        return v.clone();
    }
    let v = quiet(path, &["--version"])
        .and_then(|o| o.lines().find(|l| l.contains("version:")).map(|l| l.split("(").nth(1).unwrap_or(l).split(',').next().unwrap_or(l).trim().to_string()))
        .unwrap_or_default();
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_string(), v.clone());
    v
}

/// A Windows job that kills its processes when this app goes away, however it goes (crash,
/// task manager, power button): a llama-server never outlives her holding the VRAM.
#[cfg(windows)]
fn bind_to_app(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::*;
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32);
        job as usize
    });
    unsafe {
        AssignProcessToJobObject(job as _, child.as_raw_handle() as _);
    }
}

#[cfg(not(windows))]
fn bind_to_app(_child: &Child) {}

impl Motor {
    pub fn new(cfg: Llama) -> Self {
        Motor { port: cfg.puerto, cfg: Mutex::new(cfg), child: Mutex::new(None), last: Mutex::new(None) }
    }

    /// Where llama-server writes what it says (truncated at every start).
    pub fn log_path(&self) -> PathBuf {
        std::env::temp_dir().join(format!("kaltsit-llama-server-{}.log", self.port))
    }

    /// The last `n` lines of its log.
    pub fn log_tail(&self, n: usize) -> Vec<String> {
        let s = std::fs::read(self.log_path()).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
        let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect()
    }

    /// Running (started here, still alive), its launch, and whether it answers.
    pub fn status(&self) -> Value {
        let running = self.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut().is_some_and(|ch| ch.try_wait().ok().flatten().is_none());
        let healthy = self.healthy();
        let last = self.last.lock().unwrap_or_else(|e| e.into_inner()).clone();
        json!({
            "propio": running,
            "responde": healthy,
            // Something answers on the port but this app did not start it.
            "externo": healthy && !running,
            "pid": last.as_ref().filter(|_| running).map(|l| l.pid),
            "desde": last.as_ref().filter(|_| running).map(|l| l.at),
            "args": last.map(|l| l.args).unwrap_or_default(),
        })
    }

    pub fn cfg(&self) -> Llama {
        self.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The model the next `start` loads. The one running, if any, keeps running until `stop`.
    pub fn set_cfg(&self, mut cfg: Llama) {
        cfg.puerto = self.port;
        *self.cfg.lock().unwrap_or_else(|e| e.into_inner()) = cfg;
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }


    pub fn healthy(&self) -> bool {
        ureq::get(&format!("{}/health", self.url())).timeout(Duration::from_secs(2)).call().is_ok()
    }

    /// Starts llama-server if nothing answers on its port, and waits until it is ready.
    pub fn start(&self, wait: Duration) -> Result<(), String> {
        if self.healthy() {
            return Ok(());
        }
        {
            let mut c = self.child.lock().unwrap_or_else(|e| e.into_inner());
            let running = c.as_mut().is_some_and(|ch| ch.try_wait().ok().flatten().is_none());
            if !running {
                let cfg = self.cfg();
                let server_path = if std::path::Path::new(&cfg.servidor).exists() {
                    cfg.servidor.clone()
                } else if std::path::Path::new(r"modelos\bin\llama\llama-server.exe").exists() {
                    r"modelos\bin\llama\llama-server.exe".to_string()
                } else {
                    cfg.servidor.clone()
                };

                let model_path = if std::path::Path::new(&cfg.modelo).exists() {
                    cfg.modelo.clone()
                } else if std::path::Path::new(r"modelos\llm\gemma-4-E4B-it-Q4_K_M.gguf").exists() {
                    r"modelos\llm\gemma-4-E4B-it-Q4_K_M.gguf".to_string()
                } else {
                    cfg.modelo.clone()
                };

                let mut cmd = Command::new(&server_path);
                cmd.arg("-m").arg(&model_path);
                let mmproj_path = if !cfg.mmproj.is_empty() && std::path::Path::new(&cfg.mmproj).exists() {
                    Some(cfg.mmproj.clone())
                } else if std::path::Path::new(r"modelos\mmproj\gemma-4-E4B-mmproj-F16.gguf").exists() {
                    Some(r"modelos\mmproj\gemma-4-E4B-mmproj-F16.gguf".to_string())
                } else {
                    None
                };
                if let Some(ref mm) = mmproj_path {
                    cmd.arg("--mmproj").arg(mm);
                }
                cmd.args(&cfg.args).arg("--port").arg(self.port.to_string());
                let log = std::fs::File::create(self.log_path()).ok();
                let err = log.as_ref().and_then(|f| f.try_clone().ok()).map(Stdio::from).unwrap_or_else(Stdio::null);
                cmd.stdin(Stdio::null()).stdout(log.map(Stdio::from).unwrap_or_else(Stdio::null)).stderr(err);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }
                let child = cmd.spawn().map_err(|e| crate::lang::tr(format!("no se pudo arrancar {}: {e}", cfg.servidor), format!("could not start {}: {e}", cfg.servidor)))?;
                bind_to_app(&child);
                let mut args = vec!["-m".to_string(), cfg.modelo.clone()];
                args.extend(cfg.args.iter().cloned());
                *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(Launch { args, pid: child.id(), at: crate::clock::unix_now() });
                *c = Some(child);
            }
        }
        let end = Instant::now() + wait;
        while Instant::now() < end {
            if self.healthy() {
                return Ok(());
            }
            let exited = self.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|c| c.try_wait().ok().flatten());
            if let Some(code) = exited {
                return Err(crate::lang::tr(format!("llama-server terminó al arrancar ({code}); ¿cabe el modelo en la VRAM?"), format!("llama-server exited on startup ({code}); does the model fit in VRAM?")));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(crate::lang::tr("el modelo no respondió a tiempo", "the model did not respond in time"))
    }

    /// Stops the llama-server this app started (one started by hand is left alone).
    pub fn stop(&self) {
        if let Some(mut c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Motor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn context_and_gpu_layers_are_edited_in_place() {
        let a = v(&["-ngl", "99", "-c", "8192", "-fa", "on", "--jinja"]);
        assert_eq!(ctx_of(&a), Some(8192));
        assert_eq!(with_ctx(&a, 4096), v(&["-ngl", "99", "-c", "4096", "-fa", "on", "--jinja"]));
        assert_eq!(with_ctx(&v(&["--jinja"]), 2048), v(&["--jinja", "-c", "2048"]));
        assert!(!gpu_auto(&a));
        let auto = with_gpu_auto(&a, true);
        assert_eq!(auto, v(&["-c", "8192", "-fa", "on", "--jinja", "-fitt", "512"]));
        assert!(gpu_auto(&auto));
        assert_eq!(with_gpu_auto(&auto, false), v(&["-ngl", "99", "-c", "8192", "-fa", "on", "--jinja"]), "and back");
        assert_eq!(with_gpu_auto(&with_gpu_auto(&a, true), true), auto, "idempotent");
    }
}
