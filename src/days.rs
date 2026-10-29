//! Business days: which dates count, `between` and `add`.

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
}

const SKIPPED_MAX: usize = 100;

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
        let weekday = date.weekday().to_monday_zero_offset();
        let Some(b) = &self.block else {
            return match self.weekend_on(date) >> weekday & 1 {
                1 => Day::Weekend,
                _ => Day::Business,
            };
        };
        if self.weekend_on(date) >> weekday & 1 == 1 && !b.worked_by_decree(self.region, date) {
            Day::Weekend
        } else if let Some(h) = b.holidays(self.region, date, date).next() {
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
        let mut days: Vec<_> = b.decree_workdays(self.region, from, to).collect();
        days.dedup();
        days
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
    let mut weekend_workdays = match business || roll != Roll::None {
        true => place.workdays(from.min(result), from.max(result)),
        false => vec![],
    };
    weekend_workdays.retain(|&d| !business || d != start);
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
    })
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

    fn count(country: &str, region: Option<&str>, from: Date, to: Date) -> i64 {
        between(&place(country, region), from, to, true, true)
            .unwrap()
            .business_days
    }

    #[test]
    fn china_february_2026_counts_decree_workdays() {
        let cn = place("CN", None);
        let r = between(&cn, date(2026, 2, 1), date(2026, 2, 28), true, true).unwrap();
        assert_eq!(
            (
                r.business_days,
                r.calendar_days,
                r.weekend_days,
                r.holiday_days
            ),
            (16, 28, 6, 6)
        );
        assert_eq!((r.elapsed_days, r.elapsed.as_str()), (27, "P27D"));
        assert_eq!(r.weekend_workdays, [date(2026, 2, 14), date(2026, 2, 28)]);
        assert_eq!(r.weekend, SAT_SUN);
        let names: Vec<_> = r.holidays.iter().map(|h| (h.date, h.name)).collect();
        assert_eq!(
            names,
            [
                (date(2026, 2, 16), "Chinese New Year's Eve"),
                (date(2026, 2, 17), "Chinese New Year (Spring Festival)"),
                (date(2026, 2, 18), "Chinese New Year (Spring Festival)"),
                (date(2026, 2, 19), "Chinese New Year (Spring Festival)"),
                (date(2026, 2, 20), "Day off (substituted from 02/14/2026)"),
                (date(2026, 2, 23), "Day off (substituted from 02/28/2026)"),
            ]
        );
        assert_eq!(cn.day(date(2026, 2, 14)), Day::Business);
        assert_eq!(cn.day(date(2026, 2, 15)), Day::Weekend);
        assert_eq!(
            cn.day(date(2026, 2, 16)),
            Day::Holiday("Chinese New Year's Eve")
        );
    }

    #[test]
    fn counts_match_the_official_calendars() {
        let jan = (date(2026, 1, 1), date(2026, 1, 31));
        let year = (date(2026, 1, 1), date(2026, 12, 31));
        assert_eq!(count("CN", None, date(2026, 9, 20), date(2026, 10, 10)), 11);
        assert_eq!(count("DE", None, jan.0, jan.1), 21);
        assert_eq!(count("DE", Some("BY"), jan.0, jan.1), 20);
        assert_eq!(count("DE-BY", None, jan.0, jan.1), 20);
        assert_eq!(count("DE", None, year.0, year.1), 254);
        assert_eq!(count("DE", Some("BY"), year.0, year.1), 252);
        assert_eq!(count("US", None, date(2026, 7, 1), date(2026, 7, 31)), 22);
        let xmas = (date(2026, 12, 24), date(2027, 1, 5));
        assert_eq!(count("GB", Some("ENG"), xmas.0, xmas.1), 6);
        assert_eq!(count("GB", Some("SCT"), xmas.0, xmas.1), 5);
        assert_eq!(count("JP", None, date(2026, 5, 1), date(2026, 5, 31)), 18);
        assert_eq!(count("CN", None, date(2027, 2, 1), date(2027, 2, 28)), 16);
    }

    #[test]
    fn each_date_uses_its_own_weekend() {
        let one = |d| count("AE", None, d, d);
        assert_eq!(one(date(2021, 1, 10)), 1);
        assert_eq!(one(date(2023, 1, 8)), 0);
        assert_eq!(count("AE", None, date(2021, 12, 26), date(2022, 1, 8)), 10);
    }

    #[test]
    fn backwards_and_excluded_ends() {
        let cn = place("CN", None);
        let (feb1, feb28) = (date(2026, 2, 1), date(2026, 2, 28));
        let r = between(&cn, feb28, feb1, true, true).unwrap();
        assert_eq!((r.business_days, r.calendar_days), (-16, 28));
        assert_eq!((r.elapsed_days, r.elapsed.as_str()), (-27, "-P27D"));
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

    fn workday(country: &str, start: Date, n: i64) -> Date {
        added(Some(country), start, n, Unit::BusinessDays, Roll::None).result
    }

    #[test]
    fn adds_business_days_like_workday() {
        let r = added(
            Some("CN"),
            date(2026, 2, 13),
            5,
            Unit::BusinessDays,
            Roll::None,
        );
        assert_eq!(r.result, date(2026, 2, 27));
        let skipped: Vec<_> = r.skipped.iter().map(|s| (s.0.day(), s.1)).collect();
        let cny = Day::Holiday("Chinese New Year (Spring Festival)");
        assert_eq!(
            skipped,
            [
                (15, Day::Weekend),
                (16, Day::Holiday("Chinese New Year's Eve")),
                (17, cny),
                (18, cny),
                (19, cny),
                (20, Day::Holiday("Day off (substituted from 02/14/2026)")),
                (21, Day::Weekend),
                (22, Day::Weekend),
                (23, Day::Holiday("Day off (substituted from 02/28/2026)")),
            ]
        );
        assert_eq!(r.weekend_workdays, [date(2026, 2, 14)]);
        assert_eq!(workday("CN", date(2026, 3, 2), -5), date(2026, 2, 24));
        assert_eq!(workday("US", date(2026, 7, 2), 1), date(2026, 7, 6));
        assert_eq!(workday("US", date(2026, 12, 24), 1), date(2026, 12, 28));
        assert_eq!(workday("CN", date(2026, 2, 14), 0), date(2026, 2, 14));
        assert_eq!(workday("CN", date(2026, 2, 15), 0), date(2026, 2, 15));
        let r = added(None, date(2026, 11, 2), 10, Unit::BusinessDays, Roll::None);
        assert_eq!((r.result, r.skipped.len()), (date(2026, 11, 16), 4));
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
}
