//! Plans asked for in so many words ("agéndame una cita el domingo a las 12 PM, llámala final
//! del semestre"): when and what, read from the text itself. The analyst model is told the date
//! and asked for it too, but a small one often leaves it out (or renames the plan); this does
//! not depend on it.

use crate::clock::{self, Local};

/// Lowercase, without accents, "p. m." as "pm": words to match against.
fn plain(text: &str) -> String {
    let s: String = text
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            'ñ' => 'n',
            _ => c,
        })
        .collect();
    s.replace("p. m.", "pm").replace("a. m.", "am").replace("p.m.", "pm").replace("a.m.", "am")
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == ':' || c == '/'))
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

const DAYS: [[&str; 2]; 7] = [["domingo", "sunday"], ["lunes", "monday"], ["martes", "tuesday"], ["miercoles", "wednesday"], ["jueves", "thursday"], ["viernes", "friday"], ["sabado", "saturday"]];
const MONTHS: [[&str; 2]; 12] = [
    ["enero", "january"],
    ["febrero", "february"],
    ["marzo", "march"],
    ["abril", "april"],
    ["mayo", "may"],
    ["junio", "june"],
    ["julio", "july"],
    ["agosto", "august"],
    ["septiembre", "september"],
    ["octubre", "october"],
    ["noviembre", "november"],
    ["diciembre", "december"],
];

fn month_of(w: &str) -> Option<u32> {
    MONTHS.iter().position(|m| m.contains(&w) || (w.len() >= 3 && m[1].starts_with(w) && w != "mar")).map(|i| i as u32 + 1)
}

/// The hour said ("a las 12 pm", "5:30 de la tarde", "17:00", "al mediodia", "at noon").
fn time_of(w: &[String]) -> Option<(u32, u32)> {
    if w.iter().any(|x| x == "mediodia" || x == "noon") {
        return Some((12, 0));
    }
    if w.iter().any(|x| x == "medianoche" || x == "midnight") {
        return Some((0, 0));
    }
    for (i, x) in w.iter().enumerate() {
        // "5pm", "12am"
        let (num, glued) = match x.strip_suffix("pm").or_else(|| x.strip_suffix("am")) {
            Some(n) if !n.is_empty() => (n, Some(x.ends_with("pm"))),
            _ => (x.as_str(), None),
        };
        let (h, m) = match num.split_once(':') {
            Some((h, m)) => (h.parse::<u32>().ok(), m.parse::<u32>().ok()),
            None => (num.parse::<u32>().ok(), Some(0)),
        };
        let (Some(h), Some(m)) = (h, m) else { continue };
        let prev = if i > 0 { w[i - 1].as_str() } else { "" };
        let next: Vec<&str> = w.iter().skip(i + 1).take(3).map(String::as_str).collect();
        let clock_like = num.contains(':') || glued.is_some() || matches!(prev, "las" | "la" | "at") || next.first().is_some_and(|n| matches!(*n, "pm" | "am" | "hrs" | "h" | "horas"));
        if !clock_like || h > 23 || m > 59 {
            continue;
        }
        let pm = glued.or_else(|| match next.first().copied() {
            Some("pm") => Some(true),
            Some("am") => Some(false),
            _ if next.windows(2).any(|p| p == ["la", "tarde"] || p == ["la", "noche"]) => Some(true),
            _ if next.windows(2).any(|p| p == ["la", "manana"]) => Some(false),
            _ => None,
        });
        let h = match pm {
            Some(true) if h < 12 => h + 12,
            Some(false) if h == 12 => 0,
            // "a las 5" for a plan is the afternoon.
            None if (1..=7).contains(&h) && !num.contains(':') => h + 12,
            _ => h,
        };
        return Some((h, m));
    }
    None
}

fn add_days(l: &Local, n: i64) -> Local {
    let noon = Local { hour: 12, minute: 0, ..*l };
    let u = clock::from_local(&noon).unwrap_or(0.0) + n as f64 * 86_400.0;
    clock::to_local(u).unwrap_or(*l)
}

/// When a plan in `text` is, from `now`: a day ("hoy", "mañana", "el domingo", "el 15 de
/// noviembre", "15/11", "next friday") and/or an hour. Only an hour: today, or tomorrow if it
/// has passed. Neither: none.
pub fn find(text: &str, now: &Local) -> Option<Local> {
    let w = words(&plain(text));
    let time = time_of(&w);
    let has = |x: &str| w.iter().any(|y| y == x);
    let mut day: Option<Local> = None;
    // An explicit date first.
    for (i, x) in w.iter().enumerate() {
        if let Some(n) = x.parse::<u32>().ok().filter(|n| (1..=31).contains(n)) {
            let m = match (w.get(i + 1).map(String::as_str), w.get(i + 2)) {
                (Some("de"), Some(m)) => month_of(m),
                (Some(m), _) => month_of(m),
                _ => None,
            };
            if let Some(m) = m {
                day = Some(Local { month: m, day: n, ..*now });
                break;
            }
        }
        if let Some(m) = month_of(x) {
            if let Some(n) = w.get(i + 1).and_then(|n| n.trim_end_matches(|c: char| c.is_alphabetic()).parse::<u32>().ok()).filter(|n| (1..=31).contains(n)) {
                day = Some(Local { month: m, day: n, ..*now });
                break;
            }
        }
        if let Some((d, m)) = x.split_once('/') {
            let m = m.split('/').next().unwrap_or("");
            if let (Ok(d), Ok(m)) = (d.parse::<u32>(), m.parse::<u32>()) {
                if (1..=31).contains(&d) && (1..=12).contains(&m) {
                    day = Some(Local { month: m, day: d, ..*now });
                    break;
                }
            }
        }
    }
    if let Some(d) = day.as_mut() {
        // A date already gone this year is next year's.
        if (d.month, d.day) < (now.month, now.day) {
            d.year += 1;
        }
    }
    if day.is_none() {
        let tomorrow = w.iter().enumerate().any(|(i, x)| (x == "manana" && !(i > 0 && w[i - 1] == "la") && !(i > 0 && w[i - 1] == "pasado")) || x == "tomorrow");
        if has("pasado") && has("manana") {
            day = Some(add_days(now, 2));
        } else if tomorrow {
            day = Some(add_days(now, 1));
        } else if has("hoy") || has("today") || has("tonight") {
            day = Some(*now);
        } else if let Some(wd) = DAYS.iter().position(|d| w.iter().any(|x| d.contains(&x.as_str()))) {
            let mut delta = (wd as i64 - now.weekday as i64).rem_euclid(7);
            let later_today = time.is_some_and(|(h, m)| (h, m) > (now.hour, now.minute));
            if delta == 0 && !later_today {
                delta = 7;
            }
            day = Some(add_days(now, delta));
        }
    }
    let (h, m) = match (day, time) {
        (None, None) => return None,
        (_, Some(t)) => t,
        (Some(_), None) => (0, 0),
    };
    let mut d = day.unwrap_or(*now);
    if day.is_none() && (h, m) <= (now.hour, now.minute) {
        d = add_days(now, 1);
    }
    let l = Local { hour: h, minute: m, ..d };
    // Its weekday, from the date.
    clock::from_local(&l).and_then(clock::to_local).map(|x| Local { hour: h, minute: m, ..x })
}

const ASK: [&str; 18] = ["agend", "anota", "anotame", "apunta", "apuntame", "recuerdame", "recordarme", "recordatorio", "programa", "programame", "reserva", "agrega", "anade", "schedule", "remind", "book", "calendar", "agenda"];
const NAMED: [&str; 12] = ["llamala", "llamalo", "llamale", "ponle", "con el nombre", "que se llame", "titulada", "titulado", "call it", "name it", "titled", "called"];
/// Where the plan's own words end and its date begins.
const STOP: [&str; 16] = ["el", "la", "las", "para", "a", "este", "esta", "hoy", "manana", "pasado", "on", "at", "for", "this", "today", "tomorrow"];

/// The plan's name in a request to schedule something: the name it is given ("llámala final
/// del semestre"), or what is asked for ("una cita", "una reunión con Pedro"). None: the text
/// does not ask to schedule anything.
pub fn request(text: &str) -> Option<String> {
    let p = plain(text);
    let w = words(&p);
    if !w.iter().any(|x| ASK.iter().any(|a| x.starts_with(a))) {
        return None;
    }
    let cap = |s: &str| {
        let s = s.trim().trim_matches(|c: char| "«»\"'“”.,;:!?¿¡".contains(c)).trim();
        let mut c = s.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    };
    // Named: the original words after the marker, up to the end of the clause.
    for n in NAMED {
        if let Some(at) = p.find(n) {
            // Same byte offsets: `plain` only changes letters of the same length… except accents,
            // so the original is cut by characters.
            let skip = p[..at + n.len()].chars().count();
            let rest: String = text.chars().skip(skip).take_while(|c| !".;!?\n".contains(*c)).collect();
            let name = cap(rest.trim().trim_start_matches([':', ',']));
            if !name.is_empty() {
                return Some(name.chars().take(80).collect());
            }
        }
    }
    // A name in quotes.
    for (open, close) in [('«', '»'), ('“', '”'), ('"', '"')] {
        if let Some((_, rest)) = text.split_once(open) {
            if let Some((name, _)) = rest.split_once(close) {
                let name = cap(name);
                if !name.is_empty() {
                    return Some(name.chars().take(80).collect());
                }
            }
        }
    }
    // Otherwise what comes after the article, up to the date (in the user's own words).
    let orig = words(&text.replace("p. m.", "pm").replace("a. m.", "am").replace("p.m.", "pm").replace("a.m.", "am"));
    for (i, x) in w.iter().enumerate() {
        if matches!(x.as_str(), "una" | "un" | "a" | "an") && i > 0 && w[..i].iter().any(|y| ASK.iter().any(|a| y.starts_with(a))) {
            let take: Vec<&str> = (i + 1..w.len()).take_while(|j| !STOP.contains(&w[*j].as_str()) && !DAYS.iter().any(|d| d.contains(&w[*j].as_str())) && w[*j].parse::<u32>().is_err()).take(6).map(|j| orig.get(j).map(String::as_str).unwrap_or("")).collect();
            if !take.is_empty() {
                return Some(cap(&take.join(" ")));
            }
        }
    }
    Some(if w.iter().any(|x| ["remind", "record", "recuerd"].iter().any(|r| x.starts_with(r))) { "Recordatorio".into() } else { "Cita".into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wednesday 4 November 2026, 20:30.
    fn now() -> Local {
        Local { year: 2026, month: 11, day: 4, hour: 20, minute: 30, weekday: 3 }
    }

    fn at(t: &str) -> Option<(i32, u32, u32, u32, u32, u32)> {
        find(t, &now()).map(|l| (l.year, l.month, l.day, l.hour, l.minute, l.weekday))
    }

    #[test]
    fn the_day_and_hour_are_read() {
        assert_eq!(at("Puedes agendarme una cita el domingo a las 12 PM"), Some((2026, 11, 8, 12, 0, 0)));
        assert_eq!(at("mañana a las 5"), Some((2026, 11, 5, 17, 0, 4)), "a las 5: the afternoon");
        assert_eq!(at("mañana a las 10 de la noche"), Some((2026, 11, 5, 22, 0, 4)), "tomorrow, not by the hour");
        assert_eq!(at("pasado mañana a las 9 de la mañana"), Some((2026, 11, 6, 9, 0, 5)));
        assert_eq!(at("el viernes a las 7:30 de la tarde"), Some((2026, 11, 6, 19, 30, 5)));
        assert_eq!(at("el 15 de noviembre a las 10 am"), Some((2026, 11, 15, 10, 0, 0)));
        assert_eq!(at("el 2 de enero"), Some((2027, 1, 2, 0, 0, 6)), "gone this year: next year");
        assert_eq!(at("on December 24 at 9pm"), Some((2026, 12, 24, 21, 0, 4)));
        assert_eq!(at("dentista 20/11 17:00"), Some((2026, 11, 20, 17, 0, 5)));
        assert_eq!(at("next sunday at noon"), Some((2026, 11, 8, 12, 0, 0)));
        assert_eq!(at("el miércoles a las 9 pm"), Some((2026, 11, 4, 21, 0, 3)), "today, later");
        assert_eq!(at("el miércoles a las 8 am"), Some((2026, 11, 11, 8, 0, 3)), "today, gone: next week");
        assert_eq!(at("a las 21:00"), Some((2026, 11, 4, 21, 0, 3)));
        assert_eq!(at("a las 10 de la mañana"), Some((2026, 11, 5, 10, 0, 4)), "an hour gone today: tomorrow");
        assert_eq!(at("me gusta la mañana"), None);
        assert_eq!(at("tengo 3 gatos"), None);
    }

    #[test]
    fn the_plan_gets_its_name() {
        assert_eq!(request("Puedes agendarme una cita el domingo a las 12 PM, llámala final del semestre").as_deref(), Some("Final del semestre"));
        assert_eq!(request("agéndame una reunión con Pedro el lunes a las 3").as_deref(), Some("Reunión con Pedro"));
        assert_eq!(request("anota «Dentista» para mañana").as_deref(), Some("Dentista"), "a quoted name is not 'que se llame'");
        assert_eq!(request("Schedule a call with the bank tomorrow at 10, call it Bank call").as_deref(), Some("Bank call"));
        assert_eq!(request("recuérdame mañana a las 8").as_deref(), Some("Recordatorio"));
        assert_eq!(request("el domingo fui al cine"), None, "not a request");
    }
}
