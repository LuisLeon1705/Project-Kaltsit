//! What the companions share, in `data/comun/`: the computer's activity log (`activity`, one for
//! all, never recorded twice), the agenda (each one's upcoming plans, exported when she saves her
//! memory, read together by the calendar) and a board of notes they leave each other. Their
//! memories are not shared: only this.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The shared folder under the app's `data`.
pub fn dir(root: &Path) -> PathBuf {
    root.join("comun")
}

/// Kal'tsit's activity log lived in her own folder before there were several: it moves here.
pub fn adopt_old_activity(root: &Path) {
    let (old, new) = (root.join("actividad"), dir(root).join("actividad"));
    if old.is_dir() && !new.exists() {
        let _ = std::fs::create_dir_all(dir(root));
        let _ = std::fs::rename(old, new);
    }
}

// ── the agenda ──

/// A plan someone has: when (unix), what, and whose agenda it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub u: f64,
    pub label: String,
    pub texto: String,
    pub de: String,
}

/// Companion `who`'s upcoming plans, replacing what she had exported before.
pub fn export_agenda(root: &Path, who: &str, plans: &[Plan]) {
    let d = dir(root).join("agenda");
    let _ = std::fs::create_dir_all(&d);
    let _ = std::fs::write(d.join(format!("{who}.json")), serde_json::to_string_pretty(plans).unwrap_or_default());
}

/// Everyone's plans, in time order; the same plan in two agendas (same minute, same name) once.
pub fn agenda(root: &Path) -> Vec<Plan> {
    let mut all: Vec<Plan> = std::fs::read_dir(dir(root).join("agenda"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<Vec<Plan>>(&s).ok())
        .flatten()
        .collect();
    all.sort_by(|a, b| a.u.total_cmp(&b.u));
    let mut out: Vec<Plan> = Vec::new();
    for p in all {
        if !out.iter().any(|q| (q.u - p.u).abs() < 60.0 && q.label.to_lowercase() == p.label.to_lowercase()) {
            out.push(p);
        }
    }
    out
}

// ── the board of notes ──

/// A note one companion left another.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub de: String,
    pub para: String,
    pub texto: String,
    pub at: f64,
    pub leida: bool,
}

fn board(root: &Path) -> PathBuf {
    dir(root).join("notas.json")
}

/// Every note, newest first.
pub fn notes(root: &Path) -> Vec<Note> {
    let mut v: Vec<Note> = std::fs::read_to_string(board(root)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    v.sort_by(|a, b| b.at.total_cmp(&a.at));
    v
}

fn save(root: &Path, v: &[Note]) {
    let _ = std::fs::create_dir_all(dir(root));
    let _ = std::fs::write(board(root), serde_json::to_string_pretty(v).unwrap_or_default());
}

/// `de` leaves `para` a note (the board keeps the last 200).
pub fn leave_note(root: &Path, de: &str, para: &str, texto: &str, at: f64) {
    let texto = texto.trim();
    if texto.is_empty() {
        return;
    }
    let mut v = notes(root);
    v.insert(0, Note { de: de.into(), para: para.into(), texto: texto.chars().take(400).collect(), at, leida: false });
    v.truncate(200);
    save(root, &v);
}

/// The notes waiting for `para`, oldest first; from now on they are read.
pub fn take_notes(root: &Path, para: &str) -> Vec<Note> {
    let mut v = notes(root);
    let mut out: Vec<Note> = v.iter().filter(|n| n.para == para && !n.leida).cloned().collect();
    if out.is_empty() {
        return out;
    }
    for n in v.iter_mut().filter(|n| n.para == para) {
        n.leida = true;
    }
    save(root, &v);
    out.sort_by(|a, b| a.at.total_cmp(&b.at));
    out
}

// ── between computers ──
// Each companion lives on one computer (or follows one brain), but the bank is the household's:
// linked computers pull each other's and merge it. Each agenda belongs to whoever runs that
// companion (theirs replaces ours unless she runs here); notes are a union, read on either side
// stays read.

/// What this computer's bank holds, to send to another one.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bank {
    pub agenda: std::collections::BTreeMap<String, Vec<Plan>>,
    pub notas: Vec<Note>,
}

pub fn bank(root: &Path) -> Bank {
    let agenda = std::fs::read_dir(dir(root).join("agenda"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let who = e.path().file_stem()?.to_str()?.to_string();
            let plans = serde_json::from_str::<Vec<Plan>>(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            Some((who, plans))
        })
        .collect();
    Bank { agenda, notas: notes(root) }
}

/// Merges another computer's bank into this one; `here`: the companions running on this one
/// (their agendas are written here, never taken from elsewhere). Whether anything changed.
pub fn absorb(root: &Path, theirs: &Bank, here: &[&str]) -> bool {
    let mut changed = false;
    let mine = bank(root);
    for (who, plans) in &theirs.agenda {
        if here.contains(&who.as_str()) || mine.agenda.get(who) == Some(plans) || who.contains(['/', '\\', '.']) {
            continue;
        }
        export_agenda(root, who, plans);
        changed = true;
    }
    let same = |a: &Note, b: &Note| a.de == b.de && a.para == b.para && a.texto == b.texto && (a.at - b.at).abs() < 1e-3;
    let mut v = mine.notas;
    for n in &theirs.notas {
        match v.iter_mut().find(|m| same(m, n)) {
            Some(m) if n.leida && !m.leida => {
                m.leida = true;
                changed = true;
            }
            Some(_) => {}
            None => {
                v.push(n.clone());
                changed = true;
            }
        }
    }
    if changed {
        v.sort_by(|a, b| b.at.total_cmp(&a.at));
        v.truncate(200);
        save(root, &v);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kal'tsit on one computer, Lappland on another: each sees the other's plans and notes, a
    /// note read where its reader lives is read on both, and nobody's own agenda is overwritten.
    #[test]
    fn two_computers_share_the_bank() {
        let (a, b) = (temp("bank-a"), temp("bank-b"));
        let plan = |who: &str, label: &str| Plan { u: 1000.0, label: label.into(), texto: String::new(), de: who.into() };
        export_agenda(&a, "kaltsit", &[plan("kaltsit", "Dentista")]);
        export_agenda(&b, "lappland", &[plan("lappland", "Entrenar")]);
        export_agenda(&b, "kaltsit", &[plan("kaltsit", "viejo")]);
        leave_note(&a, "kaltsit", "lappland", "Ana duerme poco", 5.0);
        assert!(absorb(&b, &bank(&a), &["lappland"]));
        assert!(absorb(&a, &bank(&b), &["kaltsit"]));
        let labels = |r: &Path| agenda(r).into_iter().map(|p| p.label).collect::<Vec<_>>();
        assert_eq!(labels(&a), ["Dentista", "Entrenar"], "her own agenda is not replaced by a stale copy");
        assert_eq!(labels(&b), ["Dentista", "Entrenar"]);
        assert_eq!(take_notes(&b, "lappland").len(), 1, "the note reached Lappland's computer");
        absorb(&a, &bank(&b), &["kaltsit"]);
        assert!(notes(&a).iter().all(|n| n.leida), "read there, read here");
        assert!(!absorb(&a, &bank(&b), &["kaltsit"]), "nothing new: nothing written");
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(b);
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kaltsit-comun-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_agenda_is_everyones_plans_once() {
        let root = temp("agenda");
        let p = |u: f64, l: &str, de: &str| Plan { u, label: l.into(), texto: String::new(), de: de.into() };
        export_agenda(&root, "kaltsit", &[p(200.0, "Dentista", "kaltsit"), p(100.0, "Entrega", "kaltsit")]);
        export_agenda(&root, "lappland", &[p(230.0, "dentista", "lappland"), p(300.0, "Cena", "lappland")]);
        let a: Vec<String> = agenda(&root).iter().map(|x| x.label.clone()).collect();
        assert_eq!(a, vec!["Entrega", "Dentista", "Cena"], "in time order, the dentist once");
        export_agenda(&root, "lappland", &[]);
        assert_eq!(agenda(&root).len(), 2, "a new export replaces hers");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_note_is_delivered_once_to_whom_it_was_for() {
        let root = temp("notas");
        leave_note(&root, "kaltsit", "lappland", "Ana ha dormido poco.", 10.0);
        leave_note(&root, "kaltsit", "lappland", "  ", 11.0);
        leave_note(&root, "lappland", "kaltsit", "Ana quiere cenar pizza.", 12.0);
        assert!(take_notes(&root, "nadie").is_empty());
        let mine = take_notes(&root, "lappland");
        assert_eq!(mine.len(), 1, "the empty one is not a note");
        assert_eq!(mine[0].texto, "Ana ha dormido poco.");
        assert!(take_notes(&root, "lappland").is_empty(), "delivered once");
        assert_eq!(take_notes(&root, "kaltsit").len(), 1);
        assert_eq!(notes(&root).len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn kaltsits_old_activity_log_moves_to_the_shared_folder() {
        let root = temp("actividad");
        std::fs::create_dir_all(root.join("actividad").join("Ana")).unwrap();
        std::fs::write(root.join("actividad").join("Ana").join("2026-10-30.jsonl"), "{}").unwrap();
        adopt_old_activity(&root);
        assert!(dir(&root).join("actividad").join("Ana").join("2026-10-30.jsonl").exists());
        assert!(!root.join("actividad").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
