//! Real time. Her life runs on the user's clock: the Y axis of what she lives is the real year
//! (2026.79…), and dates she is told ("el viernes a las 5") land at that exact local moment.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 = domingo.
    pub weekday: u32,
}

use crate::lang::Lang;

const WEEKDAYS: [&str; 7] = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];
const MONTHS: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
const WEEKDAYS_EN: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const YEAR_S: f64 = 31_557_600.0;

pub fn unix_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Real time as a year on her Y axis.
pub fn year_of(unix: f64) -> f64 {
    1970.0 + unix / YEAR_S
}

pub fn unix_of(year: f64) -> f64 {
    (year - 1970.0) * YEAR_S
}

#[cfg(windows)]
mod sys {
    use super::Local;
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime, TzSpecificLocalTimeToSystemTime};

    /// 100-ns ticks between 1601-01-01 and 1970-01-01.
    const EPOCH: i64 = 116_444_736_000_000_000;

    fn ft(unix: f64) -> FILETIME {
        let t = (unix * 1e7) as i64 + EPOCH;
        FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 }
    }

    pub fn to_local(unix: f64) -> Option<Local> {
        unsafe {
            let mut utc: SYSTEMTIME = std::mem::zeroed();
            let mut loc: SYSTEMTIME = std::mem::zeroed();
            if FileTimeToSystemTime(&ft(unix), &mut utc) == 0 || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut loc) == 0 {
                return None;
            }
            Some(Local { year: loc.wYear as i32, month: loc.wMonth as u32, day: loc.wDay as u32, hour: loc.wHour as u32, minute: loc.wMinute as u32, weekday: loc.wDayOfWeek as u32 })
        }
    }

    pub fn from_local(l: &Local) -> Option<f64> {
        unsafe {
            let loc = SYSTEMTIME { wYear: l.year as u16, wMonth: l.month as u16, wDayOfWeek: 0, wDay: l.day as u16, wHour: l.hour as u16, wMinute: l.minute as u16, wSecond: 0, wMilliseconds: 0 };
            let mut utc: SYSTEMTIME = std::mem::zeroed();
            let mut f: FILETIME = std::mem::zeroed();
            if TzSpecificLocalTimeToSystemTime(std::ptr::null(), &loc, &mut utc) == 0 || SystemTimeToFileTime(&utc, &mut f) == 0 {
                return None;
            }
            let t = ((f.dwHighDateTime as i64) << 32) | f.dwLowDateTime as i64;
            Some((t - EPOCH) as f64 / 1e7)
        }
    }
}

#[cfg(not(windows))]
mod sys {
    //! Without Windows the clock is UTC.
    use super::Local;

    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn to_local(unix: f64) -> Option<Local> {
        let secs = unix.floor() as i64;
        let z = secs.div_euclid(86_400) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
        let sod = secs.rem_euclid(86_400);
        Some(Local { year, month, day, hour: (sod / 3600) as u32, minute: (sod % 3600 / 60) as u32, weekday: ((secs.div_euclid(86_400) + 4).rem_euclid(7)) as u32 })
    }

    pub fn from_local(l: &Local) -> Option<f64> {
        let days = days_from_civil(l.year as i64, l.month as i64, l.day as i64);
        Some((days * 86_400 + l.hour as i64 * 3600 + l.minute as i64 * 60) as f64)
    }
}

pub fn to_local(unix: f64) -> Option<Local> {
    sys::to_local(unix)
}

pub fn from_local(l: &Local) -> Option<f64> {
    sys::from_local(l)
}

pub fn now_local() -> Option<Local> {
    to_local(unix_now())
}

/// "domingo 19 de octubre de 2026, 00:42" ("Sunday, October 19, 2026, 00:42" in English);
/// midnight sharp is shown as a date only.
pub fn long(l: &Local, lang: Lang) -> String {
    let month = (l.month as usize).clamp(1, 12) - 1;
    let date = match lang {
        Lang::Es => format!("{} {} de {} de {}", WEEKDAYS[l.weekday as usize % 7], l.day, MONTHS[month], l.year),
        Lang::En => format!("{}, {} {}, {}", WEEKDAYS_EN[l.weekday as usize % 7], MONTHS_EN[month], l.day, l.year),
    };
    if l.hour == 0 && l.minute == 0 {
        date
    } else {
        format!("{date}, {:02}:{:02}", l.hour, l.minute)
    }
}

/// "2026-10-19 00:42": the format the extractor answers in.
pub fn iso(l: &Local) -> String {
    format!("{:04}-{:02}-{:02} {:02}:{:02}", l.year, l.month, l.day, l.hour, l.minute)
}

/// Reads "2026-10-24", "2026-10-24 17:00" or "2026-10-24T17:00[:00]" as a local moment. A date
/// without an hour is that day at 00:00.
pub fn parse(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date, time) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut d = date.split('-').map(|x| x.trim().parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || !(1900..=3000).contains(&year) {
        return None;
    }
    let (hour, minute) = match time {
        Some(t) if !t.is_empty() => {
            let mut p = t.split(':').map(|x| x.trim().parse::<i64>().ok());
            (p.next()??, p.next().flatten().unwrap_or(0))
        }
        _ => (0, 0),
    };
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) {
        return None;
    }
    from_local(&Local { year: year as i32, month: month as u32, day: day as u32, hour: hour as u32, minute: minute as u32, weekday: 0 })
}

/// "unos segundos", "4 minutos", "2 horas", "3 días" ("a few seconds", "4 minutes"…).
pub fn span(secs: f64, lang: Lang) -> String {
    let unit = |n: f64, one: &str, many: &str| {
        let n = n.round() as i64;
        if n == 1 {
            one.to_string()
        } else {
            format!("{n} {many}")
        }
    };
    let (few, minute, hour, day, month) = match lang {
        Lang::Es => ("unos segundos", ("un minuto", "minutos"), ("una hora", "horas"), ("un día", "días"), ("un mes", "meses")),
        Lang::En => ("a few seconds", ("a minute", "minutes"), ("an hour", "hours"), ("a day", "days"), ("a month", "months")),
    };
    match secs.abs() {
        s if s < 60.0 => few.into(),
        s if s < 3600.0 => unit(s / 60.0, minute.0, minute.1),
        s if s < 86_400.0 => unit(s / 3600.0, hour.0, hour.1),
        s if s < 60.0 * 86_400.0 => unit(s / 86_400.0, day.0, day.1),
        s => unit(s / (30.44 * 86_400.0), month.0, month.1),
    }
}

/// When something on her real timeline is, relative to now: "hace 2 horas", "dentro de 3 días
/// (viernes 24 de octubre de 2026, 17:00)" ("2 hours ago", "in 3 days: Friday, October 24, 2026, 17:00").
pub fn when(unix: f64, now: f64, exact: bool, lang: Lang) -> String {
    let d = unix - now;
    let rel = match (lang, d >= 0.0) {
        (Lang::Es, true) => format!("dentro de {}", span(d, lang)),
        (Lang::Es, false) => format!("hace {}", span(d, lang)),
        (Lang::En, true) => format!("in {}", span(d, lang)),
        (Lang::En, false) => format!("{} ago", span(d, lang)),
    };
    match (exact, to_local(unix)) {
        (true, Some(l)) => format!("{rel}: {}", long(&l, lang)),
        _ => rel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_dates_roundtrip() {
        let t = parse("2026-10-24 17:30").unwrap();
        let l = to_local(t).unwrap();
        assert_eq!((l.year, l.month, l.day, l.hour, l.minute), (2026, 10, 24, 17, 30));
        assert_eq!(l.weekday, 6, "the 24th of October 2026 is a Saturday");
        assert_eq!(long(&l, Lang::Es), "sábado 24 de octubre de 2026, 17:30");
        assert_eq!(long(&l, Lang::En), "Saturday, October 24, 2026, 17:30");
        assert_eq!(parse("2026-10-24T17:30:00"), Some(t));
        let day = to_local(parse("2026-12-01").unwrap()).unwrap();
        assert_eq!(long(&day, Lang::Es), "martes 1 de diciembre de 2026");
        assert_eq!(long(&day, Lang::En), "Tuesday, December 1, 2026");
        assert_eq!(parse("mañana"), None);
        assert_eq!(parse("2026-13-01"), None);
        assert!((unix_of(year_of(t)) - t).abs() < 1e-3);
    }

    #[test]
    fn relative_words() {
        assert_eq!(span(90.0, Lang::Es), "2 minutos");
        assert_eq!(when(1000.0 + 3.0 * 86_400.0, 1000.0, false, Lang::Es), "dentro de 3 días");
        assert_eq!(when(1000.0 - 7200.0, 1000.0, false, Lang::Es), "hace 2 horas");
    }

    #[test]
    fn relative_words_in_english() {
        assert_eq!(span(5.0, Lang::En), "a few seconds");
        assert_eq!(span(60.0, Lang::En), "a minute");
        assert_eq!(span(90.0, Lang::En), "2 minutes");
        assert_eq!(span(3600.0, Lang::En), "an hour");
        assert_eq!(span(86_400.0, Lang::En), "a day");
        assert_eq!(span(90.0 * 86_400.0, Lang::En), "3 months");
        assert_eq!(when(1000.0 + 3.0 * 86_400.0, 1000.0, false, Lang::En), "in 3 days");
        assert_eq!(when(1000.0 - 7200.0, 1000.0, false, Lang::En), "2 hours ago");
        assert_eq!(when(1000.0 - 40.0, 1000.0, false, Lang::En), "a few seconds ago");
        let t = parse("2026-10-24 17:30").unwrap();
        assert_eq!(when(t, t - 3.0 * 86_400.0, true, Lang::En), "in 3 days: Saturday, October 24, 2026, 17:30");
        assert_eq!(when(t, t - 3.0 * 86_400.0, true, Lang::Es), "dentro de 3 días: sábado 24 de octubre de 2026, 17:30");
    }
}
