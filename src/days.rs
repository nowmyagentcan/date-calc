//! Business days: which dates count, `between`, `add` and holiday listings.

use jiff::civil::Date;

use jiff::ToSpan;

use crate::data::{self, Block};
use crate::error::Error;
use crate::input::Unit;

const SAT_SUN: u8 = 0b110_0000;

/// Where the dates are counted: a country (and region) from the data, or weekends only.
#[derive(Debug)]
pub struct Place {
    pub block: Option<Block>,
    pub region: Option<usize>,
    /// `--weekend`, replacing the data's weekends.
    pub weekend: Option<u8>,
}

/// How a date counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Day {
    Business,
    Weekend,
    Holiday(&'static str),
}

/// Something the answer depends on that the user may not expect; it doesn't fail the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub code: &'static str,
    pub message: String,
}

/// A holiday as results list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub date: Date,
    pub name: &'static str,
    pub observed: bool,
    pub estimated: bool,
}

impl From<&data::Holiday> for Entry {
    fn from(h: &data::Holiday) -> Self {
        Self {
            date: h.date,
            name: h.name,
            observed: h.observed,
            estimated: h.estimated,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Between {
    pub start: Date,
    pub end: Date,
    pub include_start: bool,
    pub include_end: bool,
    pub business_days: i64,
    pub calendar_days: i64,
    pub weekend_days: i64,
    pub holiday_days: i64,
    pub elapsed_days: i64,
    /// ISO 8601 in years, months and days: `P9M14D`, `-P27D`, `P0D`.
    pub elapsed: String,
    pub holidays: Vec<Entry>,
    pub weekend_workdays: Vec<Date>,
    /// The weekend in force at `start`.
    pub weekend: u8,
    pub warnings: Vec<Warning>,
}

/// How `add` moves a calendar-unit result that isn't a business day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Roll {
    None,
    Following,
    Preceding,
    /// Following unless that changes the month, then preceding.
    ModifiedFollowing,
    /// Preceding unless that changes the month, then following.
    ModifiedPreceding,
}

#[derive(Debug, PartialEq)]
pub struct Added {
    pub start: Date,
    pub amount: i64,
    pub unit: Unit,
    pub result: Date,
    pub roll: Roll,
    /// The non-business days passed over (or rolled over), at most 100.
    pub skipped: Vec<(Date, Day)>,
    pub skipped_more: usize,
    pub weekend_workdays: Vec<Date>,
    /// The weekend in force at `start`.
    pub weekend: u8,
    pub warnings: Vec<Warning>,
}

const SKIPPED_MAX: usize = 100;

/// A holiday in a listing: `scope` is `national` or `regional`.
#[derive(Debug, PartialEq)]
pub struct Listed {
    pub entry: Entry,
    pub scope: &'static str,
    /// Regions that don't keep this national holiday (only when no region is chosen).
    pub except_regions: Vec<&'static str>,
}

/// A holiday only some regions keep, with their codes in sorted order.
#[derive(Debug, PartialEq)]
pub struct Grouped {
    pub entry: Entry,
    pub regions: Vec<&'static str>,
}

#[derive(Debug, PartialEq)]
pub struct Listing {
    pub from: Date,
    pub to: Date,
    /// National holidays, or everything the chosen region keeps.
    pub holidays: Vec<Listed>,
    /// Holidays only some regions keep; empty when a region is chosen.
    pub regional: Vec<Grouped>,
    pub weekend_workdays: Vec<Date>,
    /// The weekend in force at `from`.
    pub weekend: u8,
    pub warnings: Vec<Warning>,
}

impl Place {
    /// A country by code, alias or name (`CC-REGION` too), or none for weekends only.
    pub fn new(
        country: Option<&str>,
        region: Option<&str>,
        weekend: Option<u8>,
    ) -> Result<Self, Error> {
        let (block, region) = match country {
            Some(c) => data::find(c, region).map(|(b, r)| (Some(b), r))?,
            None => (None, None),
        };
        Ok(Self {
            block,
            region,
            weekend,
        })
    }

    /// The weekend days in force on a date: bit 0 is Monday.
    pub fn weekend_on(&self, date: Date) -> u8 {
        self.weekend
            .or_else(|| Some(self.block.as_ref()?.weekend(self.region, date)))
            .unwrap_or(SAT_SUN)
    }

    /// A weekend day worked by decree is a business day unless it is also a holiday.
    pub fn day(&self, date: Date) -> Day {
        let off = self.weekend_on(date) >> date.weekday().to_monday_zero_offset() & 1 == 1;
        let b = self.block.as_ref();
        if off && !b.is_some_and(|b| b.worked_by_decree(self.region, date)) {
            Day::Weekend
        } else if let Some(h) = b.and_then(|b| b.holidays(self.region, date, date).next()) {
            Day::Holiday(h.name)
        } else {
            Day::Business
        }
    }

    /// Fails with `no_holiday_data` for a date outside the country's data years.
    fn covers(&self, date: Date) -> Result<(), Error> {
        self.block.as_ref().map_or(Ok(()), |b| b.covers(date))
    }

    /// The holidays kept from `from` to `to`, both included.
    fn holidays(&self, from: Date, to: Date) -> Vec<Entry> {
        let Some(b) = &self.block else { return vec![] };
        b.holidays(self.region, from, to).map(Entry::from).collect()
    }

    /// Weekend days worked by decree from `from` to `to`, both included.
    fn workdays(&self, from: Date, to: Date) -> Vec<Date> {
        let Some(b) = &self.block else { return vec![] };
        b.decree_workdays(self.region, from, to).collect()
    }
}

/// Business days from `start` to `end`, both counted unless excluded (Excel NETWORKDAYS);
/// negative when `end` is before `start`.
pub fn between(
    place: &Place,
    start: Date,
    end: Date,
    include_start: bool,
    include_end: bool,
) -> Result<Between, Error> {
    place.covers(start)?;
    place.covers(end)?;
    let (lo, hi) = (start.min(end), start.max(end));
    let counted = |d: Date| (include_start || d != start) && (include_end || d != end);
    let (mut business, mut calendar, mut weekend, mut holiday) = (0, 0, 0, 0);
    let mut d = lo;
    loop {
        if counted(d) {
            calendar += 1;
            match place.day(d) {
                Day::Business => business += 1,
                Day::Weekend => weekend += 1,
                Day::Holiday(_) => holiday += 1,
            }
        }
        if d == hi {
            break;
        }
        d = d.tomorrow().unwrap();
    }
    let mut holidays = place.holidays(lo, hi);
    holidays.retain(|h| counted(h.date));
    let mut weekend_workdays = place.workdays(lo, hi);
    weekend_workdays.retain(|&d| counted(d));
    let warnings = day_warnings(place, lo, hi, &holidays, true);
    Ok(Between {
        start,
        end,
        include_start,
        include_end,
        business_days: if end < start { -business } else { business },
        calendar_days: calendar,
        weekend_days: weekend,
        holiday_days: holiday,
        elapsed_days: (end - start).get_days().into(),
        elapsed: iso_duration(start, end),
        holidays,
        weekend_workdays,
        weekend: place.weekend_on(start),
        warnings,
    })
}

/// Adds business days (Excel WORKDAY: `start` isn't counted, 0 returns it) or calendar units
/// (months and years clamp to the month's end), then applies `roll` to a calendar-unit result.
/// Business hours are `hours::add`.
pub fn add(
    place: &Place,
    start: Date,
    amount: i64,
    unit: Unit,
    roll: Roll,
) -> Result<Added, Error> {
    let years = place.block.as_ref().map_or((1, 9999), |b| b.country.years);
    let outside = || {
        let range = format!("{:04}-{:04}", years.0, years.1);
        Error::new(
            "result_out_of_range",
            format!("The result would be outside {range}."),
        )
        .hint("Use a smaller amount.")
    };
    let step = |d: Date, forward: bool| {
        let next = if forward { d.tomorrow() } else { d.yesterday() };
        next.ok()
            .filter(|d| (years.0..=years.1).contains(&d.year()))
            .ok_or_else(outside)
    };
    let business = unit == Unit::BusinessDays;
    if place.weekend == Some(0b111_1111) && (business && amount != 0 || roll != Roll::None) {
        return Err(Error::new(
            "no_business_days",
            "Every day is a weekend day, so there are no business days.",
        )
        .hint("Change --weekend."));
    }
    let (from, result, passed) = if business {
        place.covers(start)?;
        let (mut d, mut left, mut passed) = (start, amount.unsigned_abs(), vec![]);
        while left > 0 {
            d = step(d, amount > 0)?;
            match place.day(d) {
                Day::Business => left -= 1,
                day => passed.push((d, day)),
            }
        }
        (start, d, passed)
    } else {
        let span = match unit {
            Unit::Days => amount.days(),
            Unit::Weeks => amount.weeks(),
            Unit::Months => amount.months(),
            Unit::Years => amount.years(),
            _ => unreachable!("business hours are added by hours::add"),
        };
        let raw = start
            .checked_add(span)
            .ok()
            .filter(|d| d.year() >= 1)
            .ok_or_else(outside)?;
        if roll != Roll::None {
            place.covers(raw).map_err(|_| outside())?;
        }
        let walk = |forward| -> Result<(Date, Date, Vec<_>), Error> {
            let (mut d, mut over) = (raw, vec![]);
            loop {
                match place.day(d) {
                    Day::Business => return Ok((raw, d, over)),
                    day => over.push((d, day)),
                }
                d = step(d, forward)?;
            }
        };
        match roll {
            Roll::None => (raw, raw, vec![]),
            Roll::Following => walk(true)?,
            Roll::Preceding => walk(false)?,
            Roll::ModifiedFollowing | Roll::ModifiedPreceding => {
                let forward = roll == Roll::ModifiedFollowing;
                match walk(forward) {
                    Ok(r) if r.1.first_of_month() == raw.first_of_month() => r,
                    _ => walk(!forward)?,
                }
            }
        }
    };
    let (lo, hi) = (from.min(result), from.max(result));
    let (mut weekend_workdays, mut warnings) = (vec![], vec![]);
    if business || roll != Roll::None {
        weekend_workdays = place.workdays(lo, hi);
        weekend_workdays.retain(|&d| !business || d != start);
        warnings = day_warnings(place, lo, hi, &place.holidays(lo, hi), business);
    }
    if business && amount == 0 && place.day(start) != Day::Business {
        let in_country = match &place.block {
            Some(b) => format!(" in {}", b.country.name),
            None => String::new(),
        };
        let message = format!("{start} is not a business day{in_country}.");
        warnings.extend(warning("start_not_business_day", message));
    }
    let mut skipped = passed;
    let skipped_more = skipped.len().saturating_sub(SKIPPED_MAX);
    skipped.truncate(SKIPPED_MAX);
    Ok(Added {
        start,
        amount,
        unit,
        result,
        roll,
        skipped,
        skipped_more,
        weekend_workdays,
        weekend: place.weekend_on(start),
        warnings,
    })
}

/// The country's holidays from `from` to `to`, both included. Without a region, holidays only
/// some regions keep are grouped under `regional`.
pub fn holidays(place: &Place, from: Date, to: Date) -> Result<Listing, Error> {
    let b = place.block.as_ref().ok_or_else(|| {
        Error::new("country_required", "Listing holidays needs a country.")
            .hint("Pick a country first, e.g. DE.")
    })?;
    b.covers(from)?;
    b.covers(to)?;
    let holidays: Vec<_> = b
        .holidays(place.region, from, to)
        .map(|h| Listed {
            entry: h.into(),
            scope: if h.region.is_some() {
                "regional"
            } else {
                "national"
            },
            except_regions: codes(b, &h.except),
        })
        .collect();
    let entries: Vec<_> = holidays.iter().map(|h| h.entry.clone()).collect();
    let warnings = [schedule(place, to), estimated(&entries)];
    Ok(Listing {
        from,
        to,
        holidays,
        regional: match place.region {
            None => regional(b, from, to),
            Some(_) => vec![],
        },
        weekend_workdays: place.workdays(from, to),
        weekend: place.weekend_on(from),
        warnings: warnings.into_iter().flatten().collect(),
    })
}

fn codes(b: &Block, regions: &[usize]) -> Vec<&'static str> {
    let mut codes: Vec<_> = regions.iter().map(|&r| b.regions[r].code).collect();
    codes.sort_unstable();
    codes
}

/// Holidays only some regions keep, one entry per date and name.
fn regional(b: &Block, from: Date, to: Date) -> Vec<Grouped> {
    let mut groups: Vec<(Entry, Vec<usize>)> = vec![];
    let start = b.holidays.partition_point(|h| h.date < from);
    for h in b.holidays[start..].iter().take_while(|h| h.date <= to) {
        let Some(r) = h.region else { continue };
        let entry = Entry::from(h);
        let mut same_day = groups.iter_mut().rev().take_while(|g| g.0.date == h.date);
        match same_day.find(|g| g.0 == entry) {
            Some(g) => g.1.push(r),
            None => groups.push((entry, vec![r])),
        }
    }
    let group = |(entry, r): (Entry, Vec<usize>)| Grouped {
        entry,
        regions: codes(b, &r),
    };
    groups.into_iter().map(group).collect()
}

/// Up to 3 items joined by commas.
fn first3(items: impl Iterator<Item = String>) -> String {
    items.take(3).collect::<Vec<_>>().join(", ")
}

fn warning(code: &'static str, message: String) -> Option<Warning> {
    Some(Warning { code, message })
}

/// The warnings for counting business days from `from` to `to` with `holidays` in range.
fn day_warnings(
    place: &Place,
    from: Date,
    to: Date,
    holidays: &[Entry],
    counts_business_days: bool,
) -> Vec<Warning> {
    let Some(b) = &place.block else {
        let days = crate::input::weekend_text(place.weekend_on(from));
        let message = format!("No country given, so only weekends ({days}) are skipped.");
        return warning("no_country", message)
            .filter(|_| counts_business_days)
            .into_iter()
            .collect();
    };
    let differ = Some(regional(b, from, to))
        .filter(|g| place.region.is_none() && !g.is_empty())
        .and_then(|g| {
            let items = g.iter().map(|g| {
                let (e, regions) = (&g.entry, g.regions.join(", "));
                format!("{} {} ({regions})", e.name, e.date)
            });
            let message = format!(
                "{} has regional holidays in this range ({}); this uses national holidays only. Add --region, e.g. {}.",
                b.country.name,
                first3(items),
                b.regions[0].code
            );
            warning("regions_differ", message)
        });
    let worked = Some(place.workdays(from, to))
        .filter(|w| place.weekend.is_some() && !w.is_empty())
        .and_then(|w| {
            let message = format!(
                "Weekend days worked by decree still count as business days: {}.",
                first3(w.iter().map(Date::to_string))
            );
            warning("weekend_override", message)
        });
    [differ, schedule(place, to), estimated(holidays), worked]
        .into_iter()
        .flatten()
        .collect()
}

fn schedule(place: &Place, to: Date) -> Option<Warning> {
    let b = place.block.as_ref()?;
    let through = b.country.decree_through.filter(|&y| to.year() > y)?;
    let message = format!(
        "{} announces one-off holidays or workday swaps most years; the bundled data has them through {through}. Later dates follow the regular rules and may change.",
        b.country.name
    );
    warning("schedule_not_announced", message)
}

fn estimated(holidays: &[Entry]) -> Option<Warning> {
    let mut estimates = holidays.iter().filter(|h| h.estimated).peekable();
    estimates.peek()?;
    let message = format!(
        "Some holiday dates are estimates (lunar calendar): {}. The official date can differ by a day.",
        first3(estimates.map(|h| format!("{} {}", h.name, h.date)))
    );
    warning("estimated_dates", message)
}

fn iso_duration(start: Date, end: Date) -> String {
    let span = start.until((jiff::Unit::Year, end)).unwrap();
    let parts = [
        (span.get_years() as i64, 'Y'),
        (span.get_months() as i64, 'M'),
        (span.get_days() as i64, 'D'),
    ];
    let mut text = String::from(if span.is_negative() { "-P" } else { "P" });
    for (n, unit) in parts.into_iter().filter(|p| p.0 != 0) {
        text += &format!("{}{unit}", n.abs());
    }
    if text.ends_with('P') {
        text.push_str("0D");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn place(country: &str, region: Option<&str>) -> Place {
        Place::new(Some(country), region, None).unwrap()
    }

    #[test]
    fn a_holiday_on_a_decree_workday_is_a_holiday() {
        let cn = place("CN", None);
        assert_eq!(cn.day(date(2026, 2, 14)), Day::Business);
        assert_eq!(cn.day(date(2026, 2, 15)), Day::Weekend);
        assert_eq!(
            cn.day(date(2026, 2, 16)),
            Day::Holiday("Chinese New Year's Eve")
        );
        // US 2026-07-04 is a Saturday holiday: counted once, as a weekend day.
        let us = place("US", None);
        assert_eq!(us.day(date(2026, 7, 4)), Day::Weekend);
    }

    #[test]
    fn each_date_uses_its_own_weekend() {
        let r = between(
            &place("AE", None),
            date(2021, 12, 26),
            date(2022, 1, 8),
            true,
            true,
        );
        assert_eq!(r.unwrap().business_days, 10);
    }

    #[test]
    fn backwards_and_excluded_ends() {
        let cn = place("CN", None);
        let (feb1, feb28) = (date(2026, 2, 1), date(2026, 2, 28));
        let r = between(&cn, feb1, feb28, false, false).unwrap();
        assert_eq!((r.business_days, r.calendar_days), (15, 26));
        assert_eq!(r.weekend_workdays, [date(2026, 2, 14)]);
        // Exclusions keep naming the START and END arguments when the interval runs backwards.
        let r = between(&cn, feb28, feb1, false, true).unwrap();
        assert_eq!(r.business_days, -15);
        let one = |inc_s, inc_e| {
            between(&cn, feb28, feb28, inc_s, inc_e)
                .unwrap()
                .business_days
        };
        assert_eq!(
            (one(true, true), one(false, true), one(true, false)),
            (1, 0, 0)
        );
    }

    #[test]
    fn weekends_only_without_a_country() {
        let none = Place::new(None, None, None).unwrap();
        let r = between(&none, date(2026, 11, 2), date(2026, 11, 6), true, true).unwrap();
        assert_eq!((r.business_days, r.holidays.len()), (5, 0));
        let r = between(&none, date(2026, 1, 1), date(2026, 12, 31), true, true).unwrap();
        assert_eq!(r.business_days, 261);
        let fri_sat = Place::new(None, None, Some(0b011_0000)).unwrap();
        let r = between(&fri_sat, date(2026, 11, 1), date(2026, 11, 7), true, true).unwrap();
        assert_eq!((r.business_days, r.weekend), (5, 0b011_0000));
        let all = Place::new(None, None, Some(0b111_1111)).unwrap();
        let r = between(&all, date(2026, 11, 1), date(2026, 11, 7), true, true).unwrap();
        assert_eq!(r.business_days, 0);
        let r = between(&none, date(1, 1, 1), date(9999, 12, 31), true, true).unwrap();
        assert_eq!(r.calendar_days, 3_652_059);
    }

    #[test]
    fn elapsed_is_an_iso_duration() {
        assert_eq!(iso_duration(date(2026, 1, 1), date(2026, 10, 15)), "P9M14D");
        assert_eq!(iso_duration(date(2026, 1, 1), date(2027, 1, 1)), "P1Y");
        assert_eq!(iso_duration(date(2026, 3, 1), date(2026, 3, 1)), "P0D");
        assert_eq!(iso_duration(date(2027, 2, 3), date(2026, 1, 1)), "-P1Y1M2D");
    }

    #[test]
    fn dates_outside_the_data_fail() {
        let lk = place("LK", None);
        let e = between(&lk, date(2027, 1, 4), date(2027, 1, 5), true, true).unwrap_err();
        assert_eq!(
            e.message,
            "Holiday data for Sri Lanka covers 2003-2026; 2027-01-04 is outside it."
        );
        let e = between(&lk, date(2026, 12, 1), date(2027, 1, 5), true, true).unwrap_err();
        assert_eq!(e.code, "no_holiday_data");
    }

    fn added(country: Option<&str>, start: Date, n: i64, unit: Unit, roll: Roll) -> Added {
        let p = Place::new(country, None, None).unwrap();
        add(&p, start, n, unit, roll).unwrap()
    }

    #[test]
    fn adds_business_days_over_weekends_only() {
        let r = added(None, date(2026, 11, 2), 10, Unit::BusinessDays, Roll::None);
        assert_eq!((r.result, r.skipped.len()), (date(2026, 11, 16), 4));
        let r = added(
            None,
            date(2026, 11, 16),
            -10,
            Unit::BusinessDays,
            Roll::None,
        );
        assert_eq!(r.result, date(2026, 11, 2));
    }

    #[test]
    fn adds_calendar_units_and_rolls() {
        let months = |start, n, roll| added(Some("DE"), start, n, Unit::Months, roll).result;
        let jan31 = date(2026, 1, 31);
        assert_eq!(months(jan31, 1, Roll::None), date(2026, 2, 28));
        assert_eq!(months(jan31, 1, Roll::Following), date(2026, 3, 2));
        assert_eq!(months(jan31, 1, Roll::ModifiedFollowing), date(2026, 2, 27));
        assert_eq!(months(jan31, 1, Roll::Preceding), date(2026, 2, 27));
        // 2026-08-01 is a Saturday: preceding would leave August.
        let jul1 = date(2026, 7, 1);
        assert_eq!(months(jul1, 1, Roll::Preceding), date(2026, 7, 31));
        assert_eq!(months(jul1, 1, Roll::ModifiedPreceding), date(2026, 8, 3));
        let r = added(Some("DE"), jan31, 1, Unit::Months, Roll::ModifiedFollowing);
        assert_eq!(r.skipped, [(date(2026, 2, 28), Day::Weekend)]);
        let r = added(
            Some("DE"),
            date(2026, 10, 3),
            0,
            Unit::Days,
            Roll::Following,
        );
        assert_eq!(r.result, date(2026, 10, 5));
        assert_eq!(r.skipped.len(), 2);
        let r = added(None, jan31, 1, Unit::Months, Roll::None);
        assert_eq!((r.result, r.skipped.len()), (date(2026, 2, 28), 0));
        let unit = |u, n| added(None, date(2024, 2, 29), n, u, Roll::None).result;
        assert_eq!(unit(Unit::Days, -60), date(2023, 12, 31));
        assert_eq!(unit(Unit::Weeks, 2), date(2024, 3, 14));
        assert_eq!(unit(Unit::Years, 1), date(2025, 2, 28));
    }

    #[test]
    fn caps_skipped_days_at_100() {
        let r = added(None, date(2026, 1, 5), 100, Unit::BusinessDays, Roll::None);
        assert_eq!((r.skipped.len(), r.skipped_more), (40, 0));
        let r = added(None, date(2026, 1, 5), 1000, Unit::BusinessDays, Roll::None);
        assert_eq!((r.skipped.len(), r.skipped_more), (100, 300));
    }

    #[test]
    fn results_outside_the_range_fail() {
        let p = Place::new(Some("US"), None, None).unwrap();
        let e = add(&p, date(2050, 12, 1), 30, Unit::BusinessDays, Roll::None).unwrap_err();
        assert_eq!(e.code, "result_out_of_range");
        assert_eq!(e.message, "The result would be outside 2000-2050.");
        assert_eq!(e.hint.as_deref(), Some("Use a smaller amount."));
        let none = Place::new(None, None, None).unwrap();
        let e = add(&none, date(9999, 12, 30), 5, Unit::BusinessDays, Roll::None).unwrap_err();
        assert_eq!(e.message, "The result would be outside 0001-9999.");
        let e = add(&none, date(2, 1, 1), -2, Unit::Years, Roll::None).unwrap_err();
        assert_eq!(e.code, "result_out_of_range");
        let e = add(&p, date(2050, 12, 1), 1, Unit::Months, Roll::Following).unwrap_err();
        assert_eq!(e.code, "result_out_of_range");
        let e = add(&p, date(1999, 1, 4), 1, Unit::BusinessDays, Roll::None).unwrap_err();
        assert_eq!(e.code, "no_holiday_data");
        let all = Place::new(None, None, Some(0b111_1111)).unwrap();
        let e = add(&all, date(2026, 1, 5), 1, Unit::BusinessDays, Roll::None).unwrap_err();
        assert_eq!(e.code, "no_business_days");
        assert_eq!(
            e.message,
            "Every day is a weekend day, so there are no business days."
        );
        assert_eq!(e.hint.as_deref(), Some("Change --weekend."));
        assert!(add(&all, date(2026, 1, 5), 0, Unit::BusinessDays, Roll::None).is_ok());
        assert!(add(&all, date(2026, 1, 5), 1, Unit::Days, Roll::None).is_ok());
    }

    fn year(country: &str, region: Option<&str>, y: i16) -> Listing {
        holidays(&place(country, region), date(y, 1, 1), date(y, 12, 31)).unwrap()
    }

    #[test]
    fn lists_regions_that_drop_a_national_holiday() {
        let us = place("US", None);
        let b = us.block.as_ref().unwrap();
        let h = b.holidays.iter().find(|h| !h.except.is_empty()).unwrap();
        let listing = holidays(&us, h.date, h.date).unwrap();
        let listed = listing
            .holidays
            .iter()
            .find(|l| l.entry.name == h.name)
            .unwrap();
        let mut codes: Vec<_> = h.except.iter().map(|&r| b.regions[r].code).collect();
        codes.sort_unstable();
        assert_eq!(listed.except_regions, codes);
    }

    #[test]
    fn listing_needs_a_country_inside_its_data() {
        let none = Place::new(None, None, None).unwrap();
        let e = holidays(&none, date(2026, 1, 1), date(2026, 12, 31)).unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str(), e.hint.as_deref()),
            (
                "country_required",
                "Listing holidays needs a country.",
                Some("Pick a country first, e.g. DE.")
            )
        );
        let lk = place("LK", None);
        let e = holidays(&lk, date(2027, 1, 1), date(2027, 12, 31)).unwrap_err();
        assert_eq!(
            e.message,
            "Holiday data for Sri Lanka covers 2003-2026; 2027-01-01 is outside it."
        );
    }

    fn codes_of(warnings: &[Warning]) -> Vec<&'static str> {
        warnings.iter().map(|w| w.code).collect()
    }

    #[test]
    fn warns_without_a_country() {
        let none = Place::new(None, None, None).unwrap();
        let r = between(&none, date(2026, 11, 2), date(2026, 11, 6), true, true).unwrap();
        assert_eq!(
            r.warnings,
            [Warning {
                code: "no_country",
                message: "No country given, so only weekends (Sat, Sun) are skipped.".into()
            }]
        );
        let fri = Place::new(None, None, Some(0b001_0000)).unwrap();
        let r = add(&fri, date(2026, 11, 2), 3, Unit::BusinessDays, Roll::None).unwrap();
        assert_eq!(
            r.warnings[0].message,
            "No country given, so only weekends (Fri) are skipped."
        );
        let r = add(&none, date(2026, 11, 2), 3, Unit::Days, Roll::Following).unwrap();
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn warns_when_regions_differ() {
        let de = place("DE", None);
        let r = between(&de, date(2026, 1, 1), date(2026, 1, 31), true, true).unwrap();
        assert_eq!(
            r.warnings,
            [Warning {
                code: "regions_differ",
                message: "Germany has regional holidays in this range (Epiphany 2026-01-06 (Augsburg, BW, BY, ST)); this uses national holidays only. Add --region, e.g. BB.".into()
            }]
        );
        let r = between(&de, date(2026, 1, 1), date(2026, 12, 31), true, true).unwrap();
        assert!(r.warnings[0].message.contains(
            "(Epiphany 2026-01-06 (Augsburg, BW, BY, ST), Women's Day 2026-03-08 (BE, MV), Easter Sunday 2026-04-05 (BB));"
        ));
        let by = place("DE", Some("BY"));
        let r = between(&by, date(2026, 1, 1), date(2026, 1, 31), true, true).unwrap();
        assert!(r.warnings.is_empty());
        let r = between(&de, date(2026, 2, 2), date(2026, 2, 27), true, true).unwrap();
        assert!(r.warnings.is_empty());
        assert!(year("DE", None, 2026).warnings.is_empty());
    }

    #[test]
    fn warns_about_unannounced_schedules() {
        let cn = place("CN", None);
        let r = between(&cn, date(2027, 2, 1), date(2027, 2, 28), true, true).unwrap();
        assert_eq!(
            r.warnings,
            [Warning {
                code: "schedule_not_announced",
                message: "China announces one-off holidays or workday swaps most years; the bundled data has them through 2026. Later dates follow the regular rules and may change.".into()
            }]
        );
        let r = add(&cn, date(2026, 12, 30), 3, Unit::BusinessDays, Roll::None).unwrap();
        assert_eq!(codes_of(&r.warnings), ["schedule_not_announced"]);
        assert_eq!(
            codes_of(&year("CN", None, 2027).warnings),
            ["schedule_not_announced"]
        );
        assert!(year("CN", None, 2026).warnings.is_empty());
    }

    #[test]
    fn warns_about_estimated_dates() {
        let ae = year("AE", None, 2026);
        let w = ae.warnings.iter().find(|w| w.code == "estimated_dates");
        assert_eq!(
            w.unwrap().message,
            "Some holiday dates are estimates (lunar calendar): Eid al-Fitr 2026-03-20, Eid al-Fitr Holiday 2026-03-21, Eid al-Fitr Holiday 2026-03-22. The official date can differ by a day."
        );
        assert!(year("SA", None, 2026).warnings.is_empty());
    }

    #[test]
    fn warns_when_a_weekend_override_meets_decree_workdays() {
        let cn = Place::new(Some("CN"), None, Some(0b110_0000)).unwrap();
        let r = between(&cn, date(2026, 2, 1), date(2026, 2, 28), true, true).unwrap();
        assert_eq!(r.business_days, 16);
        assert_eq!(
            r.warnings,
            [Warning {
                code: "weekend_override",
                message: "Weekend days worked by decree still count as business days: 2026-02-14, 2026-02-28.".into()
            }]
        );
        let r = between(&cn, date(2026, 3, 1), date(2026, 3, 31), true, true).unwrap();
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn warns_when_adding_nothing_to_a_day_off() {
        let r = added(
            Some("CN"),
            date(2026, 2, 15),
            0,
            Unit::BusinessDays,
            Roll::None,
        );
        assert_eq!(
            r.warnings,
            [Warning {
                code: "start_not_business_day",
                message: "2026-02-15 is not a business day in China.".into()
            }]
        );
        let r = added(
            Some("CN"),
            date(2026, 2, 14),
            0,
            Unit::BusinessDays,
            Roll::None,
        );
        assert!(r.warnings.is_empty());
        let r = added(None, date(2026, 2, 15), 0, Unit::BusinessDays, Roll::None);
        assert_eq!(r.warnings[1].message, "2026-02-15 is not a business day.");
    }
}
