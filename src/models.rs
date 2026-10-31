//! «Modelos» in the app: the catalog as it fits this computer (or the other profile), downloads
//! reporting their progress as `k:descarga` events, and choosing the model her brain runs.

use crate::catalog::{self, FileSpec, Fit, Hardware, Item, Manifest, Profile};
use crate::download;
use crate::life::{Core, Role};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use tauri::Emitter;

/// Asked once: nvidia-smi and the registry take a moment and the hardware does not change.
fn hardware() -> &'static Hardware {
    static HW: OnceLock<Hardware> = OnceLock::new();
    HW.get_or_init(catalog::hardware)
}

/// Items downloading right now (each one only once at a time).
fn downloading() -> MutexGuard<'static, HashSet<String>> {
    static D: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    D.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn here() -> Profile {
    Profile::from_hardware(hardware())
}

fn same_file(a: &str, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// The panel: this computer, the profile shown (this one unless another is asked for) and every
/// item with how it fits, whether it is downloaded, downloading or in use.
pub fn state(core: &Core, profile: Option<String>) -> Value {
    let m = Manifest::load();
    let root = m.root();
    let hw = hardware();
    let mine = here();
    let p: &Profile = profile
        .as_deref()
        .filter(|&id| id != catalog::HERE)
        .and_then(|id| m.profile(id))
        .unwrap_or(&mine);
    let cfg = core.motor.cfg();
    let busy = downloading().clone();
    let voice = core.voice_json();
    let voice_id = voice["id"].as_str().map(String::from);
    let pick = m.recommend_one(p);
    let safest = pick.as_ref().map(|k| k.item.id.as_str());
    let mine_pick = m.recommend_one(&mine);
    let mine_safest = mine_pick.as_ref().map(|k| k.item.id.as_str());
    let items: Vec<Value> = m
        .elementos
        .iter()
        .map(|i| {
            let brain = i.tipo == "cerebro";
            let vram = if brain { i.vram_gb() } else { 0.0 };
            let in_use = i.file("modelo").is_some_and(|f| brain && same_file(&cfg.modelo, &root.join(&f.destino)))
                || i.servidor.as_deref().is_some_and(|s| same_file(&cfg.servidor, &root.join(s)) || (s.ends_with(".dll") && voice["lista"] == true && i.installed(&root)))
                || voice_id.as_deref() == Some(i.id.as_str());
            json!({
                "id": i.id, "tipo": i.tipo, "nivel": i.nivel, "nombre": i.nombre, "nota": i.nota,
                "descarga": i.download_bytes(),
                "vram": vram,
                "cabe": if brain { catalog::fit(vram, p).name() } else { "" },
                // On the computer the app runs on, whatever profile is being looked at.
                "cabe_aqui": !brain || catalog::fit(vram, &mine) != Fit::NoCabe || mine_safest == Some(i.id.as_str()),
                "instalado": i.installed(&root),
                "en_uso": in_use,
                "descargando": busy.contains(&i.id),
                "recomendado": safest == Some(i.id.as_str()),
            })
        })
        .collect();
    let packs: Vec<Value> = m
        .packs_for(&p.id)
        .into_iter()
        .map(|k| {
            let bytes: u64 = k.incluye.iter().filter_map(|id| m.item(id)).map(|i| i.download_bytes()).sum();
            json!({ "id": k.id, "nombre": k.nombre, "incluye": k.incluye, "descarga": bytes })
        })
        .collect();
    let rec = mine_pick.as_ref().map(|k| {
        let has_engine = !crate::motor::engines(&cfg.servidor).is_empty();
        let engine_item = m.item("llama-b11633-cuda12");
        let need_engine = !has_engine && engine_item.is_some_and(|e| !e.installed(&root));
        let mut ids = vec![k.item.id.clone()];
        let mut bytes = k.item.download_bytes();
        if need_engine {
            if let Some(e) = engine_item {
                ids.push(e.id.clone());
                bytes += e.download_bytes();
            }
        }
        let in_use = k.item.file("modelo").is_some_and(|f| same_file(&cfg.modelo, &root.join(&f.destino)));
        json!({
            "id": k.item.id,
            "nombre": k.item.nombre,
            "gb": k.gb,
            "cabe": k.fit.name(),
            "lento": k.slow,
            "motivo": k.reason,
            "instalado": k.item.installed(&root),
            "en_uso": in_use,
            "descarga": bytes,
            "incluye_motor": need_engine,
            "ids": ids,
        })
    });
    let profiles: Vec<Value> = std::iter::once(&mine)
        .chain(m.perfiles.iter())
        .map(|q| json!({ "id": q.id, "nombre": q.nombre, "nota": q.nota, "presupuesto": q.budget_gb(), "cpu_only": q.cpu_only }))
        .collect();
    json!({
        "equipo": { "gpu": hw.gpu, "vendor": hw.vendor, "vram": hw.vram_gb, "ram": hw.ram_gb, "cpu": hw.cpu, "cpu_only": mine.cpu_only, "nombre": mine.nombre, "presupuesto": mine.budget_gb(), "perfil": mine.id },
        "perfiles": profiles,
        "perfil": p.id,
        "recomendacion": rec,
        "elementos": items,
        "packs": packs,
        "carpeta": root.display().to_string(),
        "actual": cfg.modelo,
        "cerebro": core.role() == Role::Brain,
        "voz": voice,
    })
}

/// Downloads `ids` one after the other in the background; the panel follows `k:descarga`.
pub fn download(core: Arc<Core>, ids: Vec<String>) -> Result<(), String> {
    let m = Manifest::load();
    for id in &ids {
        m.item(id).ok_or_else(|| crate::lang::tr(format!("no hay ningún «{id}» en el catálogo"), format!("there is no «{id}» in the catalog")))?;
    }
    let ids: Vec<String> = {
        let mut busy = downloading();
        ids.into_iter().filter(|id| busy.insert(id.clone())).collect()
    };
    std::thread::spawn(move || {
        let root = m.root();
        let emit = |v: Value| {
            let _ = core.app.emit("k:descarga", v);
        };
        for (n, id) in ids.iter().enumerate() {
            let item = m.item(id).expect("checked above");
            let result = (|| -> Result<(), String> {
                for (k, f) in item.archivos.iter().enumerate() {
                    let dest = root.join(&f.destino);
                    // A file of the user's: there is nothing to download it from.
                    if f.url.is_empty() {
                        if !std::fs::metadata(&dest).is_ok_and(|m| m.len() == f.bytes) {
                            return Err(crate::lang::tr(format!("{} ya no está (o cambió): quítalo de «Mis modelos» y añádelo de nuevo", dest.display()), format!("{} is gone (or changed): remove it from «My models» and add it again", dest.display())));
                        }
                        continue;
                    }
                    let (mut last, mut prev, mut hashing) = (Instant::now() - Duration::from_secs(1), 0u64, false);
                    let fresh = download::fetch(&f.url, &dest, f.bytes, &f.sha256, &mut |done, total| {
                        // After the download the sha256 check reads the file again from 0.
                        hashing |= done < prev;
                        prev = done;
                        if last.elapsed() < Duration::from_millis(250) && done < total {
                            return;
                        }
                        last = Instant::now();
                        emit(json!({
                            "id": id, "fase": if hashing { "verificando" } else { "descargando" },
                            "archivo": f.destino, "n": k + 1, "de": item.archivos.len(), "hecho": done, "total": total,
                            "cola": ids.len() - n - 1,
                        }));
                    })?;
                    if let Some(to) = &f.convertir {
                        if fresh || !root.join(to).exists() {
                            download::ogg_to_wav(&dest, &root.join(to), 10.0)?;
                        }
                    }
                    if let Some(to) = &f.extraer {
                        if fresh || !item.unpacked(&root) {
                            emit(json!({ "id": id, "fase": "descomprimiendo", "archivo": f.destino }));
                            download::unzip(&dest, &root.join(to))?;
                        }
                    }
                }
                Ok(())
            })();
            downloading().remove(id);
            match result {
                Ok(()) => emit(json!({ "id": id, "fase": "listo" })),
                Err(e) => {
                    emit(json!({ "id": id, "fase": "error", "error": e }));
                    // The rest of the queue would likely fail the same way (no connection).
                    for rest in &ids[n + 1..] {
                        downloading().remove(rest);
                        emit(json!({ "id": rest, "fase": "cancelado" }));
                    }
                    return;
                }
            }
        }
    });
    Ok(())
}

/// Her brain runs `id` from now on: `local.json` points at it and, if this computer is
/// thinking now, the model is swapped right away.
pub fn use_model(core: &Arc<Core>, id: &str) -> Result<(), String> {
    let m = Manifest::load();
    let item = m.item(id).ok_or_else(|| crate::lang::tr(format!("no hay ningún «{id}» en el catálogo"), format!("there is no «{id}» in the catalog")))?;
    let mine = here();
    let mine_rec = m.recommend_one(&mine).map(|k| k.item.id.as_str());
    if catalog::fit(item.vram_gb(), &mine) == Fit::NoCabe && mine_rec != Some(item.id.as_str()) {
        return Err(crate::lang::tr(format!("{} no cabe en la VRAM de este equipo", item.nombre), format!("{} does not fit in this computer's VRAM", item.nombre)));
    }
    let previous = core.local.lock().unwrap_or_else(|e| e.into_inner()).clone();
    catalog::use_item(&m, id, &core.data)?;
    core.reload_model(previous);
    Ok(())
}

// ── her brain on a model of the user's ──

/// A Hugging Face page or download link as the direct download link.
fn hf_link(url: &str) -> String {
    url.trim().split(['?', '#']).next().unwrap_or_default().replace("/blob/", "/resolve/")
}

/// Size and sha256 of a file to download. Hugging Face answers the first request with a
/// redirect that carries both (`x-linked-size`, `x-linked-etag`); other servers, the length.
fn probe(url: &str) -> Result<(u64, String), String> {
    let agent = ureq::AgentBuilder::new().redirects(0).timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(30)).build();
    let r = agent.head(url).set("User-Agent", "kaltsit").call().map_err(|e| match e {
        ureq::Error::Status(401 | 403, _) => crate::lang::tr(format!("{url}: el modelo pide aceptar su licencia o iniciar sesión en Hugging Face; descárgalo a mano y añade el archivo"), format!("{url}: the model asks you to accept its license or sign in to Hugging Face; download it by hand and add the file")),
        ureq::Error::Status(404, _) => crate::lang::tr(format!("{url}: no existe"), format!("{url}: does not exist")),
        other => format!("{url}: {other}"),
    })?;
    let num = |h: &str| r.header(h).and_then(|v| v.parse::<u64>().ok());
    let sha = r.header("x-linked-etag").or(r.header("etag")).unwrap_or_default().trim_start_matches("W/").trim_matches('"').to_string();
    let sha = if sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()) { sha } else { String::new() };
    if let Some(n) = num("x-linked-size").or_else(|| if r.status() == 200 { num("content-length") } else { None }) {
        return Ok((n, sha));
    }
    // Another kind of redirect: follow it for the length.
    let r = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).build().head(url).set("User-Agent", "kaltsit").call().map_err(|e| format!("{url}: {e}"))?;
    let n = r.header("content-length").and_then(|v| v.parse::<u64>().ok()).filter(|n| *n > 0).ok_or_else(|| crate::lang::tr(format!("{url}: el servidor no dice cuánto ocupa"), format!("{url}: the server does not say how big it is")))?;
    Ok((n, sha))
}

fn file_name(src: &str) -> String {
    src.trim().trim_matches('"').rsplit(['/', '\\']).next().unwrap_or_default().to_string()
}

/// One file of a model of the user's: a link (downloaded into the models folder like the
/// catalog's) or a `.gguf` already on this computer (used where it is).
fn own_file(src: &str, rol: &str, id: &str) -> Result<FileSpec, String> {
    let src = src.trim().trim_matches('"');
    let name = file_name(src);
    if !name.to_lowercase().ends_with(".gguf") {
        return Err(crate::lang::tr(format!("{name}: tiene que ser un archivo .gguf"), format!("{name}: must be a .gguf file")));
    }
    if src.starts_with("https://") || src.starts_with("http://") {
        let url = hf_link(src);
        let (bytes, sha256) = probe(&url)?;
        // Every repository calls its projector mmproj-F16.gguf: the id keeps them apart.
        let destino = if rol == "mmproj" { format!("mmproj/{id}-{name}") } else { format!("llm/{name}") };
        return Ok(FileSpec { url, destino, bytes, sha256, rol: rol.into(), extraer: None, convertir: None });
    }
    let p = Path::new(src);
    let meta = std::fs::metadata(p).map_err(|e| format!("{src}: {e}"))?;
    if !meta.is_file() {
        return Err(crate::lang::tr(format!("{src} no es un archivo"), format!("{src} is not a file")));
    }
    let abs = std::fs::canonicalize(p).map(|a| a.display().to_string().trim_start_matches(r"\\?\").to_string()).unwrap_or_else(|_| src.to_string());
    Ok(FileSpec { url: String::new(), destino: abs, bytes: meta.len(), sha256: String::new(), rol: rol.into(), extraer: None, convertir: None })
}

/// Adds a brain model of the user's (`modelo` and, to let her see, `mmproj`: links or files).
pub fn add_own(name: &str, modelo: &str, mmproj: &str, ctx: Option<u32>) -> Result<String, String> {
    let m = Manifest::load();
    let root = m.root();
    let label = if name.trim().is_empty() { file_name(modelo).trim_end_matches(".gguf").to_string() } else { name.trim().to_string() };
    let slug: String = label.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    let base = format!("propio-{}", if slug.is_empty() { "modelo" } else { &slug });
    let mut id = base.clone();
    let mut n = 2;
    while m.item(&id).is_some() {
        id = format!("{base}-{n}");
        n += 1;
    }
    let mut files = vec![own_file(modelo, "modelo", &id)?];
    if !mmproj.trim().is_empty() {
        files.push(own_file(mmproj, "mmproj", &id)?);
    }
    let from = if files[0].url.is_empty() { "archivo de este equipo" } else { "enlace" };
    let item = Item {
        id: id.clone(),
        tipo: "cerebro".into(),
        nivel: "propio".into(),
        nombre: label,
        nota: crate::lang::tr(
            format!("Añadido por ti ({from}){}", if files.len() < 2 { " · sin mmproj: no podrá ver la pantalla" } else { "" }),
            format!("Added by you ({from}){}", if files.len() < 2 { " · no mmproj: it will not be able to see the screen" } else { "" }),
        ),
        ctx: Some(ctx.unwrap_or(8192).clamp(1024, 32768)),
        archivos: files,
        servidor: None,
        voz: None,
        oido: None,
    };
    let mut own = catalog::load_own(&root);
    own.push(item);
    catalog::save_own(&root, &own)?;
    Ok(id)
}

/// Takes a model of the user's out of the list. A file it downloaded into the models folder is
/// deleted only when `delete_files`; a file of the user's elsewhere is never touched.
pub fn remove_own(core: &Core, id: &str, delete_files: bool) -> Result<(), String> {
    let m = Manifest::load();
    let root = m.root();
    let mut own = catalog::load_own(&root);
    let Some(pos) = own.iter().position(|i| i.id == id) else { return Err(crate::lang::tr(format!("«{id}» no es un modelo tuyo"), format!("«{id}» is not one of your models"))) };
    let cfg = core.motor.cfg();
    if own[pos].file("modelo").is_some_and(|f| same_file(&cfg.modelo, &root.join(&f.destino))) {
        return Err(crate::lang::tr("es el modelo que usa ahora: elige otro antes de quitarlo", "it is the model in use now: pick another one before removing it"));
    }
    let item = own.remove(pos);
    catalog::save_own(&root, &own)?;
    if delete_files {
        for f in item.archivos.iter().filter(|f| !f.url.is_empty()) {
            let _ = std::fs::remove_file(root.join(&f.destino));
            let _ = std::fs::remove_file(format!("{}.part", root.join(&f.destino).display()));
        }
    }
    Ok(())
}

// ── Kal'tsit's Spine 3.8 chibi (downloaded on demand from Ark-Models, like Ark-Pets) ──

const ARK_MODELS_BASE: &str = "https://raw.githubusercontent.com/isHarryh/Ark-Models/main/models/003_kalts";

const CHIBI_FILES: [(&str, u64, &str); 3] = [
    ("build_char_003_kalts.atlas", 8_872, "cc245ca899d25df397a1aab19cf359c9851849dad8db3f8264f5aef725dc0a0d"),
    ("build_char_003_kalts.png", 277_909, "e0a3f9fc65139520536e9a7b4c2ee03eb481277bb3ef591cb658a95ad00bcd53"),
    ("build_char_003_kalts.skel", 159_443, "3f946202a188aadadd1adc1436f89891c545b50a210c12f3ac3e7a40f5bc4070"),
];

/// Ensures the 3 Spine 3.8 files (`build_char_003_kalts.{atlas,png,skel}`) are present in
/// `modelos/spine/003_kalts/`, downloading any missing file from `isHarryh/Ark-Models` with
/// SHA-256 verification, and returns them ready for the WebGL overlay (`atlas` text, `png`
/// data URI, `skel` base64).
pub fn chibi_model() -> Result<Value, String> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let dir = Manifest::load().root().join("spine/003_kalts");
    chibi_model_in(&dir, ARK_MODELS_BASE)
}

fn chibi_model_in(dir: &Path, base_url: &str) -> Result<Value, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    for &(name, bytes, sha256) in &CHIBI_FILES {
        let dest = dir.join(name);
        let url = format!("{}/{name}", base_url.trim_end_matches('/'));
        download::fetch(&url, &dest, bytes, sha256, &mut |_, _| {})?;
    }
    let atlas = std::fs::read_to_string(dir.join(CHIBI_FILES[0].0)).map_err(|e| e.to_string())?;
    let png = std::fs::read(dir.join(CHIBI_FILES[1].0)).map_err(|e| e.to_string())?;
    let skel = std::fs::read(dir.join(CHIBI_FILES[2].0)).map_err(|e| e.to_string())?;
    Ok(json!({
        "atlas": atlas,
        "png": format!("data:image/png;base64,{}", b64.encode(&png)),
        "skel": b64.encode(&skel),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hugging_face_page_link_becomes_its_download_link() {
        assert_eq!(hf_link("https://huggingface.co/u/r/blob/main/m.gguf?download=true"), "https://huggingface.co/u/r/resolve/main/m.gguf");
        assert_eq!(file_name(r#""C:\modelos\Mi Modelo.gguf""#), "Mi Modelo.gguf");
    }

    #[test]
    fn chibi_files_manifest_is_well_formed() {
        assert_eq!(CHIBI_FILES.len(), 3);
        for &(name, bytes, sha) in &CHIBI_FILES {
            assert!(name.starts_with("build_char_003_kalts."));
            assert!(bytes > 0);
            assert_eq!(sha.len(), 64);
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        }
        let cache = Manifest::load().root().join("spine/003_kalts");
        if CHIBI_FILES.iter().all(|&(n, _, _)| cache.join(n).is_file()) {
            let v = chibi_model_in(&cache, "http://127.0.0.1:9").expect("cached chibi_model_in");
            assert!(v["atlas"].as_str().unwrap_or_default().contains("build_char_003_kalts.png"));
            assert!(v["png"].as_str().unwrap_or_default().starts_with("data:image/png;base64,"));
            assert!(!v["skel"].as_str().unwrap_or_default().is_empty());
        }
    }

    /// Network: `cargo test -- --ignored probe`. Only asks for the headers.
    #[test]
    #[ignore]
    fn probe_reads_size_and_checksum_like_the_catalog() {
        let m = Manifest::parse(include_str!("../models_manifest.json")).unwrap();
        let f = m.item("qwen35-4b-q4").unwrap().file("modelo").unwrap();
        let (bytes, sha) = probe(&hf_link(&f.url.replace("/resolve/", "/blob/"))).unwrap();
        assert_eq!(bytes, f.bytes);
        assert_eq!(sha, f.sha256);
    }
}
