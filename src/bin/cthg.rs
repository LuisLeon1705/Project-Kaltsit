//! `cthg`: the command line of the CTHG engine. For now, the model manager:
//!
//!   cthg models hardware                 this computer and the profile it matches
//!   cthg models list [--perfil <id>]     the catalog, with how each one fits
//!   cthg models packs                    the ready-made packs
//!   cthg models download <id>...         download models, voices or the engine
//!   cthg models download --pack <pack>   download a whole pack
//!   cthg models use <id> [--data data]   point this computer's local.json at a model
//!
//! Downloads resume where they stopped and are checked against the manifest's sha256.

#[path = "../catalog.rs"]
mod catalog;
#[path = "../download.rs"]
mod download;

mod lang {
    pub fn tr(es: impl Into<String>, _en: impl Into<String>) -> String {
        es.into()
    }
}

use catalog::{Item, Manifest, Profile};
use std::io::Write;
use std::time::Instant;

fn gb(b: u64) -> String {
    format!("{:.2} GB", b as f64 / 1e9)
}

fn usage() -> ! {
    eprintln!(
        "cthg models — modelos para Kal'tsit

  cthg models hardware                 este equipo y el perfil que le corresponde
  cthg models list [--perfil <id>]     el catálogo y cómo cabe cada modelo
  cthg models packs                    los packs preparados
  cthg models download <id>...         descarga modelos, voces o el motor
  cthg models download --pack <pack>   descarga un pack entero
  cthg models use <id> [--data data]   usa ese modelo en este equipo (data\\local.json)

Perfiles: cpu, gpu-4gb, gpu-6gb, gpu-8gb, gpu-12gb, gpu-16gb (sin --perfil mide este equipo)."
    );
    std::process::exit(2)
}

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn profile_for(m: &Manifest, args: &[String]) -> (Profile, Option<catalog::Hardware>) {
    if let Some(id) = opt(args, "--perfil") {
        let p = m.profile(&id).cloned().unwrap_or_else(|| {
            eprintln!("perfil desconocido: {id}");
            std::process::exit(2)
        });
        return (p, None);
    }
    let hw = catalog::hardware();
    let p = Profile::from_hardware(&hw);
    (p, Some(hw))
}

fn print_hardware(hw: &catalog::Hardware, p: &Profile) {
    println!("GPU   {} ({:.1} GB de VRAM)", if hw.gpu.is_empty() { "no detectada" } else { &hw.gpu }, hw.vram_gb);
    println!("CPU   {}", if hw.cpu.is_empty() { "?" } else { &hw.cpu });
    if let Some((used, total)) = catalog::vram_now() {
        println!("      {:.1} de {:.1} GB de VRAM en uso ahora", used, total);
    }
    println!("RAM   {:.0} GB", hw.ram_gb);
    println!("perfil  {} — {} (presupuesto {:.1} GB)", p.id, p.nombre, p.budget_gb());
    if !p.nota.is_empty() {
        println!("        {}", p.nota);
    }
}

fn list(m: &Manifest, p: &Profile) {
    let root = m.root();
    println!("Para {} (presupuesto {:.1} GB):\n", p.nombre, p.budget_gb());
    for tipo in ["cerebro", "voz", "oido", "motor"] {
        let items: Vec<&Item> = m.elementos.iter().filter(|i| i.tipo == tipo).collect();
        if items.is_empty() {
            continue;
        }
        println!("{}", match tipo {
            "cerebro" => "Cerebro (chat + visión)",
            "voz" => "Voz (sherpa-onnx, en CPU)",
            "oido" => "Oído (dictado, sherpa-onnx en CPU)",
            _ => "Motor",
        });
        for i in items {
            let installed = if i.installed(&root) { "instalado" } else { "" };
            let fit = if tipo == "cerebro" {
                let gbv = i.vram_gb();
                format!("~{gbv:.1} GB VRAM · {}", catalog::fit(gbv, p).name())
            } else if let Some(v) = &i.voz {
                match v.motor.as_str() {
                    "kokoro" => format!("Kokoro · voz {} · {}", v.sid, v.lang),
                    "pocket" => format!("Pocket · clona una voz · {}", if v.idioma.is_empty() { "es" } else { &v.idioma }),
                    _ => "Piper (VITS)".to_string(),
                }
            } else if let Some(o) = &i.oido {
                if o.motor == "voces" { "limpia el ruido y reconoce voces".to_string() } else { format!("{} · entiende es/en", o.motor) }
            } else {
                String::new()
            };
            println!("  {:20} {:7} {:40} descarga {:>9}  {fit:28} {installed}", i.id, i.nivel, i.nombre, gb(i.download_bytes()));
            if !i.nota.is_empty() {
                println!("  {:20} {}", "", i.nota);
            }
        }
        println!();
    }
    if let Some(pick) = m.recommend_one(p) {
        let why = match pick.reason {
            "roomy" => "cabe con margen",
            "tight" => "cabe justo",
            "cpu" => "sin gráfica: en CPU y RAM",
            _ => "no cabe ninguno: el más pequeño",
        };
        println!(
            "Recomendado aquí: {} ({}) — ~{:.1} GB, {} ({}){}.",
            pick.item.nombre,
            pick.item.id,
            pick.gb,
            pick.fit.name(),
            why,
            if pick.slow { " — irá lento" } else { "" }
        );
    }
    for pk in m.packs_for(&p.id) {
        println!("Pack {}: cthg models download --pack {}   ({})", pk.nombre, pk.id, pk.incluye.join(" + "));
    }
}

fn packs(m: &Manifest) {
    for p in &m.packs {
        let items: Vec<&Item> = p.incluye.iter().filter_map(|id| m.item(id)).collect();
        let total: u64 = items.iter().map(|i| i.download_bytes()).sum();
        println!("{:16} {:22} {:>9}  {}", p.id, p.nombre, gb(total), p.incluye.join(" + "));
    }
}

/// A progress bar on one line, redrawn at most five times a second.
struct Bar {
    name: String,
    started: Instant,
    last: Instant,
    from: u64,
    hashing: bool,
}

impl Bar {
    fn new(name: &str) -> Bar {
        Bar { name: name.to_string(), started: Instant::now(), last: Instant::now() - std::time::Duration::from_secs(1), from: u64::MAX, hashing: false }
    }

    fn draw(&mut self, done: u64, total: u64) {
        if self.from == u64::MAX {
            self.from = done;
        }
        // The download reports up to the total; after it, the sha256 check starts again from 0.
        if done < self.from && !self.hashing {
            self.hashing = true;
            self.started = Instant::now();
            self.from = 0;
        }
        if self.last.elapsed().as_millis() < 200 && done < total {
            return;
        }
        self.last = Instant::now();
        let frac = if total > 0 { done as f64 / total as f64 } else { 1.0 };
        let filled = (frac * 28.0) as usize;
        let secs = self.started.elapsed().as_secs_f64().max(0.001);
        let speed = (done - self.from.min(done)) as f64 / secs;
        let eta = if speed > 0.0 { ((total - done) as f64 / speed) as u64 } else { 0 };
        let what = if self.hashing { "verificando" } else { "descargando" };
        print!(
            "\r  {:34} {what} [{}{}] {:5.1}% {} / {}  {:6.1} MB/s  {:>2}:{:02} ",
            self.name.chars().take(34).collect::<String>(),
            "#".repeat(filled),
            "-".repeat(28 - filled),
            frac * 100.0,
            gb(done),
            gb(total),
            speed / 1e6,
            eta / 60,
            eta % 60
        );
        let _ = std::io::stdout().flush();
    }
}

fn download(m: &Manifest, ids: &[String]) -> Result<(), String> {
    let root = m.root();
    let items: Vec<&Item> = ids.iter().map(|id| m.item(id).ok_or_else(|| format!("no hay ningún «{id}» en el catálogo (cthg models list)"))).collect::<Result<_, _>>()?;
    let total: u64 = items.iter().map(|i| i.download_bytes()).sum();
    println!("Descargando en {}\\ ({} como máximo; lo que ya esté completo se salta)\n", root.display(), gb(total));
    for i in items {
        println!("{} — {}", i.id, i.nombre);
        for f in &i.archivos {
            let dest = root.join(&f.destino);
            let mut bar = Bar::new(&f.destino);
            let fresh = download::fetch(&f.url, &dest, f.bytes, &f.sha256, &mut |d, t| bar.draw(d, t))?;
            println!("\r  {:34} {}{}", f.destino.chars().take(34).collect::<String>(), if fresh { "listo" } else { "ya estaba" }, " ".repeat(70));
            if let Some(to) = &f.convertir {
                if fresh || !root.join(to).exists() {
                    download::ogg_to_wav(&dest, &root.join(to), 10.0)?;
                    println!("  convertido en {}", root.join(to).display());
                }
            }
            if let Some(to) = &f.extraer {
                if fresh || !i.unpacked(&root) {
                    download::unzip(&dest, &root.join(to))?;
                    println!("  descomprimido en {}", root.join(to).display());
                }
            }
        }
        println!();
    }
    Ok(())
}

fn main() {
    if let Ok(exe) = std::env::current_exe() {
        let mut root = exe.parent();
        if let Some(p) = root {
            if p.file_name().map_or(false, |n| n == "release" || n == "debug") {
                if let Some(target) = p.parent() {
                    if target.file_name().map_or(false, |n| n == "target") {
                        root = target.parent();
                    }
                }
            }
        }
        if let Some(r) = root {
            let _ = std::env::set_current_dir(r);
        }
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("models") {
        usage();
    }
    let m = Manifest::load();
    let rest = &args[1..];
    let result = match rest.first().map(String::as_str) {
        Some("hardware") => {
            let hw = catalog::hardware();
            let p = Profile::from_hardware(&hw);
            print_hardware(&hw, &p);
            Ok(())
        }
        Some("list") => {
            let (p, hw) = profile_for(&m, rest);
            if let Some(hw) = hw {
                print_hardware(&hw, &p);
                println!();
            }
            list(&m, &p);
            Ok(())
        }
        Some("packs") => {
            packs(&m);
            Ok(())
        }
        Some("download") => {
            let ids: Vec<String> = match opt(rest, "--pack") {
                Some(p) => match m.pack(&p) {
                    Some(pk) => pk.incluye.clone(),
                    None => {
                        eprintln!("pack desconocido: {p} (cthg models packs)");
                        std::process::exit(2)
                    }
                },
                None => rest[1..].iter().filter(|a| !a.starts_with("--")).cloned().collect(),
            };
            if ids.is_empty() {
                usage();
            }
            download(&m, &ids)
        }
        Some("use") => {
            let id = rest.get(1).cloned().unwrap_or_else(|| usage());
            let data = opt(rest, "--data").unwrap_or_else(|| "data".into());
            catalog::use_item(&m, &id, std::path::Path::new(&data)).map(|v| {
                println!("{} usará {}", data, v["llama"]["modelo"].as_str().unwrap_or(""));
                println!("Si este equipo es el cerebro ahora mismo, reinicia la app (o elige el modelo desde «Modelos» en la app) para cargarlo.");
            })
        }
        _ => usage(),
    };
    if let Err(e) = result {
        eprintln!("\nerror: {e}");
        std::process::exit(1);
    }
}
