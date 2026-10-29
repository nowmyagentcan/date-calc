//! Reading the values people type: dates, weekends and amounts.

use jiff::civil::Date;

use crate::error::Error;

/// Weekday names from Monday, as text output writes them.
pub const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// `YYYY-MM-DD` with a year from 0001 to 9999, or `today` (the computer's local date).
pub fn date(text: &str) -> Result<Date, Error> {
    if text.eq_ignore_ascii_case("today") {
        return Ok(jiff::Zoned::now().date());
    }
    let bad = || {
        Error::new("bad_date", format!("Not a date: '{text}'."))
            .hint("Use YYYY-MM-DD, e.g. 2026-02-01, or today.")
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let parts: Vec<_> = text.split('-').collect();
    let [y, m, d] = parts[..] else {
        return Err(bad());
    };
    if !(digits(y) && y.len() >= 4 && digits(m) && m.len() == 2 && digits(d) && d.len() == 2) {
        return Err(bad());
    }
    if y.len() > 4 || y == "0000" {
        return Err(Error::new(
            "date_out_of_range",
            format!("{text} is outside 0001-01-01 to 9999-12-31."),
        ));
    }
    Date::new(y.parse().unwrap(), m.parse().unwrap(), d.parse().unwrap()).map_err(|_| bad())
}

/// Day names (`SAT,SUN`, `fri,saturday`), a 7-digit mask from Monday with 1 = day off
/// (`0000011`), or `none`. Bit 0 of the result is Monday.
pub fn weekend(text: &str) -> Result<u8, Error> {
    let bad = || {
        Error::new("bad_weekend", format!("Not a weekend: '{text}'."))
            .hint("Use day names like SAT,SUN, a 7-digit mask from Monday like 0000011, or none.")
    };
    if text.eq_ignore_ascii_case("none") {
        return Ok(0);
    }
    if text.len() == 7 && text.bytes().all(|b| b == b'0' || b == b'1') {
        return Ok(crate::data::weekend_days(text));
    }
    const NAMES: [&str; 7] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    text.split(',').try_fold(0, |mask, word| {
        let word = word.trim().to_ascii_lowercase();
        let day = NAMES
            .iter()
            .position(|n| word.len() >= 3 && n.starts_with(&word))
            .ok_or_else(bad)?;
        Ok(mask | 1 << day)
    })
}

/// The weekend days as text output writes them: `Sat, Sun`, or `none`.
pub fn weekend_text(days: u8) -> String {
    let names: Vec<_> = (0..7)
        .filter(|i| days >> i & 1 == 1)
        .map(|i| DAYS[i])
        .collect();
    if names.is_empty() {
        "none".into()
    } else {
        names.join(", ")
    }
}

/// What an `add` amount counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    BusinessDays,
    Days,
    Weeks,
    Months,
    Years,
    BusinessHours,
}

impl Unit {
    /// The JSON name.
    pub fn name(self) -> &'static str {
        match self {
            Unit::BusinessDays => "business_days",
            Unit::Days => "days",
            Unit::Weeks => "weeks",
            Unit::Months => "months",
            Unit::Years => "years",
            Unit::BusinessHours => "business_hours",
        }
    }

    pub fn is_business(self) -> bool {
        matches!(self, Unit::BusinessDays | Unit::BusinessHours)
    }

    /// The largest amount allowed, and how messages write it.
    fn limit(self) -> (u64, &'static str) {
        match self {
            Unit::BusinessDays | Unit::Days | Unit::BusinessHours => (1_000_000, "1,000,000"),
            Unit::Weeks => (200_000, "200,000"),
            Unit::Months => (120_000, "120,000"),
            Unit::Years => (9_999, "9,999"),
        }
    }
}

/// A signed whole number (`-5`, `+10`, `10`); business hours may have 2 decimals (`1.5`) and
/// come back in seconds. Over the unit's limit is a usage error.
pub fn amount(text: &str, unit: Unit) -> Result<i64, Error> {
    let bad = || {
        Error::new("bad_amount", format!("Not a valid amount: '{text}'.")).hint(
            "Use a whole number like 10 or -5 (business hours may have 2 decimals, like 1.5).",
        )
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'-') => (-1, &text[1..]),
        Some(b'+') => (1, &text[1..]),
        _ => (1, text),
    };
    let (whole, frac) = match rest.split_once('.') {
        Some((w, f)) if unit == Unit::BusinessHours && f.len() <= 2 && digits(f) => (w, f),
        Some(_) => return Err(bad()),
        None => (rest, ""),
    };
    if !digits(whole) {
        return Err(bad());
    }
    let (limit, limit_text) = unit.limit();
    let scale = if unit == Unit::BusinessHours { 3600 } else { 1 };
    let hundredths = format!("{frac:0<2}").parse::<u64>().unwrap() * 36;
    let value = whole
        .parse::<u64>()
        .ok()
        .and_then(|w| w.checked_mul(scale)?.checked_add(hundredths))
        .filter(|&v| v <= limit * scale)
        .ok_or_else(|| {
            let unit = unit.name().replace('_', " ");
            Error::new(
                "usage",
                format!("Amount is over the limit for {unit} ({limit_text})."),
            )
        })?;
    Ok(sign * value as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date as ymd;

    #[test]
    fn reads_dates_from_year_1_to_9999() {
        assert_eq!(date("2026-02-01"), Ok(ymd(2026, 2, 1)));
        assert_eq!(date("0001-01-01"), Ok(ymd(1, 1, 1)));
        assert_eq!(date("9999-12-31"), Ok(ymd(9999, 12, 31)));
        assert_eq!(date("TODAY"), Ok(jiff::Zoned::now().date()));
        for text in [
            "2026-02-30",
            "2026-2-01",
            "26-02-01",
            "2026/02/01",
            "x",
            "",
            "-2026-01-01",
        ] {
            let e = date(text).unwrap_err();
            assert_eq!(e.code, "bad_date", "{text}");
            assert_eq!(e.message, format!("Not a date: '{text}'."));
            assert_eq!(
                e.hint.as_deref(),
                Some("Use YYYY-MM-DD, e.g. 2026-02-01, or today.")
            );
        }
        for text in ["0000-01-01", "10000-01-01"] {
            let e = date(text).unwrap_err();
            assert_eq!(e.code, "date_out_of_range");
            assert_eq!(
                e.message,
                format!("{text} is outside 0001-01-01 to 9999-12-31.")
            );
            assert_eq!(e.hint, None);
        }
    }

    #[test]
    fn reads_weekends_as_names_masks_or_none() {
        const SAT_SUN: u8 = 0b110_0000;
        for text in [
            "SAT,SUN",
            "sat, sun",
            "Saturday,Sunday",
            "0000011",
            "sun,sat",
        ] {
            assert_eq!(weekend(text), Ok(SAT_SUN), "{text}");
        }
        assert_eq!(weekend("FRI,SAT"), Ok(0b011_0000));
        assert_eq!(weekend("SUN"), Ok(0b100_0000));
        assert_eq!(weekend("none"), Ok(0));
        assert_eq!(weekend("1111111"), Ok(0b111_1111));
        for text in ["", "sa", "SAT,", "funday", "000011", "0000012"] {
            let e = weekend(text).unwrap_err();
            assert_eq!(e.code, "bad_weekend", "{text}");
            assert_eq!(e.message, format!("Not a weekend: '{text}'."));
        }
        assert_eq!(weekend_text(SAT_SUN), "Sat, Sun");
        assert_eq!(weekend_text(0b101_0000), "Fri, Sun");
        assert_eq!(weekend_text(0), "none");
    }

    #[test]
    fn reads_amounts_within_limits() {
        assert_eq!(amount("10", Unit::BusinessDays), Ok(10));
        assert_eq!(amount("+10", Unit::Days), Ok(10));
        assert_eq!(amount("-5", Unit::BusinessDays), Ok(-5));
        assert_eq!(amount("1000000", Unit::Days), Ok(1_000_000));
        assert_eq!(amount("-9999", Unit::Years), Ok(-9999));
        assert_eq!(amount("1.5", Unit::BusinessHours), Ok(5400));
        assert_eq!(amount("-0.25", Unit::BusinessHours), Ok(-900));
        assert_eq!(
            amount("2.", Unit::BusinessHours).unwrap_err().code,
            "bad_amount"
        );
        for text in ["1.5", "x", "", "-", "+-1", "1e3", "1.0"] {
            let e = amount(text, Unit::BusinessDays).unwrap_err();
            assert_eq!(e.code, "bad_amount", "{text}");
            assert_eq!(e.message, format!("Not a valid amount: '{text}'."));
        }
        assert_eq!(
            amount("1.234", Unit::BusinessHours).unwrap_err().code,
            "bad_amount"
        );
        let over = |text, unit| amount(text, unit).unwrap_err();
        let e = over("1000001", Unit::BusinessDays);
        assert_eq!(e.code, "usage");
        assert_eq!(
            e.message,
            "Amount is over the limit for business days (1,000,000)."
        );
        assert_eq!(e.exit_code(), 2);
        assert_eq!(
            over("-200001", Unit::Weeks).message,
            "Amount is over the limit for weeks (200,000)."
        );
        assert_eq!(over("120001", Unit::Months).code, "usage");
        assert_eq!(over("10000", Unit::Years).code, "usage");
        assert_eq!(over("1000000.01", Unit::BusinessHours).code, "usage");
        assert_eq!(over("99999999999999999999999", Unit::Days).code, "usage");
    }
}
