//! Text for people: each answer as one string of lines. Styles are always written as ANSI
//! codes; the output stream drops them unless it is a colour terminal and NO_COLOR is unset.

use std::fmt::Write;

use jiff::civil::Date;

use crate::calendars::Calendar;
use crate::days::Warning;
use crate::error::Error;
use crate::input::DAYS;
use crate::report::{
    Added, Answer, Between, Converted, Countries, Country, Data, Holiday, Holidays, Hours, Named,
};

/// What the tool is for, in one sentence: the help's first line and the banner's second.
pub const SENTENCE: &str = "Count business days, add workdays, list holidays, convert dates.";

pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
pub const RESET: &str = "\x1b[0m";

/// Rows listed before `… and N more`.
const ROWS: usize = 10;

pub fn answer(a: &Answer) -> String {
    match a {
        Answer::Between(b) => between(b),
        Answer::Add(a) => added(a),
        Answer::Hours(h) => hours(h),
        Answer::Holidays(h) => holidays(h),
        Answer::Convert(c) => converted(c),
        Answer::Countries(c) => countries(c),
        Answer::Country(c) => country(c),
    }
}

/// `Mon 2026-02-16`.
fn day(d: Date) -> String {
    format!("{} {d}", DAYS[d.weekday().to_monday_zero_offset() as usize])
}

/// A date or date-time as JSON writes it, for people: `Tue 2026-11-03 11:00 -07:00`.
fn when(text: &str) -> String {
    let mut out = day(text[..10].parse().expect("an answer's own date"));
    if let Some(time) = text.get(11..19) {
        out += " ";
        out += time.strip_suffix(":00").unwrap_or(time);
        if let Some(offset) = text.get(19..).filter(|o| !o.is_empty()) {
            out += " ";
            out += offset;
        }
    }
    out
}

/// `1 day`, `-16 business days`.
fn count(n: i64, word: &str) -> String {
    let s = if n.abs() == 1 { "" } else { "s" };
    format!("{n} {word}{s}")
}

/// `China`, `Germany, Bayern`, or `No country`.
fn place(country: Option<&Named>, region: Option<&Named>) -> String {
    match (country, region) {
        (Some(c), Some(r)) => format!("{}, {}", c.name, r.name),
        (Some(c), None) => c.name.into(),
        _ => "No country".into(),
    }
}

fn flags(h: &Holiday) -> String {
    let mut out = String::new();
    for (on, flag) in [
        (h.observed, "observed"),
        (h.estimated, "estimated"),
        (h.scope == Some("regional"), "regional"),
    ] {
        if on {
            out += &format!(" ({flag})");
        }
    }
    if !h.except_regions.is_empty() {
        out += &format!(" (not in {})", h.except_regions.join(", "));
    }
    match out.as_str() {
        "" => out,
        _ => format!("{DIM}{out}{RESET}"),
    }
}

fn holiday_rows(holidays: &[Holiday]) -> String {
    let row = |h: &Holiday| format!("  {}  {}{}\n", day(h.date), h.name, flags(h));
    holidays.iter().map(row).collect()
}

fn worked(dates: &[Date]) -> String {
    if dates.is_empty() {
        return String::new();
    }
    let days: Vec<_> = dates.iter().map(|&d| day(d)).collect();
    format!("Worked weekend days: {}\n", days.join(", "))
}

fn data_line(text: &str) -> String {
    format!("{DIM}{text}{RESET}\n")
}

fn between(b: &Between) -> String {
    let ends = match (b.include_start, b.include_end) {
        (true, true) => "both counted",
        (false, true) => "start not counted",
        (true, false) => "end not counted",
        (false, false) => "ends not counted",
    };
    format!(
        "{BOLD}{}{RESET}\n{DIM}{} to {} ({ends}) · {}\n{}: {} weekend, {} · {} apart{RESET}\n",
        count(b.business_days, "business day"),
        b.start,
        b.end,
        place(b.country.as_ref(), b.region.as_ref()),
        count(b.calendar_days, "day"),
        b.weekend_days,
        count(b.holiday_days, "holiday"),
        count(b.elapsed_days.abs(), "day"),
    ) + &holiday_rows(&b.holidays)
        + &worked(&b.weekend_workdays)
        + &warnings(&b.warnings)
        + &data_line(&b.data.holidays)
}

fn added(a: &Added) -> String {
    let amount = a.amount.as_f64().expect("a number").abs();
    let unit = a.unit.trim_end_matches('s').replace('_', " ");
    let amount_text = a.amount.to_string();
    let s = if amount == 1.0 { "" } else { "s" };
    let direction = if amount_text.starts_with('-') {
        "before"
    } else {
        "after"
    };
    let mut out = format!(
        "{BOLD}{}{RESET}\n{DIM}{} {unit}{s} {direction} {} · {}{RESET}\n",
        when(&a.result),
        amount_text.trim_start_matches('-'),
        when(&a.start),
        place(a.country.as_ref(), a.region.as_ref()),
    );
    if let Some(c) = &a.clock {
        out += &format!(
            "{DIM}Hours {} · {}{RESET}\n",
            hours_text(&c.hours),
            zone(&c.timezone)
        );
    }
    let total = a.skipped.len() + a.skipped_more;
    if total > 0 {
        let days = count(total as i64, "day");
        out += &match a.roll {
            "none" => format!("Skipped {days}:\n"),
            roll => format!("Rolled {} past {days}:\n", roll.replace('_', "-")),
        };
        for s in a.skipped.iter().take(ROWS) {
            let _ = writeln!(out, "  {}  {}", day(s.date), s.name.unwrap_or(s.reason));
        }
        if total > ROWS {
            let _ = writeln!(out, "  … and {} more", total - ROWS);
        }
    }
    let zone = a.clock.as_ref().and_then(|c| c.timezone.as_ref());
    out + &worked(&a.weekend_workdays) + &warnings(&a.warnings) + &data_line(&data(&a.data, zone))
}

/// The data line of an answer that may use time zones.
fn data(d: &Data, zone: Option<&String>) -> String {
    match zone {
        Some(_) => format!("{} · tzdata {}", d.holidays, d.tzdata),
        None => d.holidays.clone(),
    }
}

/// `09:00-12:00, 13:00-17:00`.
fn hours_text(hours: &str) -> String {
    hours.replace(',', ", ")
}

fn zone(timezone: &Option<String>) -> &str {
    timezone.as_deref().unwrap_or("clock time")
}

fn hours(h: &Hours) -> String {
    format!(
        "{BOLD}{} business hours ({:.2} h){RESET}\n{DIM}{} to {} · {}\nHours {} on {} · {}{RESET}\n",
        h.duration,
        h.business_hours,
        when(&h.start),
        when(&h.end),
        place(h.country.as_ref(), h.region.as_ref()),
        hours_text(&h.clock.hours),
        count(h.business_days, "business day"),
        zone(&h.clock.timezone),
    ) + &holiday_rows(&h.holidays)
        + &worked(&h.weekend_workdays)
        + &warnings(&h.warnings)
        + &data_line(&data(&h.data, h.clock.timezone.as_ref()))
}

fn holidays(h: &Holidays) -> String {
    let (y0, y1) = (h.from.year(), h.to.year());
    let whole_year = y0 == y1 && h.from.day_of_year() == 1 && h.to == h.to.last_of_year();
    let period = if whole_year {
        y0.to_string()
    } else {
        format!("{} to {}", h.from, h.to)
    };
    let n = h.holidays.len() as i64;
    let total = match h.region {
        Some(_) => count(n, "holiday"),
        None => count(n, "national holiday"),
    };
    let mut out = format!(
        "{BOLD}{} · {period} · {total}{RESET}\n",
        place(Some(&h.country), h.region.as_ref())
    ) + &holiday_rows(&h.holidays);
    if !h.regional.is_empty() {
        out += "Regional:\n";
        let width = h
            .regional
            .iter()
            .map(|g| g.name.chars().count())
            .max()
            .unwrap_or(0)
            + 2;
        for g in &h.regional {
            let pad = width - g.name.chars().count();
            let _ = writeln!(
                out,
                "  {}  {}{}{}{DIM}{}{RESET}",
                day(g.date),
                g.name,
                flags(g),
                " ".repeat(pad),
                g.regions.join(", ")
            );
        }
    }
    out + &worked(&h.weekend_workdays) + &warnings(&h.warnings) + &data_line(&h.data.holidays)
}

fn converted(c: &Converted) -> String {
    let title = |id: &str| {
        Calendar::parse(id)
            .expect("an answer's own calendar")
            .title()
    };
    let titles: Vec<_> = c.dates.iter().map(|d| title(d.calendar)).collect();
    let width = titles
        .iter()
        .chain(&["Gregorian"])
        .map(|t| t.len())
        .max()
        .unwrap_or(0)
        + 2;
    let g = &c.gregorian;
    let mut out = format!(
        "{BOLD}{:width$}{} · week {} · day {}{RESET}\n",
        "Gregorian",
        day(g.date),
        g.iso_week,
        g.day_of_year
    );
    for (d, title) in c.dates.iter().zip(titles) {
        let _ = writeln!(out, "{title:width$}{}", d.text);
        if let Some(note) = d.note {
            let _ = writeln!(out, "{:width$}{DIM}{note}{RESET}", "");
        }
    }
    out + &warnings(&c.warnings)
        + &data_line(&format!("{} · {}", c.data.calendars, c.data.bikram_sambat))
}

/// `["SAT", "SUN"]` as `Sat, Sun`, or `none`.
fn days_text(days: &[&str]) -> String {
    let days: Vec<_> = days
        .iter()
        .map(|d| d[..1].to_string() + &d[1..].to_lowercase())
        .collect();
    if days.is_empty() {
        "none".into()
    } else {
        days.join(", ")
    }
}

fn countries(c: &Countries) -> String {
    let width = c
        .countries
        .iter()
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let mut out = String::new();
    for c in &c.countries {
        let _ = write!(
            out,
            "{:4}{:width$}weekend {} · {}-{}",
            c.code,
            c.name,
            days_text(&c.weekend),
            c.years[0],
            c.years[1]
        );
        if c.region_count > 0 {
            let _ = write!(out, " · {}", count(c.region_count as i64, "region"));
        }
        if let Some(y) = c.decree_through {
            let _ = write!(out, " · decree data to {y}");
        }
        out.push('\n');
    }
    out + &data_line(&c.data.holidays)
}

fn country(c: &Country) -> String {
    let i = &c.country;
    let codes = [i.code].into_iter().chain(i.aliases.iter().copied());
    let mut out = format!(
        "{BOLD}{} ({}) · {}-{}",
        i.name,
        codes.collect::<Vec<_>>().join(", "),
        i.years[0],
        i.years[1]
    );
    if i.region_count > 0 {
        out += &format!(" · {}", count(i.region_count as i64, "region"));
    }
    out += &format!("{RESET}\n");
    if !c.regions.is_empty() {
        out += "Regions:\n";
        let width = c
            .regions
            .iter()
            .map(|r| r.code.chars().count())
            .max()
            .unwrap_or(0);
        for r in &c.regions {
            let line = format!("  {:width$}  {}", r.code, r.names.join(", "));
            let _ = writeln!(out, "{}", line.trim_end());
        }
    }
    out += "Weekend:\n";
    for w in &c.weekend_history {
        let region = w.region.map(|r| format!("  ({r})")).unwrap_or_default();
        let _ = writeln!(out, "  from {}  {}{region}", w.from, days_text(&w.weekend));
    }
    out + &data_line(&c.data.holidays)
}

/// What failed, then how to fix it.
pub fn error(e: &Error) -> String {
    let mut out = format!("{RED}✗{RESET} {}\n", e.message);
    if let Some(hint) = &e.hint {
        let _ = writeln!(out, "  {hint}");
    }
    out
}

pub fn warnings(warnings: &[Warning]) -> String {
    warnings
        .iter()
        .map(|w| format!("{YELLOW}!{RESET} {}\n", w.message))
        .collect()
}

/// The text with plain ASCII in place of the marks a console without UTF-8 can't show.
pub fn ascii(text: &str) -> String {
    text.replace(['╭', '╮', '╰', '╯'], "+")
        .replace(['─', '·'], "-")
        .replace('│', "|")
        .replace('✻', "*")
        .replace('›', ">")
        .replace('✗', "x")
        .replace('…', "...")
}

/// Whether a stream shows Unicode: always when it isn't a terminal; on a Windows console only
/// when its output code page is UTF-8.
pub fn unicode(terminal: bool) -> bool {
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn GetConsoleOutputCP() -> u32;
        }
        // SAFETY: takes no arguments and only reads the console's state.
        !terminal || unsafe { GetConsoleOutputCP() } == 65001
    }
    #[cfg(not(windows))]
    {
        let _ = terminal;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_dates_and_times_for_people() {
        assert_eq!(when("2026-02-27"), "Fri 2026-02-27");
        assert_eq!(when("2026-11-03T11:00:00"), "Tue 2026-11-03 11:00");
        assert_eq!(
            when("2026-11-03T11:00:30-07:00"),
            "Tue 2026-11-03 11:00:30 -07:00"
        );
        assert_eq!(count(-1, "business day"), "-1 business day");
        assert_eq!(count(0, "holiday"), "0 holidays");
        assert_eq!(days_text(&["FRI", "SUN"]), "Fri, Sun");
        assert_eq!(days_text(&[]), "none");
    }

    #[test]
    fn marks_errors_and_warnings() {
        let e = Error::new("bad_date", "Not a date: 'x'.").hint("Use YYYY-MM-DD.");
        assert_eq!(
            error(&e),
            "\x1b[31m✗\x1b[0m Not a date: 'x'.\n  Use YYYY-MM-DD.\n"
        );
        let w = Warning {
            code: "no_country",
            message: "No country.".into(),
        };
        assert_eq!(warnings(&[w]), "\x1b[33m!\x1b[0m No country.\n");
        assert_eq!(ascii("✗ a · b … ›"), "x a - b ... >");
        assert!(unicode(false));
    }
}
