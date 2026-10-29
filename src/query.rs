//! The one-line queries of interactive mode and the page: two dates, two date-times, a date
//! and a signed amount, a year, or a date to convert.

use crate::calendars::{Calendar, month_by_name};
use crate::days::Roll;
use crate::error::Error;
use crate::input::Unit;
use crate::report::Op;

const TRY: &str =
    "Try: 2026-02-01 to 2026-02-28 · 2026-11-02 +10 · 2027 · 2026-10-27 · 1 Ramadan 1448";

const SEPARATORS: [&str; 5] = ["to", "until", "..", "–", "-"];

/// Reads one query; case doesn't matter and commas are ignored.
pub fn parse(line: &str) -> Result<Op, Error> {
    let line = line.trim();
    let bad = || Error::new("bad_query", format!("Couldn't read '{line}'.")).hint(TRY);
    if line.len() > 256 {
        return Err(Error::new("bad_query", "A query can be at most 256 bytes long.").hint(TRY));
    }
    let spaced = line
        .replace(',', " ")
        .replace("..", " .. ")
        .replace('–', " – ");
    let words: Vec<_> = spaced.split_whitespace().collect();
    let lower: Vec<_> = words.iter().map(|w| w.to_lowercase()).collect();
    let lower: Vec<_> = lower.iter().map(String::as_str).collect();
    let holidays = |year: Option<&str>| Op::Holidays {
        year: year.map(|y| y.parse().expect("four digits")),
        from: None,
        to: None,
    };
    match lower[..] {
        ["holidays"] => return Ok(holidays(None)),
        ["holidays", y] | [y] if digits(y, 4) => return Ok(holidays(Some(y))),
        _ => {}
    }
    let Some((start, timed, n)) = moment(&words) else {
        return calendar_date(&words).ok_or_else(bad);
    };
    let convert = |from: Option<&str>| Op::Convert {
        date: start.clone(),
        from: from.map(String::from),
        to: None,
    };
    match &words[n..] {
        [] if !timed => return Ok(convert(None)),
        [cal] if !timed && Calendar::parse(cal).is_ok() => return Ok(convert(Some(cal))),
        [amount, unit @ ..]
            if is_amount(amount) && (!unit.is_empty() || amount.starts_with(['+', '-'])) =>
        {
            return Ok(Op::Add {
                start,
                amount: amount.to_string(),
                unit: self::unit(&lower[n + 1..]).ok_or_else(bad)?,
                roll: Roll::None,
            });
        }
        _ => {}
    }
    let rest = match &lower[n..] {
        [sep, ..] if SEPARATORS.contains(sep) => &words[n + 1..],
        _ => &words[n..],
    };
    match moment(rest) {
        Some((end, end_timed, m)) if m == rest.len() => Ok(if timed || end_timed {
            Op::Hours { start, end }
        } else {
            Op::Between { start, end }
        }),
        _ => Err(bad()),
    }
}

fn digits(word: &str, len: usize) -> bool {
    word.len() == len && word.bytes().all(|b| b.is_ascii_digit())
}

/// Shaped like `YYYY-MM-DD` (a leap month may carry an `L`), valid or not.
fn date_like(word: &str) -> bool {
    let parts: Vec<_> = word.split('-').collect();
    let number = |p: &&str| {
        let p = p.strip_suffix(['L', 'l']).unwrap_or(p);
        !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())
    };
    parts.len() == 3 && parts.iter().all(number)
}

/// `HH:MM`, maybe with seconds and an offset.
fn time_like(word: &str) -> bool {
    word.len() >= 5 && digits(&word[..2], 2) && word.as_bytes()[2] == b':'
}

/// A date or date-time at the start of `words`: its text, whether it has a time, and the
/// number of words it took (a date and a time may be two).
fn moment(words: &[&str]) -> Option<(String, bool, usize)> {
    let &first = words.first()?;
    let lower = first.to_lowercase();
    if lower == "now" {
        return Some((first.into(), true, 1));
    }
    if lower == "today" || date_like(first) {
        return Some(match words.get(1) {
            Some(time) if date_like(first) && time_like(time) => {
                (format!("{first} {time}"), true, 2)
            }
            _ => (first.into(), false, 1),
        });
    }
    let joined = first
        .get(10..11)
        .is_some_and(|t| t.eq_ignore_ascii_case("t"));
    (joined && date_like(&first[..10])).then(|| (first.into(), true, 1))
}

/// `10`, `+10`, `-5`, `1.5`.
fn is_amount(word: &str) -> bool {
    let number = word.strip_prefix(['+', '-']).unwrap_or(word);
    let (whole, frac) = number.split_once('.').unwrap_or((number, "0"));
    [whole, frac]
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The unit words after an amount; none means business days. Hours are business hours.
fn unit(words: &[&str]) -> Option<Unit> {
    Some(match words.join(" ").as_str() {
        "" | "bd" | "business day" | "business days" | "workday" | "workdays" => Unit::BusinessDays,
        "d" | "day" | "days" => Unit::Days,
        "w" | "week" | "weeks" => Unit::Weeks,
        "month" | "months" => Unit::Months,
        "y" | "year" | "years" => Unit::Years,
        "h" | "bh" | "hour" | "hours" | "business hour" | "business hours" => Unit::BusinessHours,
        _ => return None,
    })
}

/// `D MonthName YYYY`, with an optional AH, SH or BS.
fn calendar_date(words: &[&str]) -> Option<Op> {
    let words = match words {
        [w @ .., era] if ["ah", "sh", "bs"].contains(&era.to_lowercase().as_str()) => w,
        w => w,
    };
    let [day, month @ .., year] = words else {
        return None;
    };
    let day = (1..=2).any(|n| digits(day, n));
    (day && digits(year, 4) && !month.is_empty() && month_by_name(&month.join(" ")).is_some()).then(
        || Op::Convert {
            date: words.join(" "),
            from: None,
            to: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn between(start: &str, end: &str) -> Op {
        Op::Between {
            start: start.into(),
            end: end.into(),
        }
    }

    fn add(start: &str, amount: &str, unit: Unit) -> Op {
        Op::Add {
            start: start.into(),
            amount: amount.into(),
            unit,
            roll: Roll::None,
        }
    }

    fn convert(date: &str, from: Option<&str>) -> Op {
        Op::Convert {
            date: date.into(),
            from: from.map(String::from),
            to: None,
        }
    }

    #[test]
    fn reads_two_dates_as_between() {
        for line in [
            "2026-02-01 to 2026-02-28",
            "2026-02-01 2026-02-28",
            "2026-02-01 until 2026-02-28",
            "2026-02-01..2026-02-28",
            "2026-02-01 – 2026-02-28",
            "2026-02-01, 2026-02-28",
            "  2026-02-01 TO 2026-02-28 ",
        ] {
            assert_eq!(
                parse(line),
                Ok(between("2026-02-01", "2026-02-28")),
                "{line}"
            );
        }
        assert_eq!(
            parse("today to 2026-12-31"),
            Ok(between("today", "2026-12-31"))
        );
        // Shaped like dates: the date reader names the bad one.
        assert_eq!(
            parse("2026-02-30 to 2026-03-05"),
            Ok(between("2026-02-30", "2026-03-05"))
        );
    }

    #[test]
    fn reads_date_times_as_hours() {
        let hours = |s: &str, e: &str| Op::Hours {
            start: s.into(),
            end: e.into(),
        };
        assert_eq!(
            parse("2026-11-02 10:00 to 2026-11-05 15:00"),
            Ok(hours("2026-11-02 10:00", "2026-11-05 15:00"))
        );
        assert_eq!(
            parse("2026-11-02T10:00 2026-11-05"),
            Ok(hours("2026-11-02T10:00", "2026-11-05"))
        );
        assert_eq!(
            parse("2026-10-30T16:00:00Z .. 2026-11-02 18:00+01:00"),
            Ok(hours("2026-10-30T16:00:00Z", "2026-11-02 18:00+01:00"))
        );
        assert_eq!(parse("now to 2026-12-31"), Ok(hours("now", "2026-12-31")));
    }

    #[test]
    fn reads_amounts_with_unit_words() {
        use Unit::*;
        for (line, op) in [
            ("2026-11-02 +10", add("2026-11-02", "+10", BusinessDays)),
            ("2026-03-02 -5", add("2026-03-02", "-5", BusinessDays)),
            ("2026-11-02 10 bd", add("2026-11-02", "10", BusinessDays)),
            (
                "2026-11-02 10 business days",
                add("2026-11-02", "10", BusinessDays),
            ),
            ("2026-11-02 1 Workday", add("2026-11-02", "1", BusinessDays)),
            ("2026-11-02 +3 weeks", add("2026-11-02", "+3", Weeks)),
            ("2026-01-31 1 month", add("2026-01-31", "1", Months)),
            ("2026-01-31 -2 months", add("2026-01-31", "-2", Months)),
            ("2026-01-31 2 y", add("2026-01-31", "2", Years)),
            ("2026-01-31 5 d", add("2026-01-31", "5", Days)),
            (
                "2026-11-02 16:00 +4 h",
                add("2026-11-02 16:00", "+4", BusinessHours),
            ),
            (
                "2026-11-02T16:00 1.5 business hours",
                add("2026-11-02T16:00", "1.5", BusinessHours),
            ),
            ("today +1", add("today", "+1", BusinessDays)),
        ] {
            assert_eq!(parse(line), Ok(op), "{line}");
        }
    }

    #[test]
    fn reads_years_as_holidays() {
        let holidays = |year| Op::Holidays {
            year,
            from: None,
            to: None,
        };
        assert_eq!(parse("2027"), Ok(holidays(Some(2027))));
        assert_eq!(parse("holidays 2027"), Ok(holidays(Some(2027))));
        assert_eq!(parse("Holidays"), Ok(holidays(None)));
    }

    #[test]
    fn reads_dates_to_convert() {
        assert_eq!(parse("2026-10-27"), Ok(convert("2026-10-27", None)));
        assert_eq!(parse("today"), Ok(convert("today", None)));
        assert_eq!(
            parse("1448-01-01 islamic"),
            Ok(convert("1448-01-01", Some("islamic")))
        );
        assert_eq!(
            parse("5787-05L-01 hebrew"),
            Ok(convert("5787-05L-01", Some("hebrew")))
        );
        assert_eq!(parse("1 Ramadan 1448"), Ok(convert("1 Ramadan 1448", None)));
        assert_eq!(parse("1 adar i 5787"), Ok(convert("1 adar i 5787", None)));
        assert_eq!(
            parse("10 Kartik 2083 BS"),
            Ok(convert("10 Kartik 2083", None))
        );
    }

    #[test]
    fn refuses_anything_else() {
        for line in [
            "",
            "hello",
            "2026-11-02 10",
            "2026-11-02 +10 fortnights",
            "2026-11-02 to",
            "2026-11-02 to 2026-11-05 to 2026-11-09",
            "2026-11-02 10:00",
            "1 Smarch 2026",
            "holidays next year",
            "20270",
        ] {
            let e = parse(line).unwrap_err();
            assert_eq!(e.code, "bad_query", "{line}");
            assert_eq!(e.message, format!("Couldn't read '{line}'."));
            assert_eq!(e.hint.as_deref(), Some(TRY));
        }
        let long = "2026-01-01 ".repeat(30);
        assert_eq!(parse(&long).unwrap_err().code, "bad_query");
    }

    #[test]
    fn every_form_runs() {
        use crate::report::{Options, run};
        let de = Options {
            country: Some("DE".into()),
            ..Options::default()
        };
        for (line, op) in [
            ("2026-02-01 to 2026-02-28", "between"),
            ("2026-11-02 10:00 to 2026-11-05 15:00", "hours"),
            ("2026-11-02 +10", "add"),
            ("2026-11-02 16:00 +4 h", "add"),
            ("2027", "holidays"),
            ("2026-10-27", "convert"),
            ("1 Ramadan 1448", "convert"),
        ] {
            let answer = run(&parse(line).unwrap(), &de).unwrap();
            assert_eq!(answer.op(), op, "{line}");
        }
        let e = run(&parse("2027").unwrap(), &Options::default()).unwrap_err();
        assert_eq!(e.message, "Listing holidays needs a country.");
        assert_eq!(e.hint.as_deref(), Some("Pick a country first, e.g. DE."));
        assert_eq!((e.code, e.exit_code()), ("country_required", 1));
    }
}
