//! «Accesorios» in the app: the add-ons that give her a body elsewhere (the Minecraft mod for
//! now). The catalog lives in the accessories repository on GitHub (`catalog.json`); a private
//! repository needs a read token in `local.json`, and the sibling `Kaltsit-Accesories` folder
//! (a clone next to the program) is used when GitHub cannot be reached. A mod goes into the
//! `mods` folder of the Minecraft instances found on this computer (CurseForge, the official
//! launcher, Modrinth, Prism) or of a folder added by hand. Progress arrives as `k:accesorio`.

use crate::life::Core;
use crate::local::Accesorios;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;

/// Accessories are small (a mod is ~100 KB): anything bigger is not one.
const MAX_BYTES: u64 = 64 << 20;

#[derive(Clone, Debug, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub version: String,
    pub modules: Vec<Module>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Module {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub target: String,
    /// Folder of the repository the jar is in.
    pub folder: String,
    pub jar: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub sha256: String,
    /// Minecraft version the mod is built for ("26.2").
    #[serde(default)]
    pub minecraft: String,
    /// Mods it needs next to it (only "fabric-api" is known).
    #[serde(default)]
    pub requires: Vec<String>,
}

impl Module {
    /// The jar's name without its version: every file starting with it is this mod.
    fn stem(&self) -> String {
        stem(&self.jar)
    }
}

/// "kaltsit-companion-standalone-1.0.0.jar" → "kaltsit-companion-standalone".
fn stem(jar: &str) -> String {
    let base = jar.trim_end_matches(".disabled").trim_end_matches(".jar");
    let parts: Vec<&str> = base.split('-').collect();
    let cut = parts.iter().position(|p| p.chars().next().is_some_and(|c| c.is_ascii_digit())).unwrap_or(parts.len());
    parts[..cut.max(1)].join("-")
}

fn emit(core: &Core, v: Value) {
    let _ = core.app.emit("k:accesorio", v);
}

fn cache_dir() -> PathBuf {
    PathBuf::from("modelos").join("accesorios")
}

/// A clone of the accessories repository next to the program, if there is one.
fn sibling() -> Option<PathBuf> {
    let here = std::env::current_dir().ok()?;
    let p = here.parent()?.join("Kaltsit-Accesories");
    p.join("catalog.json").is_file().then_some(p)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(30)).build()
}

/// A file of the repository, through the contents API (it serves public and private ones).
fn from_github(cfg: &Accesorios, path: &str) -> Result<Vec<u8>, String> {
    if cfg.repo.trim().is_empty() {
        return Err(crate::lang::tr("sin repositorio configurado", "no repository configured"));
    }
    let url = format!("https://api.github.com/repos/{}/contents/{}?ref={}", cfg.repo.trim(), path, cfg.rama.trim());
    let mut req = agent().get(&url).set("User-Agent", "kaltsit").set("Accept", "application/vnd.github.raw");
    if !cfg.token.trim().is_empty() {
        req = req.set("Authorization", &format!("Bearer {}", cfg.token.trim()));
    }
    let resp = req.call().map_err(|e| match e {
        ureq::Error::Status(404, _) if cfg.token.trim().is_empty() => crate::lang::tr(format!("{} no se encuentra en GitHub (si el repositorio es privado, añade un token de lectura)", cfg.repo), format!("{} was not found on GitHub (if the repository is private, add a read token)", cfg.repo)),
        ureq::Error::Status(401 | 403, _) => crate::lang::tr("GitHub rechazó el token (o se agotó el límite de consultas)", "GitHub rejected the token (or the request limit ran out)"),
        other => format!("GitHub: {other}"),
    })?;
    let mut buf = Vec::new();
    resp.into_reader().take(MAX_BYTES + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_BYTES {
        return Err(crate::lang::tr(format!("{path} es demasiado grande para un accesorio"), format!("{path} is too big for an accessory")));
    }
    Ok(buf)
}

/// A file of the repository: GitHub first, the sibling clone if GitHub fails. Says where from.
fn fetch(cfg: &Accesorios, path: &str) -> Result<(Vec<u8>, &'static str), String> {
    match from_github(cfg, path) {
        Ok(b) => Ok((b, "github")),
        Err(e) => match sibling().map(|d| d.join(path)) {
            Some(p) if p.is_file() => std::fs::read(&p).map(|b| (b, "local")).map_err(|e| e.to_string()),
            _ => Err(e),
        },
    }
}

/// The catalog: fresh from the repository, else the last one downloaded.
fn catalog(cfg: &Accesorios) -> (Option<Catalog>, String, Option<String>) {
    let cached = cache_dir().join("catalog.json");
    match fetch(cfg, "catalog.json") {
        Ok((bytes, from)) => match serde_json::from_slice::<Catalog>(&bytes) {
            Ok(c) => {
                let _ = std::fs::create_dir_all(cache_dir());
                let _ = std::fs::write(&cached, &bytes);
                (Some(c), from.to_string(), None)
            }
            Err(e) => (None, from.to_string(), Some(crate::lang::tr(format!("catalog.json no es válido: {e}"), format!("catalog.json is not valid: {e}")))),
        },
        Err(e) => {
            let old = std::fs::read(&cached).ok().and_then(|b| serde_json::from_slice::<Catalog>(&b).ok());
            (old, "copia guardada".into(), Some(e))
        }
    }
}

// ── Minecraft on this computer ──

#[derive(Clone, Debug)]
struct Instance {
    name: String,
    launcher: &'static str,
    mods: PathBuf,
    version: String,
    loader: String,
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn json_file(p: &Path) -> Value {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
}

/// The Minecraft instances found here and the folders added by hand.
fn instances(cfg: &Accesorios) -> Vec<Instance> {
    let mut out = Vec::new();
    if let Some(home) = env_dir("USERPROFILE") {
        for d in subdirs(&home.join(r"curseforge\minecraft\Instances")) {
            let j = json_file(&d.join("minecraftinstance.json"));
            let loader = j["baseModLoader"]["name"].as_str().unwrap_or_default().split('-').next().unwrap_or_default().to_string();
            out.push(Instance { name: d.file_name().unwrap_or_default().to_string_lossy().into(), launcher: "CurseForge", mods: d.join("mods"), version: j["gameVersion"].as_str().unwrap_or_default().into(), loader });
        }
    }
    if let Some(appdata) = env_dir("APPDATA") {
        let official = appdata.join(".minecraft");
        if official.is_dir() {
            let fabric = std::fs::read_dir(official.join("versions")).map(|r| r.flatten().any(|e| e.file_name().to_string_lossy().starts_with("fabric-loader"))).unwrap_or(false);
            out.push(Instance { name: ".minecraft".into(), launcher: "Launcher oficial", mods: official.join("mods"), version: String::new(), loader: if fabric { "fabric".into() } else { String::new() } });
        }
        for d in subdirs(&appdata.join(r"ModrinthApp\profiles")) {
            let j = json_file(&d.join("profile.json"));
            out.push(Instance {
                name: d.file_name().unwrap_or_default().to_string_lossy().into(),
                launcher: "Modrinth",
                mods: d.join("mods"),
                version: j["game_version"].as_str().unwrap_or_default().into(),
                loader: j["loader"].as_str().unwrap_or_default().into(),
            });
        }
        for d in subdirs(&appdata.join(r"PrismLauncher\instances")) {
            let mc = if d.join(".minecraft").is_dir() { d.join(".minecraft") } else { d.join("minecraft") };
            if mc.is_dir() {
                out.push(Instance { name: d.file_name().unwrap_or_default().to_string_lossy().into(), launcher: "Prism", mods: mc.join("mods"), version: String::new(), loader: String::new() });
            }
        }
    }
    for c in &cfg.carpetas {
        let p = PathBuf::from(c);
        if !out.iter().any(|i| same_dir(&i.mods, &p)) {
            out.push(Instance { name: p.parent().and_then(|q| q.file_name()).unwrap_or_default().to_string_lossy().into(), launcher: "Carpeta propia", mods: p, version: String::new(), loader: String::new() });
        }
    }
    out
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Jars in a mods folder whose name starts with `stem` (also the ones a launcher disabled).
fn matching(mods: &Path, stem: &str) -> Vec<PathBuf> {
    std::fs::read_dir(mods)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let n = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                    (n.ends_with(".jar") || n.ends_with(".jar.disabled")) && self::stem(&n) == stem.to_lowercase()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn version_of(file: &Path, stem: &str) -> String {
    let n = file.file_name().unwrap_or_default().to_string_lossy();
    n.trim_end_matches(".disabled").trim_end_matches(".jar").get(stem.len()..).unwrap_or_default().trim_start_matches('-').to_string()
}

/// The panel: the catalog, each Minecraft instance and what of it is installed where.
pub fn state(core: &Core) -> Value {
    let cfg = core.local.lock().unwrap_or_else(|e| e.into_inner()).accesorios.clone();
    let (cat, from, error) = catalog(&cfg);
    let found = instances(&cfg);
    let modules: Vec<Value> = cat
        .as_ref()
        .map(|c| {
            c.modules
                .iter()
                .map(|m| {
                    let st = m.stem();
                    let per: Vec<Value> = found
                        .iter()
                        .map(|i| {
                            let files = matching(&i.mods, &st);
                            let installed = files.iter().find(|f| !f.to_string_lossy().ends_with(".disabled")).or(files.first()).map(|f| version_of(f, &st));
                            let fabric_api = !matching(&i.mods, "fabric-api").is_empty();
                            json!({
                                "mods": i.mods.display().to_string(),
                                "instalada": installed,
                                "desactivada": files.iter().all(|f| f.to_string_lossy().ends_with(".disabled")) && !files.is_empty(),
                                "falta": m.requires.iter().filter(|r| r.as_str() == "fabric-api" && !fabric_api).collect::<Vec<_>>(),
                                "version_ok": m.minecraft.is_empty() || i.version.is_empty() || i.version == m.minecraft,
                                "loader_ok": i.loader.is_empty() || i.loader.eq_ignore_ascii_case("fabric"),
                            })
                        })
                        .collect();
                    json!({
                        "id": m.id, "nombre": m.name, "version": m.version, "categoria": m.category,
                        "objetivo": m.target, "descripcion": m.description, "jar": m.jar, "minecraft": m.minecraft,
                        "requiere": m.requires, "instancias": per,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "origen": from,
        "error": error,
        "catalogo": cat.as_ref().map(|c| c.version.clone()),
        "repo": cfg.repo,
        "rama": cfg.rama,
        "token": !cfg.token.trim().is_empty(),
        "local": sibling().map(|p| p.display().to_string()),
        "instancias": found.iter().map(|i| json!({
            "nombre": i.name, "launcher": i.launcher, "mods": i.mods.display().to_string(),
            "version": i.version, "loader": i.loader, "existe": i.mods.is_dir(),
            "propia": cfg.carpetas.iter().any(|c| same_dir(Path::new(c), &i.mods)),
        })).collect::<Vec<_>>(),
        "modulos": modules,
    })
}

fn sha_hex(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}

/// Writes `bytes` as `name` in `mods`, after taking out the other versions of the same mod.
/// Through a temporary file, so a failure (Minecraft open) leaves the old one in place.
fn put(mods: &Path, name: &str, stem: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(mods).map_err(|e| format!("{}: {e}", mods.display()))?;
    let tmp = mods.join(format!(".{name}.kaltsit-tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    for old in matching(mods, stem) {
        if old.file_name().is_some_and(|n| n == name) {
            continue;
        }
        if let Err(e) = std::fs::remove_file(&old) {
            let _ = std::fs::remove_file(&tmp);
            return Err(crate::lang::tr(format!("no se pudo quitar {} (¿Minecraft está abierto?): {e}", old.display()), format!("could not remove {} (is Minecraft open?): {e}", old.display())));
        }
    }
    let dest = mods.join(name);
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        crate::lang::tr(format!("no se pudo escribir {} (¿Minecraft está abierto?): {e}", dest.display()), format!("could not write {} (is Minecraft open?): {e}", dest.display()))
    })
}

/// Downloads module `id` and puts it in `mods`, in the background.
pub fn install(core: Arc<Core>, id: String, mods: String) -> Result<(), String> {
    let cfg = core.local.lock().unwrap_or_else(|e| e.into_inner()).accesorios.clone();
    let (cat, _, err) = catalog(&cfg);
    let m = cat.and_then(|c| c.modules.into_iter().find(|m| m.id == id)).ok_or_else(|| err.unwrap_or_else(|| crate::lang::tr(format!("no hay ningún «{id}» en el catálogo"), format!("there is no «{id}» in the catalog"))))?;
    let mods = PathBuf::from(mods);
    std::thread::spawn(move || {
        emit(&core, json!({ "id": m.id, "mods": mods.display().to_string(), "fase": "descargando" }));
        let result = (|| -> Result<&'static str, String> {
            let path = format!("{}/{}", m.folder.trim_matches('/'), m.jar);
            let (bytes, from) = fetch(&cfg, &path)?;
            if !m.sha256.is_empty() && !sha_hex(&bytes).eq_ignore_ascii_case(&m.sha256) {
                return Err(crate::lang::tr(format!("{}: la suma sha256 no coincide con el catálogo", m.jar), format!("{}: the sha256 sum does not match the catalog", m.jar)));
            }
            if !bytes.starts_with(b"PK") {
                return Err(crate::lang::tr(format!("{} no es un .jar", m.jar), format!("{} is not a .jar", m.jar)));
            }
            let _ = std::fs::create_dir_all(cache_dir());
            let _ = std::fs::write(cache_dir().join(&m.jar), &bytes);
            emit(&core, json!({ "id": m.id, "mods": mods.display().to_string(), "fase": "instalando" }));
            put(&mods, &m.jar, &m.stem(), &bytes)?;
            Ok(from)
        })();
        match result {
            Ok(from) => emit(&core, json!({ "id": m.id, "mods": mods.display().to_string(), "fase": "listo", "origen": from })),
            Err(e) => emit(&core, json!({ "id": m.id, "mods": mods.display().to_string(), "fase": "error", "error": e })),
        }
    });
    Ok(())
}

/// Takes module `id` out of `mods` (every version of it).
pub fn uninstall(core: &Core, id: &str, mods: &str) -> Result<(), String> {
    let cfg = core.local.lock().unwrap_or_else(|e| e.into_inner()).accesorios.clone();
    let (cat, _, err) = catalog(&cfg);
    let m = cat.and_then(|c| c.modules.into_iter().find(|m| m.id == id)).ok_or_else(|| err.unwrap_or_else(|| crate::lang::tr(format!("no hay ningún «{id}» en el catálogo"), format!("there is no «{id}» in the catalog"))))?;
    for f in matching(Path::new(mods), &m.stem()) {
        std::fs::remove_file(&f).map_err(|e| crate::lang::tr(format!("no se pudo quitar {} (¿Minecraft está abierto?): {e}", f.display()), format!("could not remove {} (is Minecraft open?): {e}", f.display())))?;
    }
    Ok(())
}

/// Fabric API for `version` from Modrinth, into `mods` (in the background, as `fabric-api`).
pub fn install_fabric_api(core: Arc<Core>, mods: String, version: String) -> Result<(), String> {
    if version.trim().is_empty() {
        return Err(crate::lang::tr("no se sabe la versión de Minecraft de esa instancia", "the Minecraft version of that instance is unknown"));
    }
    let mods = PathBuf::from(mods);
    std::thread::spawn(move || {
        let tag = json!({ "id": "fabric-api", "mods": mods.display().to_string() });
        let with = |fase: &str, extra: Value| {
            let mut v = tag.clone();
            v["fase"] = json!(fase);
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            v
        };
        emit(&core, with("descargando", json!({})));
        let result = (|| -> Result<(), String> {
            let url = format!("https://api.modrinth.com/v2/project/fabric-api/version?loaders=%5B%22fabric%22%5D&game_versions=%5B%22{}%22%5D", version.trim());
            let list: Value = agent().get(&url).set("User-Agent", "kaltsit").call().map_err(|e| format!("Modrinth: {e}"))?.into_json().map_err(|e| e.to_string())?;
            let file = list.as_array().and_then(|a| a.first()).and_then(|v| v["files"].as_array()).and_then(|f| f.iter().find(|x| x["primary"] == true).or(f.first())).ok_or_else(|| crate::lang::tr(format!("Modrinth no tiene Fabric API para Minecraft {version}"), format!("Modrinth has no Fabric API for Minecraft {version}")))?;
            let (url, name) = (file["url"].as_str().unwrap_or_default(), file["filename"].as_str().unwrap_or_default());
            if !url.starts_with("https://") || !name.ends_with(".jar") || name.contains(['/', '\\']) {
                return Err(crate::lang::tr("respuesta inesperada de Modrinth", "unexpected response from Modrinth"));
            }
            let mut bytes = Vec::new();
            agent().get(url).set("User-Agent", "kaltsit").call().map_err(|e| format!("{url}: {e}"))?.into_reader().take(MAX_BYTES).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if let Some(want) = file["hashes"]["sha512"].as_str() {
                use sha2::Sha512;
                if !format!("{:x}", Sha512::digest(&bytes)).eq_ignore_ascii_case(want) {
                    return Err(crate::lang::tr("la suma sha512 de Fabric API no coincide", "the sha512 sum of Fabric API does not match"));
                }
            }
            emit(&core, with("instalando", json!({})));
            put(&mods, name, "fabric-api", &bytes)
        })();
        match result {
            Ok(()) => emit(&core, with("listo", json!({ "origen": "modrinth" }))),
            Err(e) => emit(&core, with("error", json!({ "error": e }))),
        }
    });
    Ok(())
}

/// Repository, branch, token (None keeps it, "" removes it) and the hand-added mods folders.
pub fn configure(core: &Core, repo: Option<String>, rama: Option<String>, token: Option<String>, add: Option<String>, remove: Option<String>) -> Result<(), String> {
    let mut l = core.local.lock().unwrap_or_else(|e| e.into_inner());
    let a = &mut l.accesorios;
    if let Some(r) = repo {
        let r = r.trim().trim_start_matches("https://github.com/").trim_end_matches('/').trim_end_matches(".git").to_string();
        if r.split('/').count() != 2 || r.split('/').any(|p| p.is_empty()) {
            return Err(crate::lang::tr("el repositorio va como usuario/nombre", "the repository goes as user/name"));
        }
        a.repo = r;
    }
    if let Some(b) = rama {
        a.rama = if b.trim().is_empty() { "main".into() } else { b.trim().into() };
    }
    if let Some(t) = token {
        a.token = t.trim().to_string();
    }
    if let Some(p) = add {
        let p = p.trim().trim_matches('"').to_string();
        let path = Path::new(&p);
        if !path.is_dir() {
            return Err(crate::lang::tr(format!("no existe la carpeta {p}"), format!("the folder {p} does not exist")));
        }
        // Pointing at the instance instead of its mods folder is the usual mistake.
        let p = if path.file_name().is_some_and(|n| n != "mods") && path.join("mods").is_dir() { path.join("mods").display().to_string() } else { p };
        if !a.carpetas.iter().any(|c| same_dir(Path::new(c), Path::new(&p))) {
            a.carpetas.push(p);
        }
    }
    if let Some(p) = remove {
        a.carpetas.retain(|c| !same_dir(Path::new(c), Path::new(&p)) && c != &p);
    }
    l.save(&core.data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jar_is_known_by_its_name_without_version() {
        assert_eq!(stem("kaltsit-companion-standalone-1.0.0.jar"), "kaltsit-companion-standalone");
        assert_eq!(stem("kaltsit-companion-standalone-1.2.0.jar.disabled"), "kaltsit-companion-standalone");
        assert_eq!(stem("fabric-api-0.161.0+26.2.jar"), "fabric-api");
        assert_eq!(stem("modelexporter-1.0.1.jar"), "modelexporter");
    }

    /// Network (and the sibling clone): `cargo test -- --ignored accessories`.
    #[test]
    #[ignore]
    fn the_catalog_and_its_jar_arrive_and_match() {
        let cfg = Accesorios::default();
        let (cat, from, err) = catalog(&cfg);
        let cat = cat.unwrap_or_else(|| panic!("no catalog ({from}): {err:?}"));
        let m = cat.modules.iter().find(|m| m.id == "minecraft-companion").expect("the Minecraft mod");
        let (bytes, from) = fetch(&cfg, &format!("{}/{}", m.folder, m.jar)).unwrap();
        assert!(bytes.starts_with(b"PK"), "a jar");
        assert!(m.sha256.is_empty() || sha_hex(&bytes) == m.sha256, "checksum from {from}");
        let found = instances(&cfg);
        eprintln!("catalog from {from}; instances: {:?}", found.iter().map(|i| (&i.name, i.launcher, &i.version, &i.loader)).collect::<Vec<_>>());
    }

    #[test]
    fn installing_replaces_the_other_versions_only() {
        let d = std::env::temp_dir().join(format!("kaltsit-acc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("kaltsit-companion-standalone-0.9.0.jar"), b"PKold").unwrap();
        std::fs::write(d.join("fabric-api-0.161.0+26.2.jar"), b"PK").unwrap();
        put(&d, "kaltsit-companion-standalone-1.0.0.jar", "kaltsit-companion-standalone", b"PKnew").unwrap();
        let found = matching(&d, "kaltsit-companion-standalone");
        assert_eq!(found.len(), 1);
        assert_eq!(version_of(&found[0], "kaltsit-companion-standalone"), "1.0.0");
        assert!(d.join("fabric-api-0.161.0+26.2.jar").exists(), "other mods stay");
        let _ = std::fs::remove_dir_all(&d);
    }
}
