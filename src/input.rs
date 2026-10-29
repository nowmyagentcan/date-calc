//! Reading the values people type: dates, date-times, weekends, working hours and amounts.

use jiff::civil::{Date, Time};
use jiff::tz::{Offset, TimeZone};
use jiff::{SignedDuration, Timestamp};

use crate::error::Error;
use crate::hours::{DAY, Moment, Window};

/// Weekday names from Monday, as text output writes them.
pub const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Error `bad_date` for `text`.
pub fn bad_date(text: &str) -> Error {
    Error::new("bad_date", format!("Not a date: '{text}'."))
        .hint("Use YYYY-MM-DD, e.g. 2026-02-01, or today.")
}

/// `YYYY-MM-DD` with a year from 0001 to 9999, or `today` (the computer's local date).
pub fn date(text: &str) -> Result<Date, Error> {
    if text.eq_ignore_ascii_case("today") {
        return Ok(jiff::Zoned::now().date());
    }
    let bad = || bad_date(text);
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

/// `HH:MM`, or `HH:MM:SS` with any fraction (dropped) when `seconds` allows it, as seconds from
/// midnight.
fn clock(text: &str, seconds: bool) -> Option<i32> {
    let text = match text.split_once('.') {
        Some((whole, fraction))
            if seconds
                && whole.len() == 8
                && !fraction.is_empty()
                && fraction.bytes().all(|b| b.is_ascii_digit()) =>
        {
            whole
        }
        _ => text,
    };
    let parts: Vec<_> = text.split(':').collect();
    let two_digits = |p: &&str| p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit());
    if !(parts.len() == 2 || seconds && parts.len() == 3) || !parts.iter().all(two_digits) {
        return None;
    }
    let n: Vec<i32> = parts.iter().map(|p| p.parse().unwrap()).collect();
    let valid = n[0] < 24 && n[1..].iter().all(|&x| x < 60);
    valid.then(|| n[0] * 3600 + n[1] * 60 + n.get(2).unwrap_or(&0))
}

/// An IANA time zone name in the bundled tzdata, in any case, or `UTC`.
pub fn timezone(text: &str) -> Result<TimeZone, Error> {
    jiff::tz::db().get(text).map_err(|_| {
        Error::new("unknown_timezone", format!("Unknown time zone: '{text}'."))
            .hint("Use an IANA name such as Europe/Berlin or America/New_York.")
    })
}

/// A date (meaning 00:00), `YYYY-MM-DDTHH:MM[:SS]` with `T` or a space, optionally ending in
/// `Z` or an offset, or `now`. Without `zone` it is clock time and an offset is an error; with
/// `zone`, clock times are in it and instants are moved into it.
pub fn datetime(text: &str, zone: Option<&TimeZone>) -> Result<Moment, Error> {
    let bad = || {
        Error::new("bad_datetime", format!("Not a date and time: '{text}'.")).hint(
            "Use YYYY-MM-DDTHH:MM, e.g. 2026-11-02T09:30, optionally with Z or an offset, or now.",
        )
    };
    let outside = || {
        Error::new(
            "date_out_of_range",
            format!("{text} is outside 0001-01-01 to 9999-12-31."),
        )
    };
    if text.eq_ignore_ascii_case("now") {
        let now = Timestamp::from_second(Timestamp::now().as_second()).unwrap();
        return Ok(match zone {
            Some(tz) => Moment::Zoned(now.to_zoned(tz.clone())),
            None => Moment::Civil(now.to_zoned(TimeZone::system()).datetime()),
        });
    }
    let (day, time) = match text.get(10..11) {
        Some("T" | "t" | " ") => (&text[..10], &text[11..]),
        _ => (text, "00:00"),
    };
    let day = date(day).map_err(|e| if e.code == "bad_date" { bad() } else { e })?;
    let (time, offset) = match time.find(['Z', 'z', '+', '-']) {
        Some(i) => (&time[..i], Some(&time[i..])),
        None => (time, None),
    };
    let at = clock(time, true).ok_or_else(bad)?;
    let civil = day.to_datetime(Time::midnight()) + SignedDuration::from_secs(at.into());
    let offset = match offset {
        None => None,
        Some("Z" | "z") => Some(Offset::UTC),
        Some(o) => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let seconds = clock(&o[1..], false).ok_or_else(bad)?;
            Some(Offset::from_seconds(sign * seconds).map_err(|_| bad())?)
        }
    };
    let instant = match (offset, zone) {
        (None, None) => return Ok(Moment::Civil(civil)),
        (Some(_), None) => {
            return Err(Error::new(
                "tz_required",
                format!("{text} has a UTC offset, so the business's time zone is needed."),
            )
            .hint("Add --tz, e.g. --tz Europe/Berlin, or give local times without an offset."));
        }
        (None, Some(tz)) => civil.to_zoned(tz.clone()).map(|z| z.timestamp()),
        (Some(offset), Some(_)) => offset.to_timestamp(civil),
    };
    let moment = Moment::Zoned(
        instant
            .map_err(|_| outside())?
            .to_zoned(zone.unwrap().clone()),
    );
    if !(1..=9999).contains(&moment.shown().year()) {
        return Err(outside());
    }
    Ok(moment)
}

/// Working hours: `HH:MM-HH:MM` windows separated by commas, `24:00` allowed as an end; a window
/// ending before it starts runs past midnight. Sorted by start; windows may not overlap.
pub fn hours(text: &str) -> Result<Vec<Window>, Error> {
    let bad = || {
        Error::new("bad_hours", format!("Not working hours: '{text}'."))
            .hint("Use HH:MM-HH:MM, several separated by commas, e.g. 09:00-12:00,13:00-17:00.")
    };
    let window = |w: &str| {
        let (a, b) = w.split_once('-')?;
        let start = clock(a.trim(), false)?;
        let end = match b.trim() {
            "24:00" => DAY,
            b => clock(b, false)?,
        };
        let end = if end < start { end + DAY } else { end };
        (start != end).then_some(Window { start, end })
    };
    let mut windows: Vec<_> = text
        .split(',')
        .map(window)
        .collect::<Option<_>>()
        .ok_or_else(bad)?;
    windows.sort_by_key(|w| w.start);
    // Every day repeats the windows, so they must not meet on a 24-hour circle either.
    for (i, a) in windows.iter().enumerate() {
        for b in &windows[i + 1..] {
            if [-DAY, 0, DAY]
                .iter()
                .any(|k| a.start < b.end + k && b.start + k < a.end)
            {
                return Err(Error::new(
                    "overlapping_hours",
                    format!("Working hours overlap: {a} and {b}."),
                )
                .hint("Give windows that don't overlap, in any order."));
            }
        }
    }
    Ok(windows)
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
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    #[default]
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
    fn reads_clock_date_times() {
        let civil = |text| match datetime(text, None) {
            Ok(Moment::Civil(dt)) => dt,
            other => panic!("{text}: {other:?}"),
        };
        let at = |h, m, s| ymd(2026, 11, 2).at(h, m, s, 0);
        assert_eq!(civil("2026-11-02T09:30"), at(9, 30, 0));
        assert_eq!(civil("2026-11-02 09:30:15"), at(9, 30, 15));
        assert_eq!(civil("2026-11-02t23:59:59"), at(23, 59, 59));
        assert_eq!(civil("2026-11-02T09:30:15.999"), at(9, 30, 15));
        assert_eq!(civil("2026-11-02"), at(0, 0, 0));
        assert_eq!(civil("9999-12-31T23:59").date(), ymd(9999, 12, 31));
        assert_eq!(civil("NOW").subsec_nanosecond(), 0);
        for text in [
            "2026-11-02T",
            "2026-11-02T9:30",
            "2026-11-02T24:00",
            "2026-11-02T09:60",
            "2026-11-02T09:30:5",
            "2026-11-02T09:30.5",
            "2026-11-02T09:30:00.",
            "2026-11-02T09:30:00.5x",
            "2026-11-02X09:30",
            "2026-11-31T09:30",
            "x",
        ] {
            let e = datetime(text, None).unwrap_err();
            assert_eq!(e.code, "bad_datetime", "{text}");
            assert_eq!(e.message, format!("Not a date and time: '{text}'."));
            assert_eq!(
                e.hint.as_deref(),
                Some(
                    "Use YYYY-MM-DDTHH:MM, e.g. 2026-11-02T09:30, optionally with Z or an offset, or now."
                )
            );
        }
        assert_eq!(
            datetime("0000-01-01T10:00", None).unwrap_err().code,
            "date_out_of_range"
        );
    }

    #[test]
    fn reads_instants_and_zones() {
        let berlin = timezone("europe/BERLIN").unwrap();
        assert_eq!(berlin.iana_name(), Some("Europe/Berlin"));
        assert_eq!(timezone("UTC").unwrap().iana_name(), Some("UTC"));
        let e = timezone("Mars/Base").unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str(), e.hint.as_deref()),
            (
                "unknown_timezone",
                "Unknown time zone: 'Mars/Base'.",
                Some("Use an IANA name such as Europe/Berlin or America/New_York.")
            )
        );
        let instant = |text| match datetime(text, Some(&berlin)) {
            Ok(Moment::Zoned(z)) => z.timestamp().to_string(),
            other => panic!("{text}: {other:?}"),
        };
        assert_eq!(instant("2026-11-02T09:30:00Z"), "2026-11-02T09:30:00Z");
        assert_eq!(instant("2026-11-02t09:30z"), "2026-11-02T09:30:00Z");
        assert_eq!(instant("2026-11-02 09:30+05:30"), "2026-11-02T04:00:00Z");
        assert_eq!(instant("2026-11-02T09:30-04:00"), "2026-11-02T13:30:00Z");
        // Clock times are read in the zone: 09:30 in Berlin is 08:30Z in November.
        assert_eq!(instant("2026-11-02T09:30"), "2026-11-02T08:30:00Z");
        assert_eq!(instant("2026-11-02"), "2026-11-01T23:00:00Z");
        assert!(matches!(
            datetime("now", Some(&berlin)),
            Ok(Moment::Zoned(_))
        ));
        let e = datetime("2026-11-02T09:30:00Z", None).unwrap_err();
        assert_eq!(e.code, "tz_required");
        assert_eq!(
            e.message,
            "2026-11-02T09:30:00Z has a UTC offset, so the business's time zone is needed."
        );
        assert_eq!(
            e.hint.as_deref(),
            Some("Add --tz, e.g. --tz Europe/Berlin, or give local times without an offset.")
        );
        for text in [
            "2026-11-02T09:30+5",
            "2026-11-02T09:30+05:60",
            "2026-11-02T09:30Zz",
        ] {
            assert_eq!(
                datetime(text, None).unwrap_err().code,
                "bad_datetime",
                "{text}"
            );
        }
    }

    #[test]
    fn reads_working_hours() {
        let w = |start, end| Window { start, end };
        assert_eq!(hours("09:00-17:00"), Ok(vec![w(32_400, 61_200)]));
        assert_eq!(
            hours("13:00-17:00, 09:00-12:00"),
            Ok(vec![w(32_400, 43_200), w(46_800, 61_200)])
        );
        assert_eq!(hours("00:00-24:00"), Ok(vec![w(0, DAY)]));
        assert_eq!(hours("22:00-06:00"), Ok(vec![w(79_200, 108_000)]));
        let shown: Vec<_> = hours("22:00-06:00,09:00-17:00")
            .unwrap()
            .iter()
            .map(Window::to_string)
            .collect();
        assert_eq!(
            shown,
            ["09:00-17:00", "22:00-06:00"].map(String::from).to_vec()
        );
        for text in [
            "",
            "9:00-17:00",
            "09:00-09:00",
            "24:00-08:00",
            "09:00-24:30",
            "09:00",
            "09:00-17:00,",
            "+9:00-17:00",
            "09:00-17:60",
        ] {
            let e = hours(text).unwrap_err();
            assert_eq!(e.code, "bad_hours", "{text}");
            assert_eq!(e.message, format!("Not working hours: '{text}'."));
        }
        for (text, message) in [
            ("09:00-12:00,11:00-17:00", "09:00-12:00 and 11:00-17:00"),
            ("22:00-06:00,05:00-08:00", "05:00-08:00 and 22:00-06:00"),
            ("00:00-24:00,09:00-10:00", "00:00-24:00 and 09:00-10:00"),
        ] {
            let e = hours(text).unwrap_err();
            assert_eq!(e.code, "overlapping_hours");
            assert_eq!(e.message, format!("Working hours overlap: {message}."));
            assert_eq!(
                e.hint.as_deref(),
                Some("Give windows that don't overlap, in any order.")
            );
        }
        assert!(hours("09:00-24:00,00:00-01:00").is_ok());
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
