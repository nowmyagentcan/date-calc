//! Dates in other calendars: ICU4X's Umm al-Qura, tabular Islamic, Hebrew, Chinese and Persian
//! calendars, and Bikram Sambat from nepali-datetime's table.

use icu_calendar::cal::hijri::{
    TabularAlgorithm, TabularAlgorithmEpoch, TabularAlgorithmLeapYears,
};
use std::sync::LazyLock;

use icu_calendar::cal::{self as icu, ChineseTraditional, Hijri};
use icu_calendar::error::DateNewError;
use icu_calendar::types::Month;
use icu_calendar::{AsCalendar, Iso};
use jiff::civil::{Date, date};

use crate::days::Warning;
use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Calendar {
    Gregorian,
    UmmAlQura,
    IslamicCivil,
    Hebrew,
    Chinese,
    Persian,
    BikramSambat,
}

use Calendar::*;

impl Calendar {
    pub const ALL: [Calendar; 7] = [
        Gregorian,
        UmmAlQura,
        IslamicCivil,
        Hebrew,
        Chinese,
        Persian,
        BikramSambat,
    ];

    /// The JSON name.
    pub fn id(self) -> &'static str {
        match self {
            Gregorian => "gregorian",
            UmmAlQura => "islamic-umalqura",
            IslamicCivil => "islamic-civil",
            Hebrew => "hebrew",
            Chinese => "chinese",
            Persian => "persian",
            BikramSambat => "bikram-sambat",
        }
    }

    /// The name text output shows beside the date.
    pub fn title(self) -> &'static str {
        match self {
            UmmAlQura => "Islamic (Umm al-Qura)",
            IslamicCivil => "Islamic (tabular)",
            _ => self.name(),
        }
    }

    /// The name messages use: "the {name} calendar".
    fn name(self) -> &'static str {
        match self {
            Gregorian => "Gregorian",
            UmmAlQura => "Umm al-Qura",
            IslamicCivil => "tabular Islamic",
            Hebrew => "Hebrew",
            Chinese => "Chinese",
            Persian => "Persian",
            BikramSambat => "Bikram Sambat",
        }
    }

    /// A calendar by its name or an alias, in any case, with `-` or `_`.
    pub fn parse(text: &str) -> Result<Calendar, Error> {
        let found = match text.to_ascii_lowercase().replace('_', "-").as_str() {
            "gregorian" => Gregorian,
            "islamic-umalqura" | "islamic" | "hijri" | "umalqura" => UmmAlQura,
            "islamic-civil" | "tabular" => IslamicCivil,
            "hebrew" => Hebrew,
            "chinese" => Chinese,
            "persian" | "solar-hijri" | "jalali" | "shamsi" => Persian,
            "bikram-sambat" | "nepali" | "bs" => BikramSambat,
            _ => {
                return Err(Error::new("unknown_calendar", format!("Unknown calendar: '{text}'."))
                    .hint("Use gregorian, islamic-umalqura, islamic-civil, hebrew, chinese, persian or bikram-sambat."));
            }
        };
        Ok(found)
    }

    /// The range a table covers, for calendars whose dates come from one.
    fn table(self) -> Option<(&'static str, Date, Date)> {
        match self {
            UmmAlQura => Some(("1300-1600 AH", date(1882, 11, 12), date(2174, 11, 25))),
            BikramSambat => Some(("1975-2100 BS", BS_EPOCH, date(2044, 4, 12))),
            _ => None,
        }
    }

    /// Error `calendar_out_of_range` for this calendar's table.
    fn out_of_range(self) -> Error {
        let (years, from, to) = self.table().expect("a calendar with a table");
        let name = self.name();
        Error::new(
            "calendar_out_of_range",
            format!("The {name} table covers {years} ({from} to {to})."),
        )
    }

    fn suffix(self) -> &'static str {
        match self {
            UmmAlQura | IslamicCivil => " AH",
            Persian => " SH",
            BikramSambat => " BS",
            _ => "",
        }
    }
}

const ISLAMIC: [&str; 12] = [
    "Muharram",
    "Safar",
    "Rabi al-Awwal",
    "Rabi al-Thani",
    "Jumada al-Ula",
    "Jumada al-Akhira",
    "Rajab",
    "Shaban",
    "Ramadan",
    "Shawwal",
    "Dhu al-Qada",
    "Dhu al-Hijja",
];
/// By month number: 5 Shevat, 6 Adar (Adar II in leap years); Adar I is the leap month 5L.
const HEBREW: [&str; 12] = [
    "Tishrei", "Cheshvan", "Kislev", "Tevet", "Shevat", "Adar", "Nisan", "Iyar", "Sivan", "Tammuz",
    "Av", "Elul",
];
const PERSIAN: [&str; 12] = [
    "Farvardin",
    "Ordibehesht",
    "Khordad",
    "Tir",
    "Mordad",
    "Shahrivar",
    "Mehr",
    "Aban",
    "Azar",
    "Dey",
    "Bahman",
    "Esfand",
];
const BIKRAM_SAMBAT: [&str; 12] = [
    "Baisakh", "Jestha", "Ashar", "Shrawan", "Bhadra", "Asoj", "Kartik", "Mangsir", "Poush",
    "Magh", "Falgun", "Chait",
];
const STEMS: [&str; 10] = [
    "Jia", "Yi", "Bing", "Ding", "Wu", "Ji", "Geng", "Xin", "Ren", "Gui",
];
const BRANCHES: [&str; 12] = [
    "Zi", "Chou", "Yin", "Mao", "Chen", "Si", "Wu", "Wei", "Shen", "You", "Xu", "Hai",
];
const ZODIAC: [&str; 12] = [
    "Rat", "Ox", "Tiger", "Rabbit", "Dragon", "Snake", "Horse", "Goat", "Monkey", "Rooster", "Dog",
    "Pig",
];

/// A month's English name; `leap_year` tells Hebrew's Adar from Adar II.
fn month_name(cal: Calendar, month: u8, leap: bool, leap_year: bool) -> String {
    let i = usize::from(month).wrapping_sub(1);
    let table = match cal {
        Gregorian => unreachable!("Gregorian dates stay jiff dates"),
        UmmAlQura | IslamicCivil => &ISLAMIC,
        Hebrew => &HEBREW,
        Persian => &PERSIAN,
        BikramSambat => &BIKRAM_SAMBAT,
        Chinese if leap => return format!("Leap month {month}"),
        Chinese => return format!("Month {month}"),
    };
    match (cal, month, leap) {
        (Hebrew, 5, true) => "Adar I".into(),
        (Hebrew, 6, false) if leap_year => "Adar II".into(),
        (_, _, false) if i < 12 => table[i].into(),
        _ => format!("Month {month}{}", if leap { "L" } else { "" }),
    }
}

/// A date in one calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalDate {
    pub calendar: Calendar,
    /// Chinese: the Gregorian year in which the lunar year starts.
    pub year: i32,
    /// The month's number in a common year; Hebrew Adar I is 5 with `leap_month`.
    pub month: u8,
    pub leap_month: bool,
    pub day: u8,
    pub month_name: String,
    /// `16 Jumada al-Ula 1448 AH`, or `Month 9, day 18, year of the Horse (Bing-Wu)`.
    pub text: String,
    /// Chinese only: the cyclic year (`Bing-Wu`) and its animal (`Horse`).
    pub cycle: Option<(String, &'static str)>,
    pub note: Option<&'static str>,
}

/// The fields icu_calendar gives for a date in any of its calendars.
fn icu_fields<C: AsCalendar>(d: icu_calendar::Date<C>) -> (i32, u8, bool, u8, Option<u8>, bool) {
    let month = d.month().to_input();
    let cyclic = d.year().cyclic();
    let year = cyclic.map_or(d.year().extended_year(), |c| c.related_iso);
    let leap_year = d.months_in_year() == 13;
    (
        year,
        month.number(),
        month.is_leap(),
        d.day_of_month().0,
        cyclic.map(|c| c.year),
        leap_year,
    )
}

/// The tabular Islamic calendar with the Friday epoch and type II leap years.
fn tabular() -> Hijri<TabularAlgorithm> {
    Hijri::new_tabular(
        TabularAlgorithmLeapYears::TypeII,
        TabularAlgorithmEpoch::Friday,
    )
}

fn iso(g: Date) -> icu_calendar::Date<Iso> {
    icu_calendar::Date::try_new_iso(g.year().into(), g.month() as u8, g.day() as u8)
        .expect("jiff dates are valid ISO dates")
}

/// `g` in `cal`; `calendar_out_of_range` outside a table.
pub fn in_calendar(cal: Calendar, g: Date) -> Result<CalDate, Error> {
    if cal
        .table()
        .is_some_and(|(_, first, last)| !(first..=last).contains(&g))
    {
        return Err(cal.out_of_range());
    }
    let (year, month, leap, day, cyclic, leap_year) = match cal {
        Gregorian => unreachable!("Gregorian dates stay jiff dates"),
        UmmAlQura => icu_fields(iso(g).to_calendar(Hijri::new_umm_al_qura())),
        IslamicCivil => icu_fields(iso(g).to_calendar(tabular())),
        Hebrew => icu_fields(iso(g).to_calendar(icu::Hebrew)),
        Chinese => icu_fields(iso(g).to_calendar(ChineseTraditional::new())),
        Persian => icu_fields(iso(g).to_calendar(icu::Persian)),
        BikramSambat => {
            let (y, m, d) = bs_from(g).expect("inside the table");
            (y, m, false, d, None, false)
        }
    };
    let month_name = month_name(cal, month, leap, leap_year);
    let cycle = cyclic.map(|c| {
        let i = usize::from(c) - 1;
        (
            format!("{}-{}", STEMS[i % 10], BRANCHES[i % 12]),
            ZODIAC[i % 12],
        )
    });
    let text = match &cycle {
        Some((cyclic, animal)) => {
            format!("{month_name}, day {day}, year of the {animal} ({cyclic})")
        }
        None => format!("{day} {month_name} {year}{}", cal.suffix()),
    };
    Ok(CalDate {
        calendar: cal,
        year,
        month,
        leap_month: leap,
        day,
        month_name,
        text,
        cycle,
        note: match cal {
            UmmAlQura => Some(
                "Saudi Arabia's official Umm al-Qura calendar. Religious dates set by moon sighting can differ by a day, and more in other countries.",
            ),
            IslamicCivil => Some(
                "Tabular Islamic calendar (Friday epoch, 30-year cycle); usually 0-2 days from Umm al-Qura.",
            ),
            _ => None,
        },
    })
}

/// The Gregorian date of a date in `cal`: `month` is the month's number in a common year, and
/// `leap` picks the leap month after it (Hebrew Adar I is 5 with `leap`).
pub fn to_gregorian(
    cal: Calendar,
    year: i32,
    month: u8,
    leap: bool,
    day: u8,
) -> Result<Date, Error> {
    let found = match cal {
        Gregorian if leap => Err(Invalid::Month),
        Gregorian => return crate::input::date(&format!("{year:04}-{month:02}-{day:02}")),
        UmmAlQura if !(1300..=1600).contains(&year) => return Err(cal.out_of_range()),
        UmmAlQura => icu_date(year, month, leap, day, Hijri::new_umm_al_qura()),
        IslamicCivil => icu_date(year, month, leap, day, tabular()),
        Hebrew => icu_date(year, month, leap, day, icu::Hebrew),
        Chinese => icu_date(year, month, leap, day, ChineseTraditional::new()),
        Persian => icu_date(year, month, leap, day, icu::Persian),
        BikramSambat if leap => Err(Invalid::Month),
        BikramSambat => bs_to(year, month, day),
    };
    found.map_err(|why| {
        let name = cal.name();
        let month_name = month_name(cal, month, leap, cal == Hebrew && hebrew_leap(year));
        let text = match cal {
            Chinese => format!("{}, day {day}, {year}", month_name.to_lowercase()),
            _ => format!("{day} {month_name} {year}"),
        };
        match why {
            Invalid::Range if cal.table().is_some() => cal.out_of_range(),
            Invalid::Range => Error::new(
                "date_out_of_range",
                format!("{text} is outside 0001-01-01 to 9999-12-31."),
            ),
            Invalid::Month => Error::new(
                "bad_calendar_date",
                format!("There is no {text} in the {name} calendar."),
            ),
            Invalid::Day(n) => Error::new(
                "bad_calendar_date",
                format!("There is no {text} in the {name} calendar."),
            )
            .hint(format!("{month_name} {year} has {n} days.")),
        }
    })
}

/// A Hebrew year with Adar I and Adar II.
fn hebrew_leap(year: i32) -> bool {
    icu_calendar::Date::try_new(year.into(), Month::leap(5), 1, icu::Hebrew).is_ok()
}

/// Why a calendar date can't be converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Invalid {
    /// The month has only this many days.
    Day(u8),
    /// The month doesn't exist (in that year).
    Month,
    /// The year is outside the calendar's table, or the result outside 0001-9999.
    Range,
}

fn icu_date<C: AsCalendar>(
    year: i32,
    month: u8,
    leap: bool,
    day: u8,
    cal: C,
) -> Result<Date, Invalid> {
    let month = if leap {
        Month::leap(month)
    } else {
        Month::new(month)
    };
    let d = icu_calendar::Date::try_new(year.into(), month, day, cal).map_err(|e| match e {
        DateNewError::InvalidDay { max } => Invalid::Day(max),
        DateNewError::MonthNotInCalendar | DateNewError::MonthNotInYear => Invalid::Month,
        _ => Invalid::Range,
    })?;
    let g = d.to_calendar(Iso);
    let year = i16::try_from(g.year().extended_year()).map_err(|_| Invalid::Range)?;
    let g = Date::new(year, g.month().ordinal as i8, g.day_of_month().0 as i8);
    g.ok().filter(|g| g.year() >= 1).ok_or(Invalid::Range)
}

/// A date shown in every calendar but its own, or in one.
#[derive(Debug, PartialEq)]
pub struct Converted {
    pub input: String,
    pub from: Calendar,
    pub gregorian: Date,
    pub dates: Vec<CalDate>,
    pub warnings: Vec<Warning>,
}

/// Shows a date in every calendar but its own, or only in `only`. A calendar whose table
/// doesn't cover it is left out with a warning, or fails when it is `only`.
pub fn convert(
    input: &str,
    from: Option<Calendar>,
    only: Option<Calendar>,
) -> Result<Converted, Error> {
    let (from, gregorian) = read(input, from)?;
    // The input's own calendar isn't shown, so its caution and note become warnings.
    let source = Some(from).filter(|&c| c != Gregorian);
    let source = source.and_then(|c| in_calendar(c, gregorian).ok());
    let mut warnings: Vec<_> = source
        .iter()
        .flat_map(|d| {
            let note = d.note.map(|n| Warning {
                code: "calendar_note",
                message: n.into(),
            });
            caution(d).into_iter().chain(note)
        })
        .collect();
    let wanted = Calendar::ALL
        .into_iter()
        .filter(|&c| c != Gregorian && only.map_or(c != from, |t| t == c));
    let mut dates = vec![];
    for cal in wanted {
        match in_calendar(cal, gregorian) {
            Ok(d) => dates.push(d),
            Err(e) if only.is_none() => warnings.push(Warning {
                code: e.code,
                message: format!("{}; left out.", e.message.trim_end_matches('.')),
            }),
            Err(e) => return Err(e),
        }
    }
    warnings.extend(dates.iter().filter_map(caution));
    const ORDER: [&str; 3] = [
        "calendar_out_of_range",
        "bs_projected",
        "chinese_calculated",
    ];
    warnings.sort_by_key(|w| ORDER.iter().position(|&c| c == w.code));
    warnings.dedup();
    Ok(Converted {
        input: input.into(),
        from,
        gregorian,
        dates,
        warnings,
    })
}

/// Spellings people also use, beside the month names results show.
const VARIANTS: [(&str, Calendar, u8, bool); 22] = [
    ("Rabi I", UmmAlQura, 3, false),
    ("Rabi II", UmmAlQura, 4, false),
    ("Jumada I", UmmAlQura, 5, false),
    ("Jumada II", UmmAlQura, 6, false),
    ("Shaaban", UmmAlQura, 8, false),
    ("Dhul Qadah", UmmAlQura, 11, false),
    ("Dhul Hijjah", UmmAlQura, 12, false),
    ("Heshvan", Hebrew, 2, false),
    ("Marcheshvan", Hebrew, 2, false),
    ("Teves", Hebrew, 4, false),
    ("Shvat", Hebrew, 5, false),
    ("Adar I", Hebrew, 5, true),
    ("Adar II", Hebrew, 6, false),
    ("Baishakh", BikramSambat, 1, false),
    ("Jeth", BikramSambat, 2, false),
    ("Asar", BikramSambat, 3, false),
    ("Saun", BikramSambat, 4, false),
    ("Bhadau", BikramSambat, 5, false),
    ("Ashwin", BikramSambat, 6, false),
    ("Push", BikramSambat, 9, false),
    ("Fagun", BikramSambat, 11, false),
    ("Chaitra", BikramSambat, 12, false),
];

/// A month name in lower case without spaces, hyphens and apostrophes.
fn key(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '\'' | '\u{2019}'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// A month by name, in any case, ignoring spaces, hyphens and apostrophes: its calendar
/// (Islamic names give Umm al-Qura; `Month N` and `Leap month N` Chinese), number and whether
/// it is a leap month.
pub(crate) fn month_by_name(name: &str) -> Option<(Calendar, u8, bool)> {
    let wanted = key(name);
    for (prefix, leap) in [("leapmonth", true), ("month", false)] {
        if let Some(n) = wanted.strip_prefix(prefix) {
            return Some((Chinese, number(n, 2)?, leap));
        }
    }
    let tables = [
        (UmmAlQura, &ISLAMIC),
        (Hebrew, &HEBREW),
        (Persian, &PERSIAN),
        (BikramSambat, &BIKRAM_SAMBAT),
    ];
    let named = tables
        .into_iter()
        .flat_map(|(cal, names)| names.iter().zip(1..).map(move |(&n, m)| (n, cal, m, false)));
    let (_, cal, month, leap) = named.chain(VARIANTS).find(|v| key(v.0) == wanted)?;
    Some((cal, month, leap))
}

/// Digits only, at most `max` of them.
fn number<T: std::str::FromStr>(text: &str, max: usize) -> Option<T> {
    let digits = !text.is_empty() && text.len() <= max && text.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| text.parse().ok())?
}

/// Reads `D MonthName YYYY` (an optional AH, SH or BS after it; the name picks the calendar,
/// `from` may only change Umm al-Qura to tabular), or `YYYY-MM-DD` in `from` (Gregorian when
/// not given) with an `L` after a leap month's number (`5787-05L-01`).
fn read(input: &str, from: Option<Calendar>) -> Result<(Calendar, Date), Error> {
    let mut words: Vec<_> = input.split_whitespace().collect();
    if words.len() > 3
        && ["ah", "sh", "bs"].contains(&words[words.len() - 1].to_lowercase().as_str())
    {
        words.pop();
    }
    if let [day, name @ .., year] = &words[..]
        && let (Some(day), Some(year)) = (number(day, 2), number(year, 4))
        && let Some((cal, month, leap)) = month_by_name(&name.join(" "))
    {
        let missing = |in_cal: Calendar, month: String| {
            let text = format!(
                "There is no {day} {month} {year} in the {} calendar.",
                in_cal.name()
            );
            Error::new("bad_calendar_date", text)
        };
        if cal == Hebrew && key(&name.join(" ")) == "adarii" && !hebrew_leap(year) {
            return Err(missing(Hebrew, "Adar II".into()));
        }
        let cal = match from {
            None => cal,
            Some(IslamicCivil) if cal == UmmAlQura => IslamicCivil,
            Some(f) if f == cal => cal,
            Some(f) => return Err(missing(f, month_name(cal, month, leap, false))),
        };
        return Ok((cal, to_gregorian(cal, year, month, leap, day)?));
    }
    let cal = from.unwrap_or(Gregorian);
    if cal == Gregorian {
        return Ok((cal, crate::input::date(input)?));
    }
    let bad = || crate::input::bad_date(input);
    let [year, month, day] = input.split('-').collect::<Vec<_>>()[..] else {
        return Err(bad());
    };
    let (month, leap) = match month.strip_suffix(['L', 'l']) {
        Some(m) => (m, true),
        None => (month, false),
    };
    let fields = (year.len() == 4, month.len() == 2, day.len() == 2);
    let (Some(year), Some(month), Some(day), (true, true, true)) =
        (number(year, 4), number(month, 2), number(day, 2), fields)
    else {
        return Err(bad());
    };
    Ok((cal, to_gregorian(cal, year, month, leap, day)?))
}

/// Warnings `bs_projected` and `chinese_calculated` for a date shown or read.
fn caution(d: &CalDate) -> Option<Warning> {
    let (code, message) = match d.calendar {
        BikramSambat if d.year > 2083 => (
            "bs_projected",
            "Bikram Sambat dates after 2083 BS come from a projected table; Nepal publishes each year's official calendar about a year ahead.",
        ),
        Chinese if !(1900..=2102).contains(&d.year) => (
            "chinese_calculated",
            "Chinese dates outside 1900-2102 are calculated, not from the published tables, and may be a day off.",
        ),
        _ => return None,
    };
    Some(Warning {
        code,
        message: message.into(),
    })
}

/// nepali-datetime 1.0.8.5's table: a header, then a year and its 12 month lengths per line.
const BS_TABLE: &str = include_str!("../data/calendar_bs.csv");
/// 1 Baisakh of the table's first year, 1975 BS.
const BS_EPOCH: Date = date(1918, 4, 13);

/// Each year of the Bikram Sambat table with its month lengths, read once.
fn bs_years() -> impl Iterator<Item = (i32, [u8; 12])> {
    static YEARS: LazyLock<Vec<(i32, [u8; 12])>> = LazyLock::new(|| {
        let row = |line: &str| {
            let mut fields = line.split(',').map(|n| n.parse::<i32>().unwrap());
            let year = fields.next().unwrap();
            (year, std::array::from_fn(|_| fields.next().unwrap() as u8))
        };
        BS_TABLE.lines().skip(1).map(row).collect()
    });
    YEARS.iter().copied()
}

/// The Bikram Sambat year, month and day of a Gregorian date inside the table.
fn bs_from(g: Date) -> Option<(i32, u8, u8)> {
    let mut left = (g - BS_EPOCH).get_days();
    if left < 0 {
        return None;
    }
    for (year, months) in bs_years() {
        for (i, len) in months.into_iter().enumerate() {
            if left < i32::from(len) {
                return Some((year, i as u8 + 1, left as u8 + 1));
            }
            left -= i32::from(len);
        }
    }
    None
}

/// The Gregorian date of a Bikram Sambat date.
fn bs_to(year: i32, month: u8, day: u8) -> Result<Date, Invalid> {
    let mut days = 0;
    for (y, months) in bs_years() {
        if y == year {
            let i = usize::from(month).wrapping_sub(1);
            let len = *months.get(i).ok_or(Invalid::Month)?;
            if day == 0 || day > len {
                return Err(Invalid::Day(len));
            }
            let before: i32 = months[..i].iter().map(|&n| i32::from(n)).sum();
            return Ok(BS_EPOCH + jiff::Span::new().days(days + before + i32::from(day) - 1));
        }
        days += months.iter().map(|&n| i32::from(n)).sum::<i32>();
    }
    Err(Invalid::Range)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(cal: Calendar, y: i32, m: u8, d: u8, name: &str, text: &str) -> CalDate {
        CalDate {
            calendar: cal,
            year: y,
            month: m,
            leap_month: false,
            day: d,
            month_name: name.into(),
            text: text.into(),
            cycle: (cal == Chinese).then(|| ("Bing-Wu".to_string(), "Horse")),
            note: in_calendar(cal, date(2026, 10, 27)).unwrap().note,
        }
    }

    #[test]
    fn shows_a_date_in_every_calendar() {
        let r = convert("2026-10-27", None, None).unwrap();
        assert_eq!(
            (r.from, r.gregorian, r.input.as_str()),
            (Gregorian, date(2026, 10, 27), "2026-10-27")
        );
        assert_eq!(
            r.dates,
            [
                shown(
                    UmmAlQura,
                    1448,
                    5,
                    16,
                    "Jumada al-Ula",
                    "16 Jumada al-Ula 1448 AH"
                ),
                shown(
                    IslamicCivil,
                    1448,
                    5,
                    15,
                    "Jumada al-Ula",
                    "15 Jumada al-Ula 1448 AH"
                ),
                shown(Hebrew, 5787, 2, 16, "Cheshvan", "16 Cheshvan 5787"),
                shown(
                    Chinese,
                    2026,
                    9,
                    18,
                    "Month 9",
                    "Month 9, day 18, year of the Horse (Bing-Wu)"
                ),
                shown(Persian, 1405, 8, 5, "Aban", "5 Aban 1405 SH"),
                shown(BikramSambat, 2083, 7, 10, "Kartik", "10 Kartik 2083 BS"),
            ]
        );
        assert_eq!(
            r.dates[0].note,
            Some(
                "Saudi Arabia's official Umm al-Qura calendar. Religious dates set by moon sighting can differ by a day, and more in other countries."
            )
        );
        assert_eq!(
            r.dates[1].note,
            Some(
                "Tabular Islamic calendar (Friday epoch, 30-year cycle); usually 0-2 days from Umm al-Qura."
            )
        );
        assert!(r.dates[2..].iter().all(|d| d.note.is_none()));
        assert!(r.warnings.is_empty());
        let ramadan = convert("1 Ramadan 1448", None, Some(Gregorian)).unwrap();
        assert_eq!(
            ramadan.warnings,
            [Warning {
                code: "calendar_note",
                message: r.dates[0].note.unwrap().into(),
            }]
        );
        let only = convert("2026-10-27", None, Some(Persian)).unwrap();
        assert_eq!(
            only.dates,
            [shown(Persian, 1405, 8, 5, "Aban", "5 Aban 1405 SH")]
        );
        assert!(
            convert("2026-10-27", None, Some(Gregorian))
                .unwrap()
                .dates
                .is_empty()
        );
    }

    #[test]
    fn names_leap_months() {
        // 5787 is a Hebrew leap year: Adar I, then Adar II.
        let hebrew = |g| in_calendar(Hebrew, g).unwrap();
        assert_eq!(hebrew(date(2027, 2, 8)).text, "1 Adar I 5787");
        assert!(hebrew(date(2027, 2, 8)).leap_month);
        assert_eq!(hebrew(date(2027, 3, 10)).text, "1 Adar II 5787");
        assert_eq!(hebrew(date(2026, 2, 18)).text, "1 Adar 5786");
        let leap = in_calendar(Chinese, date(2025, 7, 25)).unwrap();
        assert_eq!(leap.text, "Leap month 6, day 1, year of the Snake (Yi-Si)");
        assert_eq!((leap.month, leap.leap_month), (6, true));
    }

    #[test]
    fn reads_dates_by_their_month_numbers() {
        let g = to_gregorian;
        assert_eq!(g(UmmAlQura, 1448, 1, false, 1), Ok(date(2026, 6, 16)));
        assert_eq!(g(UmmAlQura, 1447, 9, false, 1), Ok(date(2026, 2, 18)));
        assert_eq!(g(Hebrew, 5787, 1, false, 1), Ok(date(2026, 9, 12)));
        assert_eq!(g(Hebrew, 5787, 5, true, 1), Ok(date(2027, 2, 8)));
        assert_eq!(g(Chinese, 2026, 1, false, 1), Ok(date(2026, 2, 17)));
        assert_eq!(g(Chinese, 2027, 1, false, 1), Ok(date(2027, 2, 6)));
        assert_eq!(g(Chinese, 2025, 6, true, 1), Ok(date(2025, 7, 25)));
        assert_eq!(g(Persian, 1405, 1, false, 1), Ok(date(2026, 3, 21)));
        assert_eq!(g(BikramSambat, 2083, 1, false, 1), Ok(date(2026, 4, 14)));
        assert_eq!(g(BikramSambat, 2084, 1, false, 1), Ok(date(2027, 4, 14)));
        let e = g(UmmAlQura, 1447, 2, false, 30).unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str(), e.hint.as_deref()),
            (
                "bad_calendar_date",
                "There is no 30 Safar 1447 in the Umm al-Qura calendar.",
                Some("Safar 1447 has 29 days.")
            )
        );
        let e = g(Hebrew, 5786, 5, true, 1).unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str(), e.hint),
            (
                "bad_calendar_date",
                "There is no 1 Adar I 5786 in the Hebrew calendar.",
                None
            )
        );
        let message = |cal, y, m, leap, d| g(cal, y, m, leap, d).unwrap_err().message;
        assert_eq!(
            message(Chinese, 2026, 6, true, 1),
            "There is no leap month 6, day 1, 2026 in the Chinese calendar."
        );
        assert_eq!(
            message(Persian, 1405, 13, false, 1),
            "There is no 1 Month 13 1405 in the Persian calendar."
        );
        let hint = |cal, y, m, d| g(cal, y, m, false, d).unwrap_err().hint;
        assert_eq!(
            hint(BikramSambat, 2083, 1, 32).as_deref(),
            Some("Baisakh 2083 has 31 days.")
        );
        assert_eq!(g(Gregorian, 2026, 10, false, 27), Ok(date(2026, 10, 27)));
        assert_eq!(
            g(Gregorian, 2026, 2, false, 30).unwrap_err().code,
            "bad_date"
        );
        let e = g(UmmAlQura, 1601, 1, false, 1).unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str()),
            (
                "calendar_out_of_range",
                "The Umm al-Qura table covers 1300-1600 AH (1882-11-12 to 2174-11-25)."
            )
        );
        assert_eq!(
            message(BikramSambat, 1974, 1, false, 1),
            "The Bikram Sambat table covers 1975-2100 BS (1918-04-13 to 2044-04-12)."
        );
        assert_eq!(
            g(Hebrew, 1, 1, false, 1).unwrap_err().code,
            "date_out_of_range"
        );
        assert_eq!(
            g(Persian, 9999, 1, false, 1).unwrap_err().code,
            "date_out_of_range"
        );
    }

    #[test]
    fn leaves_out_calendars_whose_tables_end() {
        let r = convert("1882-11-11", None, None).unwrap();
        let left: Vec<_> = r.dates.iter().map(|d| d.calendar).collect();
        assert_eq!(left, [IslamicCivil, Hebrew, Chinese, Persian]);
        let messages: Vec<_> = r
            .warnings
            .iter()
            .map(|w| (w.code, w.message.as_str()))
            .collect();
        assert_eq!(
            messages,
            [
                (
                    "calendar_out_of_range",
                    "The Umm al-Qura table covers 1300-1600 AH (1882-11-12 to 2174-11-25); left out."
                ),
                (
                    "calendar_out_of_range",
                    "The Bikram Sambat table covers 1975-2100 BS (1918-04-13 to 2044-04-12); left out."
                ),
                (
                    "chinese_calculated",
                    "Chinese dates outside 1900-2102 are calculated, not from the published tables, and may be a day off."
                ),
            ]
        );
        let e = convert("1882-11-11", None, Some(UmmAlQura)).unwrap_err();
        assert_eq!(e.code, "calendar_out_of_range");
        assert!(convert("1882-11-12", None, Some(UmmAlQura)).is_ok());
        assert!(convert("2174-11-25", None, Some(UmmAlQura)).is_ok());
        assert!(convert("2174-11-26", None, Some(UmmAlQura)).is_err());
        let r = convert("2027-04-14", None, None).unwrap();
        let w = (r.warnings[0].code, r.warnings[0].message.as_str());
        assert_eq!(
            w,
            (
                "bs_projected",
                "Bikram Sambat dates after 2083 BS come from a projected table; Nepal publishes each year's official calendar about a year ahead."
            )
        );
        assert!(
            convert("2027-04-13", None, None)
                .unwrap()
                .warnings
                .is_empty()
        );
        assert!(convert("0001-01-01", None, None).is_ok());
        assert!(convert("9999-12-31", None, None).is_ok());
    }

    fn read_as(input: &str, from: Option<Calendar>) -> (Calendar, Date) {
        let r = convert(input, from, None).unwrap();
        assert!(r.dates.iter().all(|d| d.calendar != r.from), "{input}");
        (r.from, r.gregorian)
    }

    #[test]
    fn reads_dates_by_number_or_month_name() {
        let june16 = (UmmAlQura, date(2026, 6, 16));
        assert_eq!(read_as("1 Muharram 1448", None), june16);
        assert_eq!(read_as("1448-01-01", Some(UmmAlQura)), june16);
        assert_eq!(
            read_as("1 Ramadan 1447", None),
            (UmmAlQura, date(2026, 2, 18))
        );
        assert_eq!(read_as("1 Tishrei 5787", None), (Hebrew, date(2026, 9, 12)));
        let adar1 = (Hebrew, date(2027, 2, 8));
        assert_eq!(read_as("5787-05L-01", Some(Hebrew)), adar1);
        assert_eq!(read_as("1 Adar I 5787", None), adar1);
        // Plain Adar in a leap year is Adar II.
        assert_eq!(read_as("1 Adar 5787", None), (Hebrew, date(2027, 3, 10)));
        assert_eq!(
            read_as("2026-01-01", Some(Chinese)),
            (Chinese, date(2026, 2, 17))
        );
        assert_eq!(
            read_as("2025-06l-01", Some(Chinese)),
            (Chinese, date(2025, 7, 25))
        );
        assert_eq!(
            read_as("1405-01-01", Some(Persian)),
            (Persian, date(2026, 3, 21))
        );
        assert_eq!(
            read_as("1 Farvardin 1405 SH", None),
            (Persian, date(2026, 3, 21))
        );
        let bs = (BikramSambat, date(2026, 4, 14));
        assert_eq!(read_as("2083-01-01", Some(BikramSambat)), bs);
        assert_eq!(
            read_as("10 Kartik 2083", None),
            (BikramSambat, date(2026, 10, 27))
        );
        assert_eq!(
            read_as("2026-10-27", Some(Gregorian)),
            (Gregorian, date(2026, 10, 27))
        );
        let r = convert("2084-01-01", Some(BikramSambat), None).unwrap();
        assert_eq!(r.gregorian, date(2027, 4, 14));
        let codes: Vec<_> = r.warnings.iter().map(|w| w.code).collect();
        assert_eq!(codes, ["bs_projected"]);
        let r = convert("1899-01-01", Some(Chinese), Some(Chinese)).unwrap();
        assert_eq!(r.dates.len(), 1);
        assert_eq!(r.warnings.len(), 1);
        assert_eq!(r.warnings[0].code, "chinese_calculated");
    }

    #[test]
    fn reads_month_name_spellings() {
        // 2026-10-27 is 16 Jumada al-Ula in Umm al-Qura and 15 in the tabular calendar.
        let tabular = (IslamicCivil, date(2026, 10, 28));
        assert_eq!(
            read_as("16 Jumada al-Ula 1448", Some(IslamicCivil)),
            tabular
        );
        for (input, expected) in [
            (
                "16 jumada al-ula 1448 ah",
                read_as("16 Jumada al-Ula 1448", None),
            ),
            ("1 Rabi I 1448", read_as("1 Rabi al-Awwal 1448", None)),
            ("1 Jumada II 1448", read_as("1 Jumada al-Akhira 1448", None)),
            ("1 Dhul Hijjah 1447", read_as("1 Dhu al-Hijja 1447", None)),
            ("1 Shaaban 1447", read_as("1 Shaban 1447", None)),
            ("1 Marcheshvan 5787", read_as("1 Cheshvan 5787", None)),
            ("1 Adar II 5787", read_as("1 Adar 5787", None)),
            ("1 Baishakh 2083 BS", read_as("1 Baisakh 2083", None)),
            ("1 Chaitra 2082", read_as("1 Chait 2082", None)),
            ("1 dhul-qadah 1447", read_as("1 Dhu al-Qada 1447", None)),
        ] {
            assert_eq!(read_as(input, None), expected, "{input}");
        }
    }

    #[test]
    fn rejects_dates_that_dont_read() {
        let fail = |input, from| convert(input, from, None).unwrap_err();
        let e = fail("30 Safar 1447", None);
        assert_eq!(e.code, "bad_calendar_date");
        assert_eq!(e.hint.as_deref(), Some("Safar 1447 has 29 days."));
        assert_eq!(fail("5786-05L-01", Some(Hebrew)).code, "bad_calendar_date");
        assert_eq!(
            fail("1 Ramadan 1448", Some(Hebrew)).message,
            "There is no 1 Ramadan 1448 in the Hebrew calendar."
        );
        for (input, from) in [
            ("1 Foo 1448", None),
            ("2083-1-01", Some(BikramSambat)),
            ("today", Some(BikramSambat)),
            ("2083-01-01-01", Some(Persian)),
            ("123 Ramadan 1448", None),
        ] {
            let e = fail(input, from);
            assert_eq!(e.code, "bad_date", "{input}");
            assert_eq!(e.message, format!("Not a date: '{input}'."));
        }
        assert_eq!(fail("1 Muharram 1700", None).code, "calendar_out_of_range");
        assert_eq!(fail("1 Baisakh 2101", None).code, "calendar_out_of_range");
    }

    #[test]
    fn reads_calendar_names_and_aliases() {
        for (text, cal) in [
            ("Hijri", UmmAlQura),
            ("islamic_umalqura", UmmAlQura),
            ("tabular", IslamicCivil),
            ("JALALI", Persian),
            ("solar-hijri", Persian),
            ("bs", BikramSambat),
            ("nepali", BikramSambat),
        ] {
            assert_eq!(Calendar::parse(text), Ok(cal), "{text}");
        }
        for cal in Calendar::ALL {
            assert_eq!(Calendar::parse(cal.id()), Ok(cal));
        }
        let e = Calendar::parse("julian").unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str(), e.hint.as_deref()),
            (
                "unknown_calendar",
                "Unknown calendar: 'julian'.",
                Some(
                    "Use gregorian, islamic-umalqura, islamic-civil, hebrew, chinese, persian or bikram-sambat."
                )
            )
        );
    }

    #[test]
    fn bikram_sambat_follows_the_table() {
        assert_eq!(bs_to(1975, 1, 1), Ok(date(1918, 4, 13)));
        assert_eq!(bs_to(2100, 12, 30), Ok(date(2044, 4, 12)));
        assert_eq!(bs_from(date(1918, 4, 12)), None);
        assert_eq!(bs_from(date(2044, 4, 13)), None);
        // 2062's disputed Baisakh/Jestha split is nepali-datetime's 31/31, so Jestha 23 is
        // 2005-06-06 as Hamro Patro has it.
        assert_eq!(bs_to(2062, 1, 31), Ok(date(2005, 5, 14)));
        assert_eq!(bs_to(2062, 2, 23), Ok(date(2005, 6, 6)));
        assert_eq!(bs_to(2083, 1, 0), Err(Invalid::Day(31)));
        assert_eq!(bs_to(2083, 0, 1), Err(Invalid::Month));
        assert_eq!(bs_to(2101, 1, 1), Err(Invalid::Range));
        // Every day of the table converts both ways.
        let mut g = BS_EPOCH;
        while let Some((y, m, d)) = bs_from(g) {
            assert_eq!(bs_to(y, m, d), Ok(g));
            g = g.tomorrow().unwrap();
        }
        assert_eq!(g, date(2044, 4, 13));
    }
}
