//! Tokenization for lexical matching, for Spanish and English at once: a Spanish speaker may
//! write an English word and the other way round, so the stopwords of both are always filtered.

/// English function words (folded, three letters or more; shorter ones are dropped anyway),
/// plus the pieces a contraction leaves once the apostrophe is folded away ("isn't" → "isn").
const STOP_EN: &[&str] = &[
    "about", "after", "again", "all", "also", "and", "any", "are", "aren", "because", "been", "before", "being", "between", "both", "but", "can", "could",
    "couldn", "did", "didn", "does", "doesn", "doing", "don", "down", "each", "for", "from", "had", "has", "hasn", "have", "haven", "her", "here", "hers",
    "him", "his", "how", "into", "isn", "its", "just", "may", "more", "most", "must", "not", "now", "off", "once", "only", "other", "our", "out", "over",
    "own", "same", "she", "should", "shouldn", "some", "such", "than", "that", "the", "their", "them", "then", "there", "these", "they", "this", "those",
    "through", "too", "under", "until", "very", "was", "wasn", "were", "weren", "what", "when", "where", "which", "while", "who", "whom", "why", "will",
    "with", "won", "would", "wouldn", "you", "your", "yours", "yourself",
];

const STOP: &[&str] = &[
    "a", "al", "algo", "ante", "aqui", "asi", "aun", "bajo", "bien", "cada", "como", "con", "contra",
    "cual", "cuando", "de", "del", "desde", "donde", "dos", "el", "ella", "ellas", "ellos", "en",
    "entre", "era", "eres", "es", "esa", "ese", "eso", "esta", "estas", "este", "esto", "estoy",
    "fue", "ha", "han", "hay", "la", "las", "le", "les", "lo", "los", "mas", "me", "mi", "mis",
    "mucho", "muy", "nada", "ni", "no", "nos", "o", "os", "para", "pero", "poco", "por", "porque",
    "que", "quien", "se", "sea", "ser", "si", "sin", "sobre", "solo", "son", "su", "sus", "tal",
    "te", "tan", "tengo", "ti", "tiene", "toda", "todo", "tu", "tus", "un", "una", "uno", "unos",
    "y", "ya", "yo", "the", "of", "and", "dei", "di", "il", "i", "hace", "esto", "eso", "vez",
];

pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

pub fn tokens(s: &str) -> Vec<String> {
    fold(s)
        .split_whitespace()
        .filter(|w| w.len() > 2 && !STOP.contains(w) && !STOP_EN.contains(w))
        .map(stem)
        .collect()
}

/// English plurals the light Spanish `stem` gets wrong («games» → «gam» while «game» stays,
/// «movies» → «movi»): the plural «s» alone («games» → «game»), and the spelling rules
/// («memories» → «memory», «watches» → «watch»).
fn english_stems(w: &str) -> [String; 2] {
    let plain = if w.len() > 3 { w.strip_suffix('s').filter(|b| !b.ends_with(['s', 'u'])) } else { None };
    let spelled = if w.len() > 4 && w.ends_with("ies") {
        Some(format!("{}y", &w[..w.len() - 3]))
    } else {
        w.strip_suffix("es").filter(|b| w.len() > 3 && (b.ends_with(['s', 'x', 'z']) || b.ends_with("ch") || b.ends_with("sh"))).map(String::from)
    };
    [plain.unwrap_or(w).to_string(), spelled.unwrap_or_else(|| w.to_string())]
}

/// Every form under which a folded word is matched against a keyword: the stem used everywhere,
/// and the English ones. Two words match when they share any form.
pub fn forms(w: &str) -> [String; 3] {
    let [plain, spelled] = english_stems(w);
    [stem(w), plain, spelled]
}

/// The forms of every content word of `s` (stopwords and short words left out).
pub fn form_set(s: &str) -> std::collections::HashSet<String> {
    fold(s)
        .split_whitespace()
        .filter(|w| w.len() > 2 && !STOP.contains(w) && !STOP_EN.contains(w))
        .flat_map(forms)
        .collect()
}

/// Very light stemming so "espadas" ~ "espada", "lobos" ~ "lobo".
fn stem(w: &str) -> String {
    let w = w.trim_end_matches("mente");
    for suf in ["es", "s"] {
        if w.len() > 4 {
            if let Some(b) = w.strip_suffix(suf) {
                return b.to_string();
            }
        }
    }
    w.to_string()
}

/// Whole-phrase match of `needle` inside folded `hay` (word boundaries).
pub fn contains_phrase(hay_folded: &str, needle: &str) -> bool {
    let n = fold(needle);
    let n = n.trim();
    if n.len() < 3 {
        return false;
    }
    let padded = format!(" {} ", hay_folded.split_whitespace().collect::<Vec<_>>().join(" "));
    padded.contains(&format!(" {} ", n.split_whitespace().collect::<Vec<_>>().join(" ")))
}

/// How well claim `a` matches document `b`: half set-cosine, half coverage of `a` by `b`
/// (so a short claim can still match a long canon description). Duplicates ignored.
pub fn similarity(a: &[String], b: &[String]) -> f32 {
    use std::collections::HashSet;
    let a: HashSet<&String> = a.iter().collect();
    let b: HashSet<&String> = b.iter().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(&b).count() as f32;
    let cosine = inter / ((a.len() * b.len()) as f32).sqrt();
    let coverage = inter / a.len() as f32;
    0.5 * cosine + 0.5 * coverage
}

/// How alike `a` and `b` are once the `frame` words (who speaks, how it is reported) are set
/// aside, and how many words they share then. With an empty frame this is plain `similarity`
/// and no minimum of shared words applies; with one, fewer than two shared words count as 0.
pub fn bare_similarity(a: &[String], b: &[String], frame: &[String]) -> f32 {
    if frame.is_empty() {
        return similarity(a, b);
    }
    let bare = |t: &[String]| -> Vec<String> { t.iter().filter(|w| !frame.contains(w)).cloned().collect() };
    let (a, b) = (bare(a), bare(b));
    let shared = a.iter().filter(|w| b.contains(w)).collect::<std::collections::HashSet<_>>().len();
    if shared < 2 {
        return 0.0;
    }
    similarity(&a, &b)
}

pub fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Deterministic jitter in [-1, 1] from a string, so positions are stable across runs.
pub fn hash_unit(s: &str, salt: u32) -> f32 {
    let mut h: u32 = 2166136261 ^ salt;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 20001) as f32 / 10000.0 - 1.0
}
