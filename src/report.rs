//! Each operation's answer as one value: what `--json` prints and text output formats.

use jiff::civil::Date;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Number;

use crate::calendars::{self, CalDate, Calendar};
use crate::data;
use crate::days::{self, Day, Entry, Place, Roll, Warning};
use crate::error::Error;
use crate::hours;
use crate::input::{self, Unit};

/// The bundled IANA tzdata release, e.g. "2026f".
pub const TZDATA: &str = match jiff_tzdb::VERSION {
    Some(v) => v,
    None => "unknown",
};
pub const CALENDARS: &str = "icu_calendar 2.4.0";
pub const BIKRAM_SAMBAT: &str = "nepali-datetime 1.0.8.5 (2026-05-07)";
pub const DEFAULT_HOURS: &str = "09:00-17:00";

/// The settings an operation may use besides its own arguments; `None` means the default.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub country: Option<String>,
    pub region: Option<String>,
    pub weekend: Option<String>,
    pub hours: Option<String>,
    pub timezone: Option<String>,
    pub exclude_start: bool,
    pub exclude_end: bool,
}

/// One operation with its arguments as typed.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Between {
        start: String,
        end: String,
    },
    Hours {
        start: String,
        end: String,
    },
    Add {
        start: String,
        amount: String,
        unit: Unit,
        roll: Roll,
    },
    /// A year, a range, or neither for the current year; the country comes from the options.
    Holidays {
        year: Option<i16>,
        from: Option<String>,
        to: Option<String>,
    },
    Convert {
        date: String,
        from: Option<String>,
        to: Option<String>,
    },
    /// Every country, or the one in the options with its regions.
    Countries,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Answer {
    Between(Between),
    Add(Added),
    Hours(Hours),
    Holidays(Holidays),
    Convert(Converted),
    Countries(Countries),
    Country(Country),
}

impl Answer {
    /// The operation's name, as the page's API reports it.
    pub fn op(&self) -> &'static str {
        match self {
            Answer::Between(_) => "between",
            Answer::Add(_) => "add",
            Answer::Hours(_) => "hours",
            Answer::Holidays(_) => "holidays",
            Answer::Convert(_) => "convert",
            Answer::Countries(_) | Answer::Country(_) => "countries",
        }
    }
}

/// The data and versions every answer is computed from.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Data {
    pub holidays: String,
    pub tzdata: &'static str,
    pub calendars: &'static str,
    pub bikram_sambat: &'static str,
}

fn data() -> Data {
    let (version, released) = data::source();
    Data {
        holidays: format!("python-holidays {version} ({released})"),
        tzdata: TZDATA,
        calendars: CALENDARS,
        bikram_sambat: BIKRAM_SAMBAT,
    }
}

/// A country or region: its code and English name (a region's first alias, or its code).
#[derive(Debug, Serialize, JsonSchema)]
pub struct Named {
    pub code: &'static str,
    pub name: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Holiday {
    #[schemars(with = "String")]
    pub date: Date,
    pub weekday: &'static str,
    pub name: &'static str,
    pub observed: bool,
    pub estimated: bool,
    /// Listings only: `national` or `regional`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<&'static str>,
    /// Regions that don't keep this national holiday.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub except_regions: Vec<&'static str>,
    /// For a holiday only some regions keep: those regions.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<&'static str>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Between {
    #[schemars(with = "String")]
    pub start: Date,
    #[schemars(with = "String")]
    pub end: Date,
    pub include_start: bool,
    pub include_end: bool,
    pub country: Option<Named>,
    pub region: Option<Named>,
    pub business_days: i64,
    pub calendar_days: i64,
    pub weekend_days: i64,
    pub holiday_days: i64,
    pub elapsed_days: i64,
    pub elapsed: String,
    pub holidays: Vec<Holiday>,
    #[schemars(with = "Vec<String>")]
    pub weekend_workdays: Vec<Date>,
    pub weekend: Vec<&'static str>,
    pub warnings: Vec<Warning>,
    pub data: Data,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Skipped {
    #[schemars(with = "String")]
    pub date: Date,
    pub reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'static str>,
}

/// The working hours and zone a business-hours answer used.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Clock {
    pub hours: String,
    pub timezone: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Added {
    /// A date, or a date-time for business hours.
    pub start: String,
    /// Business hours may have decimals.
    pub amount: Number,
    pub unit: &'static str,
    pub result: String,
    pub weekday: &'static str,
    pub country: Option<Named>,
    pub region: Option<Named>,
    pub roll: &'static str,
    #[serde(flatten)]
    pub clock: Option<Clock>,
    pub skipped: Vec<Skipped>,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_more: usize,
    #[schemars(with = "Vec<String>")]
    pub weekend_workdays: Vec<Date>,
    pub weekend: Vec<&'static str>,
    pub warnings: Vec<Warning>,
    pub data: Data,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Hours {
    pub start: String,
    pub end: String,
    pub country: Option<Named>,
    pub region: Option<Named>,
    #[serde(flatten)]
    pub clock: Clock,
    pub seconds: i64,
    pub business_hours: f64,
    /// `H:MM`, minutes rounded down, `-` when negative.
    pub duration: String,
    pub business_days: i64,
    pub holidays: Vec<Holiday>,
    #[schemars(with = "Vec<String>")]
    pub weekend_workdays: Vec<Date>,
    pub weekend: Vec<&'static str>,
    pub warnings: Vec<Warning>,
    pub data: Data,
}

const WEEKDAYS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];

fn weekday(d: Date) -> &'static str {
    WEEKDAYS[d.weekday().to_monday_zero_offset() as usize]
}

/// Weekend days as JSON lists them: `["SAT", "SUN"]`.
fn weekend(days: u8) -> Vec<&'static str> {
    (0..7)
        .filter(|i| days >> i & 1 == 1)
        .map(|i| WEEKDAYS[i])
        .collect()
}

fn holiday(e: &Entry) -> Holiday {
    Holiday {
        date: e.date,
        weekday: weekday(e.date),
        name: e.name,
        observed: e.observed,
        estimated: e.estimated,
        scope: None,
        except_regions: vec![],
        regions: vec![],
    }
}

fn holidays(entries: &[Entry]) -> Vec<Holiday> {
    entries.iter().map(holiday).collect()
}

fn place(o: &Options) -> Result<Place, Error> {
    let weekend = o.weekend.as_deref().map(input::weekend).transpose()?;
    Place::new(o.country.as_deref(), o.region.as_deref(), weekend)
}

/// The place's country and region for an answer.
fn named(place: &Place) -> (Option<Named>, Option<Named>) {
    let Some(b) = &place.block else {
        return (None, None);
    };
    let region = place.region.map(|r| {
        let r = &b.regions[r];
        Named {
            code: r.code,
            name: r.names.first().copied().unwrap_or(r.code),
        }
    });
    let country = Named {
        code: b.country.code,
        name: b.country.name,
    };
    (Some(country), region)
}

/// Working hours and zone from the options, as given or by default.
fn clock(o: &Options) -> Result<(Vec<hours::Window>, Option<jiff::tz::TimeZone>), Error> {
    let windows = input::hours(o.hours.as_deref().unwrap_or(DEFAULT_HOURS))?;
    let zone = o.timezone.as_deref().map(input::timezone).transpose()?;
    Ok((windows, zone))
}

fn clock_info(windows: &[hours::Window], zone: &Option<jiff::tz::TimeZone>) -> Clock {
    let hours: Vec<_> = windows.iter().map(ToString::to_string).collect();
    Clock {
        hours: hours.join(","),
        timezone: zone.as_ref().map(|z| z.iana_name().unwrap_or("UTC").into()),
    }
}

/// Runs one operation.
pub fn run(op: &Op, o: &Options) -> Result<Answer, Error> {
    match op {
        Op::Between { start, end } => between(start, end, o).map(Answer::Between),
        Op::Hours { start, end } => hours(start, end, o).map(Answer::Hours),
        Op::Add {
            start,
            amount,
            unit,
            roll,
        } => add(start, amount, *unit, *roll, o).map(Answer::Add),
        Op::Holidays { year, from, to } => {
            listing(*year, from.as_deref(), to.as_deref(), o).map(Answer::Holidays)
        }
        Op::Convert { date, from, to } => {
            convert(date, from.as_deref(), to.as_deref()).map(Answer::Convert)
        }
        Op::Countries => match &o.country {
            None => Ok(Answer::Countries(Countries {
                countries: data::countries().map(|c| info(&c)).collect(),
                data: data(),
            })),
            Some(c) => country(c).map(Answer::Country),
        },
    }
}

fn between(start: &str, end: &str, o: &Options) -> Result<Between, Error> {
    let (start, end) = (input::date(start)?, input::date(end)?);
    let place = place(o)?;
    let r = days::between(&place, start, end, !o.exclude_start, !o.exclude_end)?;
    let (country, region) = named(&place);
    Ok(Between {
        start,
        end,
        include_start: r.include_start,
        include_end: r.include_end,
        country,
        region,
        business_days: r.business_days,
        calendar_days: r.calendar_days,
        weekend_days: r.weekend_days,
        holiday_days: r.holiday_days,
        elapsed_days: r.elapsed_days,
        elapsed: r.elapsed,
        holidays: holidays(&r.holidays),
        weekend_workdays: r.weekend_workdays,
        weekend: weekend(r.weekend),
        warnings: r.warnings,
        data: data(),
    })
}

fn skipped(days: &[(Date, Day)]) -> Vec<Skipped> {
    let skip = |&(date, day)| match day {
        Day::Holiday(name) => Skipped {
            date,
            reason: "holiday",
            name: Some(name),
        },
        _ => Skipped {
            date,
            reason: "weekend",
            name: None,
        },
    };
    days.iter().map(skip).collect()
}

fn add(start: &str, amount: &str, unit: Unit, roll: Roll, o: &Options) -> Result<Added, Error> {
    let business = matches!(unit, Unit::BusinessDays | Unit::BusinessHours);
    if business && roll != Roll::None {
        return Err(Error::new(
            "usage",
            "--roll works only with --unit days, weeks, months or years.",
        ));
    }
    if unit == Unit::BusinessHours {
        let (windows, zone) = clock(o)?;
        let start = input::datetime(start, zone.as_ref())?;
        let seconds = input::amount(amount, unit)?;
        let place = place(o)?;
        let r = hours::add(&place, &start, seconds, &windows)?;
        let (country, region) = named(&place);
        let amount = match seconds % 3600 {
            0 => Number::from(seconds / 3600),
            _ => Number::from_f64(seconds as f64 / 3600.0).expect("a finite amount"),
        };
        return Ok(Added {
            start: r.start.to_string(),
            amount,
            unit: unit.name(),
            result: r.result.to_string(),
            weekday: weekday(r.result.shown().date()),
            country,
            region,
            roll: roll_name(roll),
            clock: Some(clock_info(&windows, &zone)),
            skipped: skipped(&r.skipped),
            skipped_more: r.skipped_more,
            weekend_workdays: r.weekend_workdays,
            weekend: weekend(r.weekend),
            warnings: r.warnings,
            data: data(),
        });
    }
    let start = input::date(start)?;
    let amount = input::amount(amount, unit)?;
    let place = place(o)?;
    let r = days::add(&place, start, amount, unit, roll)?;
    let (country, region) = named(&place);
    Ok(Added {
        start: r.start.to_string(),
        amount: Number::from(r.amount),
        unit: unit.name(),
        result: r.result.to_string(),
        weekday: weekday(r.result),
        country,
        region,
        roll: roll_name(roll),
        clock: None,
        skipped: skipped(&r.skipped),
        skipped_more: r.skipped_more,
        weekend_workdays: r.weekend_workdays,
        weekend: weekend(r.weekend),
        warnings: r.warnings,
        data: data(),
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Holidays {
    pub country: Named,
    pub region: Option<Named>,
    #[schemars(with = "String")]
    pub from: Date,
    #[schemars(with = "String")]
    pub to: Date,
    /// National holidays, or everything the region keeps.
    pub holidays: Vec<Holiday>,
    /// Holidays only some regions keep; empty with a region.
    pub regional: Vec<Holiday>,
    #[schemars(with = "Vec<String>")]
    pub weekend_workdays: Vec<Date>,
    pub weekend: Vec<&'static str>,
    pub warnings: Vec<Warning>,
    pub data: Data,
}

fn listing(
    year: Option<i16>,
    from: Option<&str>,
    to: Option<&str>,
    o: &Options,
) -> Result<Holidays, Error> {
    let (from, to) = match (year, from, to) {
        (_, None, None) => {
            let y = year.unwrap_or_else(|| jiff::Zoned::now().year());
            if !(1..=9999).contains(&y) {
                return Err(Error::new(
                    "date_out_of_range",
                    format!("{y} is outside 0001-01-01 to 9999-12-31."),
                ));
            }
            (jiff::civil::date(y, 1, 1), jiff::civil::date(y, 12, 31))
        }
        (None, Some(from), Some(to)) => {
            let (from, to) = (input::date(from)?, input::date(to)?);
            (from.min(to), from.max(to))
        }
        _ => {
            return Err(Error::new("usage", "Give both --from and --to, or a YEAR."));
        }
    };
    let place = Place::new(o.country.as_deref(), o.region.as_deref(), None)?;
    let r = days::holidays(&place, from, to)?;
    let (country, region) = named(&place);
    let listed = |l: &days::Listed| Holiday {
        scope: Some(l.scope),
        except_regions: l.except_regions.clone(),
        ..holiday(&l.entry)
    };
    let grouped = |g: &days::Grouped| Holiday {
        regions: g.regions.clone(),
        ..holiday(&g.entry)
    };
    Ok(Holidays {
        country: country.expect("a listing has a country"),
        region,
        from,
        to,
        holidays: r.holidays.iter().map(listed).collect(),
        regional: r.regional.iter().map(grouped).collect(),
        weekend_workdays: r.weekend_workdays,
        weekend: weekend(r.weekend),
        warnings: r.warnings,
        data: data(),
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Gregorian {
    #[schemars(with = "String")]
    pub date: Date,
    pub weekday: &'static str,
    pub iso_week: String,
    pub day_of_year: i16,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CalendarDate {
    pub calendar: &'static str,
    pub year: i32,
    pub month: u8,
    pub leap_month: bool,
    pub month_name: String,
    pub day: u8,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cyclic_year: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zodiac: Option<&'static str>,
    pub note: Option<&'static str>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Converted {
    pub input: String,
    pub from: &'static str,
    pub gregorian: Gregorian,
    pub dates: Vec<CalendarDate>,
    pub warnings: Vec<Warning>,
    pub data: Data,
}

fn convert(date: &str, from: Option<&str>, to: Option<&str>) -> Result<Converted, Error> {
    let from = from.map(Calendar::parse).transpose()?;
    let to = to.map(Calendar::parse).transpose()?;
    let c = calendars::convert(date, from, to)?;
    let g = c.gregorian;
    let week = g.iso_week_date();
    let shown = |d: CalDate| {
        let (cyclic_year, zodiac) = d.cycle.unzip();
        CalendarDate {
            calendar: d.calendar.id(),
            year: d.year,
            month: d.month,
            leap_month: d.leap_month,
            month_name: d.month_name,
            day: d.day,
            text: d.text,
            cyclic_year,
            zodiac,
            note: d.note,
        }
    };
    Ok(Converted {
        input: c.input,
        from: c.from.id(),
        gregorian: Gregorian {
            date: g,
            weekday: weekday(g),
            iso_week: format!("{:04}-W{:02}", week.year(), week.week()),
            day_of_year: g.day_of_year(),
        },
        dates: c.dates.into_iter().map(shown).collect(),
        warnings: c.warnings,
        data: data(),
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CountryInfo {
    pub code: &'static str,
    pub name: &'static str,
    pub aliases: Vec<&'static str>,
    pub region_count: usize,
    /// The national weekend at the start of the data years.
    pub weekend: Vec<&'static str>,
    pub years: [i16; 2],
    pub decree_through: Option<i16>,
}

fn info(c: &data::Country) -> CountryInfo {
    CountryInfo {
        code: c.code,
        name: c.name,
        aliases: c.aliases.clone(),
        region_count: c.region_count,
        weekend: weekend(c.weekend),
        years: [c.years.0, c.years.1],
        decree_through: c.decree_through,
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Countries {
    pub countries: Vec<CountryInfo>,
    pub data: Data,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WeekendChange {
    #[schemars(with = "String")]
    pub from: Date,
    pub weekend: Vec<&'static str>,
    pub region: Option<&'static str>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Country {
    #[serde(flatten)]
    pub country: CountryInfo,
    pub regions: Vec<data::Region>,
    pub weekend_history: Vec<WeekendChange>,
    pub data: Data,
}

fn country(text: &str) -> Result<Country, Error> {
    let (b, _) = data::find(text, None)?;
    let change = |w: &data::Weekend| WeekendChange {
        from: w.from,
        weekend: weekend(w.days),
        region: w.region.map(|r| b.regions[r].code),
    };
    Ok(Country {
        country: info(&b.country),
        weekend_history: b.weekends.iter().map(change).collect(),
        regions: b.regions,
        data: data(),
    })
}

/// The JSON name of a roll rule.
pub fn roll_name(roll: Roll) -> &'static str {
    match roll {
        Roll::None => "none",
        Roll::Following => "following",
        Roll::Preceding => "preceding",
        Roll::ModifiedFollowing => "modified_following",
        Roll::ModifiedPreceding => "modified_preceding",
    }
}

fn hours(start: &str, end: &str, o: &Options) -> Result<Hours, Error> {
    let (windows, zone) = clock(o)?;
    let (start, end) = (
        input::datetime(start, zone.as_ref())?,
        input::datetime(end, zone.as_ref())?,
    );
    let place = place(o)?;
    let r = hours::between(&place, &start, &end, &windows)?;
    let (country, region) = named(&place);
    let minutes = r.seconds.abs() / 60;
    let sign = if r.seconds < 0 { "-" } else { "" };
    Ok(Hours {
        start: r.start.to_string(),
        end: r.end.to_string(),
        country,
        region,
        clock: clock_info(&windows, &zone),
        seconds: r.seconds,
        business_hours: (r.seconds as f64 / 36.0).round() / 100.0,
        duration: format!("{sign}{}:{:02}", minutes / 60, minutes % 60),
        business_days: r.business_days,
        holidays: holidays(&r.holidays),
        weekend_workdays: r.weekend_workdays,
        weekend: weekend(r.weekend),
        warnings: r.warnings,
        data: data(),
    })
}

/// The default of a JSON flag that is on unless sent as false.
pub fn yes() -> bool {
    true
}

/// `{"error": {"code", "message", "hint"}}`, the JSON for every failure.
pub fn failure(error: &Error) -> String {
    #[derive(Serialize)]
    struct Failure<'a> {
        error: &'a Error,
    }
    serde_json::to_string(&Failure { error }).expect("errors are plain data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_calendar_crate_in_the_lock_file() {
        let lock = include_str!("../Cargo.lock");
        let version = CALENDARS.strip_prefix("icu_calendar ").unwrap();
        assert!(lock.contains(&format!(
            "name = \"icu_calendar\"\nversion = \"{version}\"\n"
        )));
    }
}
