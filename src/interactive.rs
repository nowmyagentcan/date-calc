//! The no-argument mode: each typed line is a query or a slash command.

use std::io::{self, BufRead, Write};

use crate::days::Place;
use crate::error::Error;
use crate::report::{self, DEFAULT_HOURS, Options, TZDATA};
use crate::text::{self, BOLD, DIM, RESET, SENTENCE};
use crate::{input, query};

/// Shows the banner, then answers each line from `input` until `/quit` or the end of input.
/// `unicode` false swaps the box and marks for ASCII.
pub fn run(mut input: impl BufRead, out: &mut impl Write, unicode: bool) -> io::Result<()> {
    let mut send = |text: String| {
        let text = if unicode { text } else { text::ascii(&text) };
        out.write_all(text.as_bytes())?;
        out.flush()
    };
    let mut session = Options::default();
    send(banner())?;
    let mut line = vec![];
    loop {
        send(prompt(&session))?;
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            return send("\n".into());
        }
        match String::from_utf8_lossy(&line).trim() {
            "/quit" => return Ok(()),
            "" => {}
            typed => send(reply(&mut session, typed) + "\n")?,
        }
    }
}

fn banner() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let name = format!("✻ date-calc {version}");
    let sentence = format!("  {SENTENCE}");
    let width = name.chars().count().max(sentence.chars().count());
    let row = |plain: &str, styled: String| {
        let pad = " ".repeat(width - plain.chars().count());
        format!("{DIM}│{RESET} {styled}{pad} {DIM}│{RESET}\n")
    };
    let border = "─".repeat(width + 2);
    format!("{DIM}╭{border}╮{RESET}\n")
        + &row(&name, format!("{BOLD}✻ date-calc{RESET} {version}"))
        + &row(&sentence, sentence.clone())
        + &format!("{DIM}╰{border}╯{RESET}\n")
        + &format!("{DIM}  Type dates, a year or a calendar date · /country · /help · /quit\n")
        + &format!("  No country yet, so only weekends are skipped. Try /country DE{RESET}\n\n")
}

/// `› `, or `CN › ` and `DE-BY › ` with a country.
fn prompt(o: &Options) -> String {
    let place = match (&o.country, &o.region) {
        (Some(c), Some(r)) => format!("{c}-{r} "),
        (Some(c), None) => format!("{c} "),
        _ => String::new(),
    };
    format!("{BOLD}{place}›{RESET} ")
}

fn reply(o: &mut Options, typed: &str) -> String {
    answer(o, typed).unwrap_or_else(|e| indent(&text::error(&e)))
}

fn answer(o: &mut Options, typed: &str) -> Result<String, Error> {
    let (command, arg) = typed
        .split_once(' ')
        .map_or((typed, ""), |(c, a)| (c, a.trim()));
    let line = match command {
        "/country" if arg.is_empty() => {
            (o.country, o.region) = (None, None);
            "No country: only weekends are skipped.".into()
        }
        "/country" => {
            let place = Place::new(Some(arg), None, None)?;
            let b = place.block.as_ref().expect("a country");
            o.country = Some(b.country.code.into());
            o.region = place.region.map(|r| b.regions[r].code.into());
            describe(&place)
        }
        "/region" => {
            let Some(country) = &o.country else {
                return Err(Error::new("usage", "No country set yet.")
                    .hint("Pick one first, e.g. /country DE."));
            };
            if arg.is_empty() {
                o.region = None;
                "Region: whole country".into()
            } else {
                let place = Place::new(Some(country), Some(arg), None)?;
                let (code, name) = region(&place).expect("a region");
                o.region = Some(code.into());
                format!("Region: {name} ({code})")
            }
        }
        "/tz" if arg.is_empty() => {
            o.timezone = None;
            "Time zone: none (clock time)".into()
        }
        "/tz" => {
            let zone = input::timezone(arg)?;
            let name = zone.iana_name().unwrap_or("UTC");
            o.timezone = Some(name.into());
            format!("Time zone: {name} (tzdata {TZDATA})")
        }
        "/hours" => {
            let windows = input::hours(if arg.is_empty() { DEFAULT_HOURS } else { arg })?;
            let windows: Vec<_> = windows.iter().map(ToString::to_string).collect();
            o.hours = (!arg.is_empty()).then(|| windows.join(","));
            format!("Working hours: {}", windows.join(", "))
        }
        "/help" => return Ok(help(o)),
        _ if command.starts_with('/') => {
            return Err(
                Error::new("usage", format!("Unknown command '{command}'.")).hint("Try /help.")
            );
        }
        _ => {
            let answer = report::run(&query::parse(typed)?, o)?;
            return Ok(indent(&text::answer(&answer)));
        }
    };
    Ok(format!("  {line}\n"))
}

/// The chosen region's code and name.
fn region(place: &Place) -> Option<(&'static str, &'static str)> {
    let r = &place.block.as_ref()?.regions[place.region?];
    Some((r.code, r.names.first().copied().unwrap_or(r.code)))
}

/// `China (CN) · weekend Sat, Sun · holidays 2000-2050 · decree data to 2026`.
fn describe(place: &Place) -> String {
    let c = &place.block.as_ref().expect("a country").country;
    let (name, code) = match region(place) {
        Some((code, name)) => (format!("{}, {name}", c.name), code),
        None => (c.name.into(), c.code),
    };
    let weekend = input::weekend_text(place.weekend_on(jiff::Zoned::now().date()));
    let mut line = format!(
        "{name} ({code}) · weekend {weekend} · holidays {}-{}",
        c.years.0, c.years.1
    );
    if let Some(y) = c.decree_through {
        line += &format!(" · decree data to {y}");
    }
    line
}

fn help(o: &Options) -> String {
    let country = match (&o.country, &o.region) {
        (Some(c), Some(r)) => format!("{c}-{r}"),
        (Some(c), None) => c.clone(),
        _ => "none".into(),
    };
    let region = o.region.as_deref().unwrap_or("whole country");
    let zone = o.timezone.as_deref().unwrap_or("none: clock time");
    let hours = o.hours.as_deref().unwrap_or(DEFAULT_HOURS);
    format!(
        "  Type one of:
    2026-02-01 to 2026-02-28              business days between two dates
    2026-11-02 10:00 to 2026-11-05 15:00  business hours
    2026-11-02 +10                        add business days (or +3 weeks, -2 months, +4 h)
    2027                                  the holidays of the country you set
    2026-10-27                            the date in every calendar
    1 Ramadan 1448                        a date from another calendar
  /country C   country, or country and region like DE-BY (now {country})
  /region R    region of that country (now {region})
  /tz ZONE     time zone for business hours (now {zone})
  /hours H     working hours, e.g. 09:00-12:00,13:00-17:00 (now {hours})
  /quit        leave (Ctrl+C works too)
"
    )
}

fn indent(text: &str) -> String {
    text.lines().map(|l| format!("  {l}\n")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(lines: &str) -> String {
        let mut out = vec![];
        run(lines.as_bytes(), &mut out, true).unwrap();
        anstream::adapter::strip_str(&String::from_utf8(out).unwrap()).to_string()
    }

    #[test]
    fn shows_the_banner_and_answers_queries() {
        let out = session("2026-11-02 to 2026-11-06\n/quit\n");
        assert_eq!(
            out,
            "╭────────────────────────────────────────────────────────────────────╮
│ ✻ date-calc 0.1.0                                                  │
│   Count business days, add workdays, list holidays, convert dates. │
╰────────────────────────────────────────────────────────────────────╯
  Type dates, a year or a calendar date · /country · /help · /quit
  No country yet, so only weekends are skipped. Try /country DE

›   5 business days
  2026-11-02 to 2026-11-06 (both counted) · No country
  5 days: 0 weekend, 0 holidays · 4 days apart
  ! No country given, so only weekends (Sat, Sun) are skipped.
  python-holidays 0.107 (2026-10-19)

› "
        );
    }

    #[test]
    fn keeps_session_settings() {
        let out = session(
            "/country CN\n2026-02-13 +5\n2027-02-30 to 2027-03-05\n/country DE-BY\n/region\n/region BW\n/tz europe/berlin\n/hours 09:00-12:00, 13:00-17:00\n/help\n/tz\n/hours\n/country\n",
        );
        for want in [
            "›   China (CN) · weekend Sat, Sun · holidays 2000-2050 · decree data to 2026\n\nCN › ",
            "CN ›   Fri 2026-02-27\n  5 business days after Fri 2026-02-13 · China\n  Skipped 9 days:\n",
            "CN ›   ✗ Not a date: '2027-02-30'.\n    Use YYYY-MM-DD, e.g. 2026-02-01, or today.\n\n",
            "  Germany, Bayern (BY) · weekend Sat, Sun · holidays 2000-2050\n\nDE-BY › ",
            "DE-BY ›   Region: whole country\n\nDE › ",
            "DE ›   Region: Baden-Württemberg (BW)\n\nDE-BW › ",
            "  Time zone: Europe/Berlin (tzdata 2026f)\n",
            "  Working hours: 09:00-12:00, 13:00-17:00\n",
            "  /country C   country, or country and region like DE-BY (now DE-BW)\n",
            "  /region R    region of that country (now BW)\n",
            "  /tz ZONE     time zone for business hours (now Europe/Berlin)\n",
            "(now 09:00-12:00,13:00-17:00)\n",
            "  Time zone: none (clock time)\n",
            "  Working hours: 09:00-17:00\n",
            "DE-BW ›   No country: only weekends are skipped.\n\n› \n",
        ] {
            assert!(out.contains(want), "missing {want:?} in\n{out}");
        }
    }

    #[test]
    fn explains_what_it_cannot_do() {
        let out = session("2027\n/region BY\n/frob\nhello\n/country Atlantis\n/tz Mars/Base\n");
        for want in [
            "›   ✗ Listing holidays needs a country.\n    Pick a country first, e.g. DE.\n",
            "  ✗ No country set yet.\n    Pick one first, e.g. /country DE.\n",
            "  ✗ Unknown command '/frob'.\n    Try /help.\n",
            "  ✗ Couldn't read 'hello'.\n",
            "  ✗ Unknown country: 'Atlantis'.\n",
            "  ✗ Unknown time zone: 'Mars/Base'.\n",
        ] {
            assert!(out.contains(want), "missing {want:?} in\n{out}");
        }
        let ascii = {
            let mut out = vec![];
            run("x\n".as_bytes(), &mut out, false).unwrap();
            anstream::adapter::strip_str(&String::from_utf8(out).unwrap()).to_string()
        };
        assert!(
            ascii.contains("+-----")
                && ascii.contains("* date-calc")
                && ascii.contains("x Couldn't")
        );
    }
}
