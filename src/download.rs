//! Downloads with resume and verification: a `.part` file grows until the size matches, an
//! interrupted download continues where it stopped (HTTP Range), and the sha256 of the
//! manifest must match before the file takes its real name. Zips (the engine) are unpacked
//! with Windows' own `tar`.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CHUNK: usize = 1 << 20;

/// Bytes done, bytes in total.
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64);

fn sha256_of(path: &Path, total: u64, progress: Progress) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Downloads `url` into `dest` (`bytes` long, `sha256` if known). Returns false if it was
/// already there and complete.
pub fn fetch(url: &str, dest: &Path, bytes: u64, sha256: &str, progress: Progress) -> Result<bool, String> {
    if std::fs::metadata(dest).map(|m| m.len() == bytes).unwrap_or(false) {
        return Ok(false);
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let part: PathBuf = PathBuf::from(format!("{}.part", dest.display()));
    let mut start = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if start > bytes {
        start = 0;
    }
    if start < bytes {
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).build();
        let mut req = agent.get(url).set("User-Agent", "kaltsit-cthg");
        if start > 0 {
            req = req.set("Range", &format!("bytes={start}-"));
        }
        let resp = req.call().map_err(|e| format!("{url}: {e}"))?;
        // A server that ignores Range sends everything again: start over.
        if resp.status() != 206 {
            start = 0;
        }
        let mut file = std::fs::OpenOptions::new().create(true).write(true).append(start > 0).truncate(start == 0).open(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; CHUNK];
        let mut done = start;
        progress(done, bytes);
        loop {
            let n = reader.read(&mut buf).map_err(|e| crate::lang::tr(format!("descarga interrumpida ({done} de {bytes} bytes; se reanudará): {e}"), format!("download interrupted ({done} of {bytes} bytes; it will resume): {e}")))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            progress(done, bytes);
        }
        file.flush().map_err(|e| e.to_string())?;
    }
    let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if got != bytes {
        return Err(crate::lang::tr(format!("{}: {got} bytes de {bytes}; vuelve a intentarlo para reanudar", dest.display()), format!("{}: {got} bytes of {bytes}; try again to resume", dest.display())));
    }
    if !sha256.is_empty() {
        let h = sha256_of(&part, bytes, progress)?;
        if !h.eq_ignore_ascii_case(sha256) {
            let _ = std::fs::remove_file(&part);
            return Err(crate::lang::tr(format!("{}: el sha256 no coincide; se borró la descarga", dest.display()), format!("{}: the sha256 does not match; the download was deleted", dest.display())));
        }
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 16-bit mono PCM WAV.
pub fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    w
}

/// An Ogg Vorbis clip as a mono WAV of at most `max_secs` seconds (a cloning voice's reference).
pub fn ogg_to_wav(ogg: &Path, out: &Path, max_secs: f32) -> Result<(), String> {
    let f = std::fs::File::open(ogg).map_err(|e| format!("{}: {e}", ogg.display()))?;
    let mut r = lewton::inside_ogg::OggStreamReader::new(std::io::BufReader::new(f)).map_err(|e| format!("{}: {e}", ogg.display()))?;
    let (rate, ch) = (r.ident_hdr.audio_sample_rate, r.ident_hdr.audio_channels.max(1) as usize);
    let max = (max_secs * rate as f32) as usize;
    let mut mono: Vec<f32> = Vec::new();
    while mono.len() < max {
        match r.read_dec_packet_itl().map_err(|e| format!("{}: {e}", ogg.display()))? {
            Some(p) => mono.extend(p.chunks_exact(ch).map(|f| f.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / ch as f32)),
            None => break,
        }
    }
    mono.truncate(max);
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, wav(&mono, rate)).map_err(|e| format!("{}: {e}", out.display()))
}

/// Unpacks a zip into `to` with the `tar` that ships with Windows 10 and later.
pub fn unzip(zip: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut c = std::process::Command::new("tar");
    c.arg("-xf").arg(zip).arg("-C").arg(to);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().map_err(|e| format!("tar: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(crate::lang::tr(format!("no se pudo descomprimir {}: {}", zip.display(), String::from_utf8_lossy(&out.stderr).trim()), format!("could not unzip {}: {}", zip.display(), String::from_utf8_lossy(&out.stderr).trim())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A tiny server that honours Range (like Hugging Face's CDN) and counts the bytes it sends.
    fn serve(body: Vec<u8>, port: u16) -> Arc<AtomicUsize> {
        let sent = Arc::new(AtomicUsize::new(0));
        let counter = sent.clone();
        let server = tiny_http::Server::http(("127.0.0.1", port)).unwrap();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let range = req.headers().iter().find(|h| h.field.equiv("Range")).map(|h| h.value.as_str().to_string());
                let (code, slice) = match range.as_deref().and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.trim_end_matches('-').parse::<usize>().ok()) {
                    Some(from) => (206, body[from..].to_vec()),
                    None => (200, body.clone()),
                };
                counter.fetch_add(slice.len(), Ordering::Relaxed);
                let _ = req.respond(tiny_http::Response::from_data(slice).with_status_code(code));
            }
        });
        sent
    }

    #[test]
    fn resumes_where_it_stopped_and_verifies() {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let sha = format!("{:x}", Sha256::digest(&body));
        let sent = serve(body.clone(), 47951);
        let d = std::env::temp_dir().join(format!("kaltsit-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let dest = d.join("llm/modelo.gguf");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        // A previous attempt left 100 000 bytes.
        std::fs::write(format!("{}.part", dest.display()), &body[..100_000]).unwrap();
        let mut last = (0, 0);
        let url = "http://127.0.0.1:47951/modelo.gguf";
        assert!(fetch(url, &dest, body.len() as u64, &sha, &mut |a, b| last = (a, b)).unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(sent.load(Ordering::Relaxed), 200_000, "only the missing part travels");
        assert_eq!(last.1, body.len() as u64);
        assert!(!fetch(url, &dest, body.len() as u64, &sha, &mut |_, _| {}).unwrap(), "already there");
        // A wrong hash deletes the download instead of keeping a corrupt model.
        let bad = d.join("llm/malo.gguf");
        let err = fetch(url, &bad, body.len() as u64, &"0".repeat(64), &mut |_, _| {}).unwrap_err();
        assert!(err.contains("sha256") && !bad.exists() && !Path::new(&format!("{}.part", bad.display())).exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
