//! `kaltsit --migrar-sectores [--data <carpeta>] [--forzar] [--semilla <archivo>]`: applies the
//! curated import (`DISENO-MEMORIA-SECTORES.md` §9) to her real memory, once, by explicit order.
//! It is never automatic and it never opens a window. It builds the memory from the owner's
//! PERSONAL seed, which is not compiled in: it is read from `--semilla`, or from
//! `seed/kaltsit_sectores.ron` next to the executable or in the app folder (see
//! `seed::personal_seed`); without it, nothing is done.
//!
//! What it does, in this order, stopping at the first thing that does not hold:
//! 1. checks the app is not using the folder;
//! 2. if the memory already has sectors, it does nothing;
//! 3. copies the WHOLE folder to `<parent>/data_backup/antes-sectores-AAAAMMDD-HHMMSS/` and checks
//!    the copy byte by byte (SHA-256 of every file); if it differs, it stops before touching anything;
//! 4. builds the new memory with `import::curated`, saves it in the same folder (format 2) and writes
//!    the report next to the backup (`IMPORTACION-APLICADA.md`);
//! 5. opens the saved folder again with `Mind::open` and checks it (see `verify_opened`); if
//!    anything fails, from step 4 on, the folder is restored from the copy and checked again.

use crate::cthg::model::*;
use crate::cthg::{sectors, seed};
use crate::mind::Mind;
use crate::{clock, import, modules};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The report written inside the backup folder.
pub const REPORT_FILE: &str = "IMPORTACION-APLICADA.md";

/// File → (size, SHA-256). Directories are listed as `name/` so empty ones are compared too.
type Tree = BTreeMap<String, (u64, [u8; 32])>;

fn walk(dir: &Path, base: &Path, out: &mut Tree) -> Result<(), String> {
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        let md = std::fs::symlink_metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if md.is_dir() {
            out.insert(format!("{rel}/"), (0, [0; 32]));
            walk(&p, base, out)?;
        } else {
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            out.insert(rel, (bytes.len() as u64, Sha256::digest(&bytes).into()));
        }
    }
    Ok(())
}

/// Every file and folder under `dir` (the `.git` of the memory too), by hash.
fn tree(dir: &Path) -> Result<Tree, String> {
    let mut out = Tree::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// Copies `src` into `dst` (created if missing). Names in `skip` are left out at the top level.
fn copy_tree(src: &Path, dst: &Path, skip: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for e in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
        let from = e.map_err(|e| e.to_string())?.path();
        let name = from.file_name().unwrap_or_default().to_string_lossy().to_string();
        if skip.contains(&name.as_str()) {
            continue;
        }
        let to = dst.join(&name);
        if std::fs::symlink_metadata(&from).map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&from, &to, &[])?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Removes a file or folder, read-only files included (git packs are).
#[allow(clippy::permissions_set_readonly_false)] // Windows: the attribute is what blocks the removal
pub fn force_remove(p: &Path) -> std::io::Result<()> {
    let Ok(md) = std::fs::symlink_metadata(p) else { return Ok(()) };
    if md.is_dir() {
        for e in std::fs::read_dir(p)? {
            force_remove(&e?.path())?;
        }
        std::fs::remove_dir(p)
    } else {
        let mut perm = md.permissions();
        if perm.readonly() {
            perm.set_readonly(false);
            let _ = std::fs::set_permissions(p, perm);
        }
        std::fs::remove_file(p)
    }
}

/// Puts the folder back exactly as the copy had it (wipes what is inside, not the folder itself).
fn restore(data: &Path, backup: &Path, expected: &Tree) -> Result<(), String> {
    for e in std::fs::read_dir(data).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        force_remove(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    copy_tree(backup, data, &[REPORT_FILE])?;
    if &tree(data)? != expected {
        return Err("la carpeta restaurada no coincide con la copia".into());
    }
    Ok(())
}

/// `<parent of data>/data_backup`.
pub fn backup_root(data: &Path) -> PathBuf {
    let parent = data.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    parent.join("data_backup")
}

/// "20261025-142334" in local time.
pub fn stamp_of(now: f64) -> String {
    match clock::to_local(now) {
        Some(l) => format!("{:04}{:02}{:02}-{:02}{:02}{:02}", l.year, l.month, l.day, l.hour, l.minute, (now as u64) % 60),
        None => format!("{}", now as u64),
    }
}

/// Checks run on the folder after saving: the opened memory against the old one and the new one.
/// The lines of what was checked, or what failed.
pub type Verify<'a> = &'a dyn Fn(&Path, &Memory, &Memory) -> Result<Vec<String>, String>;

pub struct Applied {
    pub backup: PathBuf,
    pub report: PathBuf,
}

pub enum Outcome {
    /// The memory already had sectors: nothing was touched.
    Already,
    Applied(Applied),
}

fn node_count(m: &Memory, kind: NodeKind) -> usize {
    m.nodes.iter().filter(|n| n.kind == kind).count()
}

/// The migration over `data`. `stamp` names the backup folder, `verify` is the final check
/// (`verify_opened` in the app). `log` receives, in order, what is done and what was found.
/// An error means the folder is as it was (nothing touched, or restored from the copy).
pub fn migrate(data: &Path, now: f64, stamp: &str, verify: Verify, log: &mut Vec<String>) -> Result<Outcome, String> {
    if !modules::dir_of(data).is_dir() {
        return Err(format!("{} no tiene memoria (falta memoria/)", data.display()));
    }
    let loaded = modules::load(data)?.ok_or("no hay módulos de memoria en la carpeta")?;
    let old = loaded.mem;
    if sectors::has_sectors(&old) {
        log.push(format!("La memoria de {} YA tiene sectores ({} sectores): no se hace nada.", data.display(), node_count(&old, NodeKind::Sector)));
        return Ok(Outcome::Already);
    }
    // What the migration cannot carry over faithfully: refuse, before copying anything.
    if loaded.modules.len() > modules::CORE.len() {
        let extra: Vec<&str> = loaded.modules[modules::CORE.len()..].iter().map(|m| m.name.as_str()).collect();
        return Err(format!("hay módulos extra montados ({}): la migración solo conoce los cuatro de siempre. No se ha tocado nada.", extra.join(", ")));
    }
    if !loaded.pending.is_empty() {
        return Err(format!("hay {} aristas pendientes hacia módulos que no están aquí: no se pueden trasladar. No se ha tocado nada.", loaded.pending.len()));
    }
    log.push(format!("Memoria actual: {} nodos, {} aristas, {} mensajes, sin sectores.", old.nodes.len(), old.edges.len(), old.chat.len()));
    // 3. The whole folder, copied and compared.
    let root = backup_root(data);
    let backup = root.join(format!("antes-sectores-{stamp}"));
    if backup.exists() {
        return Err(format!("ya existe {}; no se sobrescribe. No se ha tocado nada.", backup.display()));
    }
    // This migration is the owner's: it builds the memory from HIS personal seed, read from disk
    // (`seed::personal_seed`). Without the file it stops here, before copying anything; a base
    // seed never stands in for it.
    let guided = seed::build_personal().map_err(|e| format!("{e} No se ha tocado nada."))?;
    let original = tree(data)?;
    let copied = copy_tree(data, &backup, &[]).and_then(|_| tree(&backup));
    match copied {
        Ok(t) if t == original => {}
        other => {
            let _ = force_remove(&backup);
            let why = match other {
                Ok(_) => "la copia no es idéntica al original".to_string(),
                Err(e) => e,
            };
            return Err(format!("copia de seguridad fallida ({why}). No se ha tocado nada; la copia incompleta se ha borrado."));
        }
    }
    log.push(format!("Copia de seguridad: {} ({} entradas, {} bytes), verificada archivo por archivo (SHA-256).", backup.display(), original.len(), original.values().map(|v| v.0).sum::<u64>()));

    // 4–5. Everything from here on can be undone from the copy.
    let applied = apply(data, &old, now, guided, &backup, verify, log);
    match applied {
        Ok(report) => Ok(Outcome::Applied(Applied { backup, report })),
        Err(e) => {
            log.push(format!("FALLO: {e}"));
            match restore(data, &backup, &original) {
                Ok(()) => Err(format!("{e}. La carpeta de datos se ha RESTAURADO desde {} y se ha comprobado que coincide con la copia.", backup.display())),
                Err(r) => Err(format!("{e}. Y la restauración automática también falló ({r}): restaura a mano la carpeta {} desde {}.", data.display(), backup.display())),
            }
        }
    }
}

fn apply(data: &Path, old: &Memory, now: f64, guided: Memory, backup: &Path, verify: Verify, log: &mut Vec<String>) -> Result<PathBuf, String> {
    let (mut new, rep) = import::curated(old, now, guided)?;
    // It was born from the owner's seed, and he has already been through the first launch.
    new.set_meta("semilla", seed::Born::Personal.meta());
    new.set_meta("primer_arranque", "hecho");
    // The persona, scene and greeting of the new seed travel in `yo`'s meta, as `Mind::save` writes them.
    if let Some(p) = new.periods.first().cloned() {
        new.set_meta("p.persona", &p.persona);
        new.set_meta("p.escena", &p.scene);
        new.set_meta("p.saludo", &p.opening);
    }
    let mods: Vec<modules::Module> = modules::CORE.iter().map(|(n, t)| modules::Module { name: n.to_string(), title: t.to_string() }).collect();
    modules::save(data, &new, &mods, &[], now)?;
    log.push(format!(
        "Memoria nueva guardada en {} (formato 2): {} nodos ({} sectores), {} aristas, {} mensajes.",
        data.display(),
        new.nodes.len(),
        node_count(&new, NodeKind::Sector),
        new.edges.len(),
        new.chat.len()
    ));
    log.push(format!(
        "Importación: {} importados, {} fusionados con la semilla, {} descartados, {} equipos; recalled del historial: {} traducidos, {} quitados.",
        rep.imported().count(),
        rep.merged().count(),
        rep.nodes.len() - rep.imported().count() - rep.merged().count(),
        rep.equipment.len(),
        rep.recalled_translated,
        rep.recalled_dropped
    ));
    let report = backup.join(REPORT_FILE);
    let mut md = rep.to_markdown_applied(now, &backup.display().to_string());
    std::fs::write(&report, &md).map_err(|e| format!("{}: {e}", report.display()))?;
    log.push(format!("Informe: {}", report.display()));

    let checks = verify(data, old, &new);
    md.push_str("\n## Comprobaciones tras guardar\n\n");
    match &checks {
        Ok(lines) => lines.iter().for_each(|l| md.push_str(&format!("- OK: {l}\n"))),
        Err(e) => md.push_str(&format!("- FALLÓ: {e}\n\nLa carpeta de datos se restauró desde esta copia.\n")),
    }
    let _ = std::fs::write(&report, &md);
    for l in checks? {
        log.push(format!("Comprobación OK: {l}"));
    }
    Ok(report)
}

/// The final check of the app: reopens the saved folder with `Mind::open` and looks at what matters.
pub fn verify_opened(data: &Path, old: &Memory, new: &Memory) -> Result<Vec<String>, String> {
    let before = tree(data)?;
    let mind = Mind::open(data, "migracion")?;
    let mut ok = vec!["la memoria guardada se abre con Mind::open".to_string()];
    if tree(data)? != before {
        return Err("abrir la memoria nueva modificó la carpeta".into());
    }
    ok.push("abrirla no escribe nada".into());

    let m = &mind.mem;
    if !sectors::has_sectors(m) {
        return Err("la memoria reabierta no tiene sectores".into());
    }
    let n_sectors = node_count(m, NodeKind::Sector);
    if n_sectors != node_count(new, NodeKind::Sector) {
        return Err(format!("sectores: {} guardados, {} al reabrir", node_count(new, NodeKind::Sector), n_sectors));
    }
    ok.push(format!("tiene sectores ({n_sectors})"));

    // The guided core: exactly the axioms of the sectored seed, with the seed's text.
    let guided = seed::build_personal()?;
    let want: BTreeMap<&str, &str> = guided.nodes.iter().filter(|n| n.is_axiom()).map(|n| (n.key.as_str(), n.text.as_str())).collect();
    let have: BTreeMap<&str, &str> = m.nodes.iter().filter(|n| n.is_axiom()).map(|n| (n.key.as_str(), n.text.as_str())).collect();
    if want != have {
        return Err(format!("el núcleo no es el guiado: esperados {:?}, hay {:?}", want.keys().collect::<Vec<_>>(), have.keys().collect::<Vec<_>>()));
    }
    ok.push(format!("el núcleo son los {} axiomas guiados de la semilla, con su texto", want.len()));
    // An old axiom coming back would be a core node outside the guided ones (equal sets above, so
    // none) or one of the old core nodes carrying the mark again by key. A key can be shared with
    // a node of the new seed that is NOT an axiom (like «kaltsit», the character, in fiction): that
    // is not an old axiom coming back, so only the mark counts.
    let old_core: Vec<&str> = old.nodes.iter().filter(|n| n.is_axiom() && !want.contains_key(n.key.as_str())).map(|n| n.key.as_str()).collect();
    let back: Vec<&str> = old_core.iter().copied().filter(|k| m.by_key(k).is_some_and(|id| m.nodes[id as usize].is_axiom())).collect();
    if !back.is_empty() {
        return Err(format!("han vuelto axiomas viejos: {back:?}"));
    }
    ok.push(format!("no ha vuelto ningún axioma viejo (había {} en la memoria anterior)", old.nodes.iter().filter(|n| n.is_axiom()).count()));
    let shown = mind.moral_json()["axiomas"].as_array().map_or(0, |a| a.len());
    if shown != want.len() {
        return Err(format!("el compás moral lista {shown} axiomas, debían ser {}", want.len()));
    }
    ok.push(format!("el compás moral lista los {shown} axiomas"));

    if (m.nodes.len(), m.edges.len(), m.chat.len()) != (new.nodes.len(), new.edges.len(), new.chat.len()) {
        return Err(format!(
            "conteos distintos al reabrir: nodos {}/{}, aristas {}/{}, mensajes {}/{}",
            m.nodes.len(),
            new.nodes.len(),
            m.edges.len(),
            new.edges.len(),
            m.chat.len(),
            new.chat.len()
        ));
    }
    ok.push(format!("{} nodos, {} aristas y {} mensajes, como se guardaron", m.nodes.len(), m.edges.len(), m.chat.len()));
    // «present» is her clock: opening moves it forward to now (in memory, not on disk).
    if let Some((k, _)) = new.meta.iter().find(|(k, v)| k != "present" && m.meta(k) != Some(v.as_str())) {
        return Err(format!("el metadato {k} cambió al guardar"));
    }
    let vital = new.meta.iter().filter(|(k, _)| k.starts_with("vital.")).count();
    ok.push(format!("metadatos intactos ({} en total, {vital} vitales: hormonas y homeostasis)", new.meta.len()));
    if mind.mem.periods.first().map(|p| &p.persona) != guided.periods.first().map(|p| &p.persona) {
        return Err("la persona no es la de la semilla nueva".into());
    }
    ok.push("la persona es la de la semilla nueva".into());
    Ok(ok)
}

// ── the command ──

/// Reads `local.json` without writing anything (`Local::load` would create it).
fn app_port(data: &Path) -> u16 {
    std::fs::read_to_string(data.join("local.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["puerto"].as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or_else(|| crate::local::Local::default().puerto)
}

/// Other running `kaltsit.exe` processes (not this one), by pid. Empty off Windows or if the
/// list cannot be read.
fn other_instances() -> Vec<u32> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("tasklist").args(["/FI", "IMAGENAME eq kaltsit.exe", "/FO", "CSV", "/NH"]).creation_flags(0x0800_0000).output();
        if let Ok(o) = out {
            let me = std::process::id();
            return String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.to_ascii_lowercase().starts_with("\"kaltsit.exe\""))
                .filter_map(|l| l.split("\",\"").nth(1).and_then(|p| p.trim_matches('"').parse::<u32>().ok()))
                .filter(|&p| p != me)
                .collect();
        }
    }
    Vec::new()
}

/// The app is not using the folder: nothing answers on its port and no other `kaltsit.exe` runs.
/// There is no lock file in the app, so this is all that can be known from outside.
fn check_not_running(data: &Path, force: bool) -> Result<(), String> {
    let port = app_port(data);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500)).is_ok() {
        return Err(format!("algo responde en el puerto {port} de esta carpeta: la aplicación está abierta. Ciérrala y vuelve a ejecutar."));
    }
    let others = other_instances();
    if !others.is_empty() && !force {
        return Err(format!("hay otra instancia de kaltsit.exe en marcha (pid {others:?}). Ciérrala; si es una copia que usa OTRA carpeta, repite con --forzar."));
    }
    Ok(())
}

/// On Windows release builds there is no console (`windows_subsystem = "windows"`): join the
/// terminal that launched the command so its output can be read.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_console() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{AttachConsole, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    // SAFETY: plain Win32 calls; the handle is opened here and stays open for the life of the process.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                let h = f.into_raw_handle();
                SetStdHandle(STD_OUTPUT_HANDLE, h as _);
                SetStdHandle(STD_ERROR_HANDLE, h as _);
            }
        }
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_console() {}

/// `--migrar-sectores`: runs the migration over `data` and returns the exit code.
pub fn command(data: &Path) -> i32 {
    attach_console();
    let force = std::env::args().any(|a| a == "--forzar");
    println!("kaltsit --migrar-sectores · carpeta de datos: {}", data.display());
    println!("La aplicación debe estar CERRADA mientras esto corre (si la abres después, usará la memoria ya migrada).");
    if let Err(e) = check_not_running(data, force) {
        eprintln!("ABORTADO: {e}");
        return 2;
    }
    println!("Comprobado: nada responde en el puerto de la app{}.", if force { " (instancias ajenas ignoradas por --forzar)" } else { " y no hay otro kaltsit.exe" });
    let now = clock::unix_now();
    let mut log = Vec::new();
    let res = migrate(data, now, &stamp_of(now), &verify_opened, &mut log);
    for l in &log {
        println!("{l}");
    }
    match res {
        Ok(Outcome::Already) => 0,
        Ok(Outcome::Applied(a)) => {
            println!("RESULTADO: migración aplicada. Copia anterior: {} · informe: {}", a.backup.display(), a.report.display());
            println!("Para volver atrás: cierra la app y copia el contenido de la carpeta de la copia (menos {REPORT_FILE}) sobre {}.", data.display());
            0
        }
        Err(e) => {
            eprintln!("RESULTADO: no se aplicó. {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{COSAS, PERSONAS, YO};

    const NOW: f64 = 1_793_000_000.0;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kaltsit-migrar-{name}-{}", std::process::id()));
        let _ = force_remove(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A folder of data as the app leaves it with the sector-less memory: seed, a few facts and
    /// chat, her vital state, plus the files around it (local.json, a key, a read-only git pack).
    fn old_data(root: &Path) -> PathBuf {
        let data = root.join("data");
        let mut m = seed::build_legacy().unwrap();
        let src = m.source_id(&Source { mac: "AA-BB".into(), host: "SOBREMESA".into(), model: "gemma".into() });
        for n in m.nodes.iter_mut() {
            n.module = if n.kind == NodeKind::Person && n.y > crate::mind::TERRA && !n.has(flags::CORE) { PERSONAS } else { YO };
            n.source = src;
            n.born = 1_790_000_000.0;
            if n.y > crate::mind::TERRA {
                n.y = 2026.8;
            }
        }
        for (key, label, text) in [("f1", "Usa Rust", "Ana programa en Rust."), ("f2", "Corre un modelo", "Un modelo GGUF en la GPU.")] {
            m.add_node(Node {
                id: 0,
                kind: NodeKind::Fact,
                origin: Origin::Learned,
                flags: 0,
                period: 0,
                x: 0.1,
                y: 2026.8,
                alpha: 3.0,
                beta: 1.0,
                activation: 0.4,
                recalls: 2,
                created_turn: 3,
                key: key.into(),
                label: label.into(),
                text: text.into(),
                aliases: vec![],
                module: COSAS,
                source: src,
                born: 1_791_000_000.0,
            });
        }
        let f1 = m.by_key("f1").unwrap();
        m.chat.push(ChatMessage { period: 0, role: Role::User, turn: 1, text: "hola".into(), recalled: vec![f1], at: 1_790_000_100.0 });
        m.set_meta("usuario", "Ana");
        m.set_meta("seed", "kaltsit.ron");
        m.set_meta("identidad", "2");
        m.set_meta("vital.h.at", "123");
        m.set_meta("vital.homeostasis.a", "0.7000");
        m.set_meta("turno", "7");
        m.turn = 7;
        let mods: Vec<modules::Module> = modules::CORE.iter().map(|(n, t)| modules::Module { name: n.to_string(), title: t.to_string() }).collect();
        modules::save(&data, &m, &mods, &[], NOW - 10.0).unwrap();
        std::fs::write(data.join("local.json"), "{\"puerto\": 47821}").unwrap();
        std::fs::write(data.join("enlace.key"), "clave").unwrap();
        let pack = data.join(".git").join("objects").join("pack");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::create_dir_all(data.join(".git").join("refs").join("empty")).unwrap();
        std::fs::write(pack.join("p.pack"), vec![7u8; 5000]).unwrap();
        let mut perm = std::fs::metadata(pack.join("p.pack")).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(pack.join("p.pack"), perm).unwrap();
        data
    }

    /// The migration is the owner's one-off and reads his personal seed from disk: the tests that
    /// run it skip cleanly (and say so) on a checkout without that file.
    fn personal_here() -> bool {
        let ok = seed::personal_seed().is_ok();
        if !ok {
            eprintln!("SKIPPED (no personal seed file)");
        }
        ok
    }

    fn run(data: &Path, stamp: &str, verify: Verify) -> (Result<Outcome, String>, Vec<String>) {
        let mut log = Vec::new();
        let r = migrate(data, NOW, stamp, verify, &mut log);
        (r, log)
    }

    #[test]
    fn an_old_memory_migrates_leaves_an_identical_backup_and_opens_with_sectors() {
        if !personal_here() {
            return;
        }
        let root = tmp("ok");
        let data = old_data(&root);
        let before = tree(&data).unwrap();
        let (r, log) = run(&data, "20261025-120000", &verify_opened);
        let Outcome::Applied(a) = r.unwrap_or_else(|e| panic!("{e}\n{log:#?}")) else { panic!("debía aplicarse") };
        // The backup is the whole old folder, byte by byte, plus the report.
        assert_eq!(a.backup, root.join("data_backup").join("antes-sectores-20261025-120000"));
        let mut bt = tree(&a.backup).unwrap();
        let report = bt.remove(REPORT_FILE).expect("informe dentro de la copia");
        assert!(report.0 > 100);
        assert_eq!(bt, before, "copia idéntica, .git incluido");
        let md = std::fs::read_to_string(&a.report).unwrap();
        assert!(md.starts_with("# Importación aplicada") && md.contains("## Comprobaciones tras guardar") && md.contains("OK: el núcleo son los 9 axiomas"), "{md}");
        // The folder in use is the new memory, and the files that are not memory stayed.
        assert_ne!(tree(&data).unwrap(), before);
        assert_eq!(std::fs::read_to_string(data.join("enlace.key")).unwrap(), "clave");
        let m = Mind::open(&data, "x").unwrap();
        assert!(sectors::has_sectors(&m.mem));
        assert_eq!(m.mem.nodes.iter().filter(|n| n.is_axiom()).count(), 9);
        assert!(m.mem.by_key("f1").is_some() && m.mem.meta("vital.homeostasis.a") == Some("0.7000"));
        assert!(log.iter().any(|l| l.contains("Copia de seguridad")) && log.iter().any(|l| l.contains("Comprobación OK")));
        let _ = force_remove(&root);
    }

    #[test]
    fn a_memory_that_already_has_sectors_is_left_alone() {
        if !personal_here() {
            return;
        }
        let root = tmp("already");
        let data = old_data(&root);
        let (r, _) = run(&data, "20261025-120000", &verify_opened);
        assert!(matches!(r, Ok(Outcome::Applied(_))));
        let after_first = tree(&data).unwrap();
        let (r, log) = run(&data, "20261025-120001", &verify_opened);
        assert!(matches!(r, Ok(Outcome::Already)));
        assert!(log[0].contains("YA tiene sectores"));
        assert_eq!(tree(&data).unwrap(), after_first, "ni un byte cambió");
        assert!(!root.join("data_backup").join("antes-sectores-20261025-120001").exists(), "ni siquiera copia");
        let _ = force_remove(&root);
    }

    #[test]
    fn without_the_personal_seed_the_migration_stops_before_touching_anything() {
        let root = tmp("nopersonal");
        let data = old_data(&root);
        let before = tree(&data).unwrap();
        let (r, _) = seed::without_personal(|| run(&data, "20261025-120000", &verify_opened));
        let e = r.err().expect("sin la semilla personal no migra");
        assert!(e.contains("semilla personal") && e.contains("No se ha tocado nada"), "{e}");
        assert_eq!(tree(&data).unwrap(), before);
        assert!(!root.join("data_backup").exists(), "ni siquiera copia");
        let _ = force_remove(&root);
    }

    #[test]
    fn if_the_final_check_fails_the_folder_is_restored_from_the_backup() {
        if !personal_here() {
            return;
        }
        let root = tmp("restore");
        let data = old_data(&root);
        let before = tree(&data).unwrap();
        let broken = |_: &Path, _: &Memory, _: &Memory| -> Result<Vec<String>, String> { Err("fallo simulado".into()) };
        let (r, log) = run(&data, "20261025-120000", &broken);
        let e = r.err().expect("debía fallar");
        assert!(e.contains("fallo simulado") && e.contains("RESTAURADO"), "{e}");
        assert_eq!(tree(&data).unwrap(), before, "la carpeta quedó idéntica a la original");
        assert!(log.iter().any(|l| l.starts_with("FALLO")));
        // The copy stays, with the report that says what failed; the old memory opens as before.
        let backup = root.join("data_backup").join("antes-sectores-20261025-120000");
        assert!(std::fs::read_to_string(backup.join(REPORT_FILE)).unwrap().contains("FALLÓ: fallo simulado"));
        assert!(!sectors::has_sectors(&modules::load(&data).unwrap().unwrap().mem));
        let _ = force_remove(&root);
    }

    #[test]
    fn it_refuses_before_touching_anything_when_it_cannot_do_it_faithfully() {
        let root = tmp("refuse");
        let data = old_data(&root);
        // An extra module (a memory from another computer) would come back to life as old nodes.
        let extra = Memory::default();
        let m = modules::Module { name: "portatil".into(), title: "Portátil".into() };
        std::fs::write(modules::dir_of(&data).join("portatil.cthm"), modules::encode(&modules::raw_of(&extra, 0, &m, &[], 1.0))).unwrap();
        let before = tree(&data).unwrap();
        let (r, _) = run(&data, "20261025-120000", &verify_opened);
        assert!(r.err().unwrap().contains("módulos extra"));
        assert_eq!(tree(&data).unwrap(), before);
        assert!(!root.join("data_backup").exists());
        // A folder without memory is not a memory either.
        assert!(run(&root.join("nada"), "x", &verify_opened).0.is_err());
        let _ = force_remove(&root);
    }

    #[test]
    fn a_backup_that_already_exists_is_never_overwritten() {
        let root = tmp("exists");
        let data = old_data(&root);
        std::fs::create_dir_all(root.join("data_backup").join("antes-sectores-20261025-120000")).unwrap();
        let before = tree(&data).unwrap();
        let (r, _) = run(&data, "20261025-120000", &verify_opened);
        assert!(r.err().unwrap().contains("ya existe"));
        assert_eq!(tree(&data).unwrap(), before);
        let _ = force_remove(&root);
    }

    #[test]
    fn stamp_has_the_shape_the_folder_name_needs() {
        let s = stamp_of(NOW);
        assert_eq!(s.len(), 15, "{s}");
        assert_eq!(s.as_bytes()[8], b'-');
        assert!(s.bytes().enumerate().all(|(i, b)| i == 8 || b.is_ascii_digit()));
        assert_eq!(backup_root(Path::new("data")), Path::new("./data_backup"));
        assert_eq!(backup_root(Path::new("x/y/data")), Path::new("x/y/data_backup"));
    }

    #[test]
    fn an_app_answering_on_its_port_stops_the_command() {
        let root = tmp("port");
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::fs::write(data.join("local.json"), format!("{{\"puerto\": {port}}}")).unwrap();
        assert_eq!(app_port(&data), port);
        let e = check_not_running(&data, true).unwrap_err();
        assert!(e.contains("abierta"), "{e}");
        drop(l);
        assert!(check_not_running(&data, true).is_ok(), "closed port: free");
        let _ = force_remove(&root);
    }

    // ── the real thing, over a copy ──

    /// Copy of `data/` in full, hashed before and after: the real folder is never written.
    #[test]
    #[ignore = "reads data/ (copying it) and migrates the COPY; run it by exact name"]
    fn migration_over_a_full_copy_of_the_real_data() {
        let real = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
        let real_before = tree(&real).unwrap();
        assert!(!real_before.is_empty(), "no hay data/");
        let root = tmp("real");
        let copy = root.join("data");
        copy_tree(&real, &copy, &[]).unwrap();
        assert_eq!(tree(&copy).unwrap(), real_before);

        let now = clock::unix_now();
        let mut log = Vec::new();
        let r = migrate(&copy, now, &stamp_of(now), &verify_opened, &mut log);
        for l in &log {
            println!("{l}");
        }
        let Outcome::Applied(a) = r.unwrap() else { panic!("la copia real no tiene sectores y debía migrar") };
        assert_eq!(tree(&a.backup).unwrap().len(), real_before.len() + 1, "toda la carpeta + el informe");

        // Now she lives in it.
        let old = modules::load(&a.backup).unwrap().unwrap().mem;
        let mut mind = Mind::open(&copy, "modelo-de-prueba").unwrap();
        assert!(sectors::has_sectors(&mind.mem));
        // Her vital state came across before she does anything.
        for (k, v) in old.meta.iter().filter(|(k, _)| k.starts_with("vital.")) {
            assert_eq!(mind.mem.meta(k), Some(v.as_str()), "{k}");
        }
        assert!(old.meta.iter().any(|(k, _)| k.starts_with("vital.h.")) && old.meta.iter().any(|(k, _)| k.starts_with("vital.homeostasis.")), "la memoria real tenía hormonas y homeostasis");
        let affinity: f32 = old.meta("vital.homeostasis.a").expect("afinidad guardada").parse().unwrap();
        assert!((mind.homeostasis.state.affinity - affinity.clamp(0.0, 1.0)).abs() < 1e-6, "homeostasis conservada");
        println!("hormonas y homeostasis conservadas (afinidad {affinity})");
        // A real message of the history: the user's lines, one from the middle.
        let said: Vec<&str> = mind.mem.chat.iter().filter(|c| c.role == Role::User).map(|c| c.text.as_str()).filter(|t| t.chars().count() > 25).collect();
        assert!(!said.is_empty());
        let msg = said[said.len() / 2].to_string();
        mind.say(Role::User, &msg, &[]);
        let p = mind.prepare_reply(1800, "tranquila", None, None);
        assert!(!p.messages.is_empty());
        let sys = p.messages[0].as_text();
        assert!(sys.contains("Hablas con:"), "el prompt se construye");
        println!("mensaje real: «{msg}» → {} recuerdos en el prompt, entendimiento {:.2}", p.recalled.len(), p.understood);
        assert!(p.recalled.iter().all(|&(id, _)| mind.mem.nodes[id as usize].kind != NodeKind::Sector), "un sector nunca es recuerdo");

        // She learns a synthetic fact: it is born with a sector.
        let ep = mind.record_reply("Te cuento algo", "Anotado.", &[]);
        let ev = mind.learn(
            &serde_json::json!({ "hechos": [{ "tipo": "hecho", "texto": "Ana prueba la migración de sectores en una copia", "modulo": "cosas" }], "nuevos": [], "tension": 0.0, "afecto": 0.0 }),
            &[],
            ep,
            1.0,
            None,
        );
        let id = ev.iter().map(|e| e.node).find(|&n| mind.mem.nodes[n as usize].text.contains("migración de sectores")).expect("aprendió el hecho");
        let homes: Vec<&str> = mind.mem.edges.iter().filter(|e| e.kind == EdgeKind::BelongsTo && e.from == id).map(|e| mind.mem.nodes[e.to as usize].key.as_str()).collect();
        assert!(!homes.is_empty(), "el hecho nuevo nació sin sector");
        println!("el hecho sintético nació en: {homes:?}");

        // Moral compass: the nine new axioms, hormones and homeostasis kept.
        let moral = mind.moral_json();
        let axioms = moral["axiomas"].as_array().unwrap();
        assert_eq!(axioms.len(), 9, "{axioms:?}");
        let guided = seed::build_personal().unwrap();
        for n in guided.nodes.iter().filter(|n| n.is_axiom()) {
            assert!(axioms.iter().any(|a| a["label"] == n.label.as_str()), "falta el axioma {}", n.label);
        }
        // The memory and graph the app shows do not fail with Sector nodes.
        let mj = mind.memory_json();
        assert_eq!(mj["modules"].as_array().unwrap().len(), 4);
        let gj = mind.graph_json();
        assert!(gj["nodes"].as_array().unwrap().iter().any(|n| n["k"] == "sector"));
        assert!(gj["edges"].as_array().unwrap().iter().any(|e| e["k"] == "pertenece"));
        mind.save().unwrap();

        // The real folder did not change by a byte.
        assert_eq!(tree(&real).unwrap(), real_before, "data/ cambió");
        let _ = force_remove(&root);
    }

}
