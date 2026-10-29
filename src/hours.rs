//! Business hours: working windows on business days, counted between two date-times or
//! consumed from one.

use std::fmt;

use jiff::civil::{Date, DateTime, Time};
use jiff::fmt::temporal::DateTimePrinter;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, Unit, Zoned};

use crate::days::{Day, Entry, Place, SKIPPED_MAX, Warning, day_warnings};
use crate::error::Error;

/// Seconds in a day without clock changes.
pub const DAY: i32 = 86_400;

const EPOCH: DateTime = DateTime::constant(1970, 1, 1, 0, 0, 0, 0);

/// Working time on every business day, in seconds from its midnight. A window that runs past
/// midnight ends after `DAY` and belongs to the day it starts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: i32,
    pub end: i32,
}

impl fmt::Display for Window {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let hm = |s: i32| format!("{:02}:{:02}", s / 3600, s / 60 % 60);
        let end = if self.end == DAY {
            "24:00".into()
        } else {
            hm(self.end % DAY)
        };
        write!(f, "{}-{end}", hm(self.start))
    }
}

/// A point in time: clock time without a zone, or an instant in the business's time zone.
#[derive(Debug, Clone, PartialEq)]
pub enum Moment {
    Civil(DateTime),
    Zoned(Zoned),
}

impl Moment {
    /// The clock as printed: in a zone, the instant at its offset rounded to whole minutes.
    pub fn shown(&self) -> DateTime {
        match self {
            Moment::Civil(dt) => *dt,
            Moment::Zoned(z) => minute_offset(z).to_datetime(z.timestamp()),
        }
    }

    /// The date on the zone's clock.
    pub fn date(&self) -> Date {
        match self {
            Moment::Civil(dt) => dt.date(),
            Moment::Zoned(z) => z.date(),
        }
    }

    fn zone(&self) -> Option<&TimeZone> {
        match self {
            Moment::Civil(_) => None,
            Moment::Zoned(z) => Some(z.time_zone()),
        }
    }

    /// Seconds on one line shared by every moment of the same kind.
    fn second(&self) -> i64 {
        match self {
            Moment::Civil(dt) => dt.duration_since(EPOCH).as_secs(),
            Moment::Zoned(z) => z.timestamp().as_second(),
        }
    }

    /// The moment `second` names on the line of `like`, if it is in 0001-9999.
    fn at(second: i64, like: &Moment) -> Option<Moment> {
        let moment = match like {
            Moment::Civil(_) => {
                Moment::Civil(EPOCH.checked_add(SignedDuration::from_secs(second)).ok()?)
            }
            Moment::Zoned(z) => Moment::Zoned(
                Timestamp::from_second(second)
                    .ok()?
                    .to_zoned(z.time_zone().clone()),
            ),
        };
        (moment.shown().year() >= 1).then_some(moment)
    }
}

fn minute_offset(z: &Zoned) -> jiff::tz::Offset {
    z.offset().round(Unit::Minute).expect("tzdata offsets fit")
}

/// Clock time as `2026-11-02T10:00:00`; an instant as RFC 3339 to the second. An offset with
/// seconds (Monrovia's -00:44:30 until 1972) becomes the closest whole-minute offset and the
/// clock moves with it, so the text still names the same instant (RFC 3339 §5.8).
impl fmt::Display for Moment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Moment::Civil(dt) => write!(f, "{dt}"),
            Moment::Zoned(z) => {
                let printer = DateTimePrinter::new().precision(Some(0));
                f.write_str(
                    &printer.timestamp_with_offset_to_string(&z.timestamp(), minute_offset(z)),
                )
            }
        }
    }
}

/// Where a window edge `at` seconds after the midnight starting `d` falls, on the same line as
/// `Moment::second`. In a zone a clock time in a gap moves later and one in a fold takes the
/// earlier instant (jiff's "compatible"); past the last instant jiff holds it is `i64::MAX`.
fn edge(d: Date, at: i32, zone: Option<&TimeZone>) -> i64 {
    let midnight = d.to_datetime(Time::midnight());
    let Some(tz) = zone else {
        return midnight.duration_since(EPOCH).as_secs() + i64::from(at);
    };
    midnight
        .checked_add(SignedDuration::from_secs(at.into()))
        .ok()
        .and_then(|clock| tz.to_ambiguous_timestamp(clock).compatible().ok())
        .map_or(i64::MAX, |t| t.as_second())
}

#[derive(Debug, PartialEq)]
pub struct Hours {
    pub start: Moment,
    pub end: Moment,
    /// Negative when `end` is before `start`.
    pub seconds: i64,
    /// Business days with any working time inside the interval.
    pub business_days: i64,
    pub holidays: Vec<Entry>,
    pub weekend_workdays: Vec<Date>,
    /// The weekend in force at `start`.
    pub weekend: u8,
    pub warnings: Vec<Warning>,
}

/// Working time from `start` to `end`: each window `[from, to)` on each business day,
/// intersected with the interval. Time is counted once even where a clock change makes windows
/// overlap, as `add` consumes it.
pub fn between(
    place: &Place,
    start: &Moment,
    end: &Moment,
    windows: &[Window],
) -> Result<Hours, Error> {
    let backwards = end.second() < start.second();
    let (lo, hi) = if backwards {
        (end, start)
    } else {
        (start, end)
    };
    let (from, to) = (lo.date(), hi.date());
    let (a, b, zone) = (lo.second(), hi.second(), start.zone());
    // The end's own day needs data only if one of its windows opens before the end; otherwise
    // only an overnight window of the day before reaches it, as in `add`.
    let opens = windows.iter().any(|w| edge(to, w.start, zone) < b);
    let last = if opens {
        to
    } else {
        to.yesterday().unwrap_or(to)
    };
    place.covers(from)?;
    place.covers(last.max(from))?;
    let (mut seconds, mut business_days, mut t) = (0, 0, a);
    let mut d = first_day(place, from);
    loop {
        if place.day(d) == Day::Business {
            let before = seconds;
            for w in windows {
                let (ws, we) = (edge(d, w.start, zone).max(t), edge(d, w.end, zone).min(b));
                if we > ws {
                    seconds += we - ws;
                    t = we;
                }
            }
            business_days += i64::from(seconds > before);
        }
        if d >= to {
            break;
        }
        d = d.tomorrow().unwrap();
    }
    let holidays = place.holidays(from, to);
    let warnings = day_warnings(place, from, to, &holidays, true);
    Ok(Hours {
        start: start.clone(),
        end: end.clone(),
        seconds: if backwards { -seconds } else { seconds },
        business_days,
        holidays,
        weekend_workdays: place.workdays(from, to),
        weekend: place.weekend_on(start.date()),
        warnings,
    })
}

/// The first day whose windows can reach `date`: the day before, when the data covers it, as a
/// window that runs past midnight belongs to the day it starts on.
fn first_day(place: &Place, date: Date) -> Date {
    date.yesterday()
        .ok()
        .filter(|d| place.covers(*d).is_ok())
        .unwrap_or(date)
}

#[derive(Debug, PartialEq)]
pub struct Added {
    pub start: Moment,
    /// Seconds of working time; negative goes back.
    pub amount: i64,
    pub result: Moment,
    /// The non-business days passed over, at most 100.
    pub skipped: Vec<(Date, Day)>,
    pub skipped_more: usize,
    pub weekend_workdays: Vec<Date>,
    /// The weekend in force at `start`.
    pub weekend: u8,
    pub warnings: Vec<Warning>,
}

/// Consumes `amount` seconds of working time from `start`, forward or (negative) backward.
/// Outside working time it begins at the next window's start (backward: the previous window's
/// end), and a result exactly at a window's end stays there.
pub fn add(place: &Place, start: &Moment, amount: i64, windows: &[Window]) -> Result<Added, Error> {
    place.covers(start.date())?;
    let (forward, zone) = (amount > 0, start.zone());
    let (mut t, mut left) = (start.second(), amount.abs());
    if left > 0 {
        place.has_business_days()?;
    }
    let mut d = if forward {
        first_day(place, start.date())
    } else {
        start.date()
    };
    while left > 0 {
        if place.day(d) == Day::Business {
            for i in 0..windows.len() {
                let w = windows[if forward { i } else { windows.len() - 1 - i }];
                let (ws, we) = (edge(d, w.start, zone), edge(d, w.end, zone));
                let (from, to) = if forward {
                    (ws.max(t), we)
                } else {
                    (ws, we.min(t))
                };
                let free = (to - from).max(0);
                if free >= left {
                    t = if forward { from + left } else { to - left };
                    left = 0;
                    break;
                } else if free > 0 {
                    left -= free;
                    t = if forward { to } else { from };
                }
            }
        }
        if left > 0 {
            d = place.step(d, forward)?;
        }
    }
    let result = Moment::at(t, start).ok_or_else(|| place.out_of_range())?;
    let (lo, hi) = (
        start.date().min(result.date()),
        start.date().max(result.date()),
    );
    let mut passed = start
        .date()
        .series(jiff::Span::new().days(if forward { 1 } else { -1 }))
        .take_while(|&d| d != result.date())
        .skip(1)
        .filter_map(|d| Some((d, place.day(d))).filter(|p| p.1 != Day::Business));
    let skipped: Vec<_> = passed.by_ref().take(SKIPPED_MAX).collect();
    let skipped_more = passed.count();
    let mut warnings = day_warnings(place, lo, hi, &place.holidays(lo, hi), true);
    if amount == 0 {
        warnings.extend(place.start_warning(start.date()));
    }
    Ok(Added {
        start: start.clone(),
        amount,
        result,
        skipped,
        skipped_more,
        weekend_workdays: place.workdays(lo, hi),
        weekend: place.weekend_on(start.date()),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{datetime, hours, timezone};

    fn worked(country: Option<&str>, start: &str, end: &str, windows: &str) -> Hours {
        let place = Place::new(country, None, None).unwrap();
        let (s, e) = (datetime(start, None).unwrap(), datetime(end, None).unwrap());
        between(&place, &s, &e, &hours(windows).unwrap()).unwrap()
    }

    #[test]
    fn counts_windows_on_business_days() {
        let r = worked(
            Some("DE"),
            "2026-11-02T10:00",
            "2026-11-05T15:00",
            "09:00-17:00",
        );
        assert_eq!((r.seconds, r.business_days), (104_400, 4));
        let r = worked(
            Some("DE"),
            "2026-11-02 10:00",
            "2026-11-05 15:00",
            "09:00-12:00,13:00-17:00",
        );
        assert_eq!(r.seconds, 25 * 3600);
        let r = worked(
            Some("DE"),
            "2026-11-05T15:00",
            "2026-11-02T10:00",
            "09:00-17:00",
        );
        assert_eq!(r.seconds, -104_400);
        let r = worked(None, "2026-11-02T10:00", "2026-11-02T10:00", "09:00-17:00");
        assert_eq!((r.seconds, r.business_days), (0, 0));
        // German Unity Day (Sat) and a plain weekend: nothing to count.
        let r = worked(Some("DE"), "2026-10-03", "2026-10-05", "09:00-17:00");
        assert_eq!((r.seconds, r.holidays.len()), (0, 1));
    }

    #[test]
    fn overnight_windows_belong_to_the_day_they_start() {
        // Fri 22:00 to Sat 06:00 counts in full; Sat's own window doesn't (weekend).
        let r = worked(None, "2026-11-06 12:00", "2026-11-08", "22:00-06:00");
        assert_eq!((r.seconds, r.business_days), (8 * 3600, 1));
        // Thursday's window reaches into Friday's interval.
        let r = worked(None, "2026-11-06 00:00", "2026-11-06 03:00", "22:00-06:00");
        assert_eq!(r.seconds, 3 * 3600);
        // ... even when the next day is a holiday (DE: Thu 2026-12-24, Fri Christmas).
        let r = worked(Some("DE"), "2026-12-24 12:00", "2026-12-26", "22:00-06:00");
        assert_eq!(r.seconds, 8 * 3600);
        let r = worked(None, "2026-11-02", "2026-11-03", "00:00-24:00");
        assert_eq!(r.seconds, 24 * 3600);
    }

    #[test]
    fn warns_and_lists_like_business_days() {
        let r = worked(None, "2026-11-02T10:00", "2026-11-03T10:00", "09:00-17:00");
        assert_eq!(r.warnings[0].code, "no_country");
        let r = worked(Some("CN"), "2026-02-13", "2026-02-15", "09:00-17:00");
        assert_eq!(r.weekend_workdays, [jiff::civil::date(2026, 2, 14)]);
        assert_eq!(r.seconds, 16 * 3600);
        let place = Place::new(Some("LK"), None, None).unwrap();
        let (s, e) = (
            datetime("2026-12-31", None).unwrap(),
            datetime("2027-01-04", None).unwrap(),
        );
        let e = between(&place, &s, &e, &hours("09:00-17:00").unwrap()).unwrap_err();
        assert_eq!(e.code, "no_holiday_data");
    }

    fn zoned(place: Place, start: &str, end: &str, windows: &str, tz: &str) -> Hours {
        let tz = timezone(tz).unwrap();
        let (s, e) = (datetime(start, Some(&tz)), datetime(end, Some(&tz)));
        between(&place, &s.unwrap(), &e.unwrap(), &hours(windows).unwrap()).unwrap()
    }

    #[test]
    fn windows_in_a_zone_count_real_time() {
        let anyday = || Place::new(None, None, Some(0)).unwrap();
        let ny = "America/New_York";
        let r = zoned(
            anyday(),
            "2026-03-08 00:00",
            "2026-03-09 00:00",
            "00:00-24:00",
            ny,
        );
        assert_eq!(r.seconds, 23 * 3600);
        let (s, e) = (datetime("2026-03-08", None), datetime("2026-03-09", None));
        let r = between(
            &anyday(),
            &s.unwrap(),
            &e.unwrap(),
            &hours("00:00-24:00").unwrap(),
        );
        assert_eq!(r.unwrap().seconds, 24 * 3600);
        // 02:30 falls in the spring-forward gap, so the window starts an hour later, at 03:30.
        let r = zoned(anyday(), "2026-03-08", "2026-03-09", "02:30-04:00", ny);
        assert_eq!(r.seconds, 1800);
        // ... so 01:00-02:30 runs to 03:30 and overlaps 03:00-04:00: 06:00Z-08:00Z, counted once.
        let r = zoned(
            anyday(),
            "2026-03-08",
            "2026-03-09",
            "01:00-02:30,03:00-04:00",
            ny,
        );
        assert_eq!(r.seconds, 7200);
        // Instants are read in the zone: Fri 09:00-17:00 and Mon 09:00-11:00 at -07 (tzdata
        // 2026b ended Vancouver's clock changes).
        let bc = Place::new(Some("CA"), Some("BC"), None).unwrap();
        let (start, end) = ("2026-10-30T16:00:00Z", "2026-11-02T18:00:00Z");
        let r = zoned(bc, start, end, "09:00-17:00", "America/Vancouver");
        assert_eq!((r.seconds, r.business_days), (10 * 3600, 2));
        assert_eq!(r.start.to_string(), "2026-10-30T09:00:00-07:00");
        assert_eq!(r.end.to_string(), "2026-11-02T11:00:00-07:00");
    }

    #[test]
    fn a_fold_at_midnight_makes_a_longer_day() {
        // Santiago turns Sat 2026-04-04 24:00 back to 23:00, so that day has 25 hours.
        let r = zoned(
            Place::new(None, None, Some(0)).unwrap(),
            "2026-04-04",
            "2026-04-05",
            "00:00-24:00",
            "America/Santiago",
        );
        assert_eq!(r.seconds, 25 * 3600);
        let tz = timezone("America/Santiago").unwrap();
        let first = datetime("2026-04-04 23:30", Some(&tz)).unwrap();
        assert_eq!(first.to_string(), "2026-04-04T23:30:00-03:00");
    }

    #[test]
    fn prints_whole_minute_offsets_and_keeps_the_instant() {
        let tz = timezone("Africa/Monrovia").unwrap();
        let m = datetime("1971-06-01T09:00", Some(&tz)).unwrap();
        // 09:00 at -00:44:30 is 09:44:30Z; at -00:45 that instant reads 08:59:30.
        assert_eq!(m.to_string(), "1971-06-01T08:59:30-00:45");
        let utc = timezone("utc").unwrap();
        let m = datetime("2026-11-02T09:30:00+05:30", Some(&utc)).unwrap();
        assert_eq!(m.to_string(), "2026-11-02T04:00:00+00:00");
        let m = datetime("2026-11-02 09:30", None).unwrap();
        assert_eq!(m.to_string(), "2026-11-02T09:30:00");
    }

    fn plus(place: &Place, start: &str, hours_: f64, windows: &str, tz: Option<&str>) -> Added {
        let tz = tz.map(|t| timezone(t).unwrap());
        let start = datetime(start, tz.as_ref()).unwrap();
        let seconds = (hours_ * 3600.0) as i64;
        add(place, &start, seconds, &hours(windows).unwrap()).unwrap()
    }

    #[test]
    fn adds_business_hours_through_windows() {
        let de = Place::new(Some("DE"), None, None).unwrap();
        let at = |start, n| plus(&de, start, n, "09:00-17:00", None).result.to_string();
        assert_eq!(at("2026-11-02 16:00", 3.0), "2026-11-03T11:00:00");
        assert_eq!(at("2026-11-03 10:00", -2.0), "2026-11-02T16:00:00");
        // A result exactly at a window's end stays there.
        assert_eq!(at("2026-11-02 15:00", 2.0), "2026-11-02T17:00:00");
        assert_eq!(at("2026-11-02 11:00", -2.0), "2026-11-02T09:00:00");
        // Outside working time it begins at the next window (backward: the previous one).
        assert_eq!(at("2026-11-02 06:00", 1.5), "2026-11-02T10:30:00");
        assert_eq!(at("2026-11-02 20:00", -1.0), "2026-11-02T16:00:00");
        assert_eq!(at("2026-11-02 20:00", 0.0), "2026-11-02T20:00:00");
        // Fri 16:00 + 2 h passes the weekend; German Unity Day (Sat 2026-10-03) too.
        let r = plus(&de, "2026-10-02 16:00", 2.0, "09:00-17:00", None);
        assert_eq!(r.result.to_string(), "2026-10-05T10:00:00");
        let days: Vec<_> = r.skipped.iter().map(|s| (s.0.to_string(), s.1)).collect();
        assert_eq!(
            days,
            [
                ("2026-10-03".into(), Day::Weekend),
                ("2026-10-04".into(), Day::Weekend)
            ]
        );
        let r = plus(&de, "2026-10-05 10:00", -2.0, "09:00-17:00", None);
        assert_eq!(r.result.to_string(), "2026-10-02T16:00:00");
        assert_eq!(r.skipped[0].0, jiff::civil::date(2026, 10, 4));
        let lunch = plus(
            &de,
            "2026-11-02 11:00",
            3.0,
            "09:00-12:00,13:00-17:00",
            None,
        );
        assert_eq!(lunch.result.to_string(), "2026-11-02T15:00:00");
        let lunch = plus(
            &de,
            "2026-11-02 15:00",
            -3.0,
            "09:00-12:00,13:00-17:00",
            None,
        );
        assert_eq!(lunch.result.to_string(), "2026-11-02T11:00:00");
    }

    #[test]
    fn adds_through_overnight_windows_and_zones() {
        let none = Place::new(None, None, None).unwrap();
        // Thursday's 22:00-06:00 window is still open at Fri 03:00.
        let r = plus(&none, "2026-11-06 03:00", 2.0, "22:00-06:00", None);
        assert_eq!(r.result.to_string(), "2026-11-06T05:00:00");
        let r = plus(&none, "2026-11-06 03:00", 4.0, "22:00-06:00", None);
        assert_eq!(r.result.to_string(), "2026-11-06T23:00:00");
        // New York springs forward on Sun 2026-03-08: 8 working hours are 9 on the clock.
        let anyday = Place::new(None, None, Some(0)).unwrap();
        let ny = Some("America/New_York");
        let r = plus(&anyday, "2026-03-08 00:00", 8.0, "00:00-24:00", ny);
        assert_eq!(r.result.to_string(), "2026-03-08T09:00:00-04:00");
        let r = plus(&anyday, "2026-03-08T13:00:00Z", -8.0, "00:00-24:00", ny);
        assert_eq!(r.result.to_string(), "2026-03-08T00:00:00-05:00");
    }

    #[test]
    fn adding_hours_warns_and_fails_like_business_days() {
        let cn = Place::new(Some("CN"), None, None).unwrap();
        let r = plus(&cn, "2026-02-15 10:00", 0.0, "09:00-17:00", None);
        assert_eq!((&r.result, r.skipped.len()), (&r.start, 0));
        assert_eq!(
            r.warnings,
            [Warning {
                code: "start_not_business_day",
                message: "2026-02-15 is not a business day in China.".into()
            }]
        );
        // Sat 2026-02-14 is worked by decree.
        let r = plus(&cn, "2026-02-13 16:00", 2.0, "09:00-17:00", None);
        assert_eq!(r.result.to_string(), "2026-02-14T10:00:00");
        assert_eq!(r.weekend_workdays, [jiff::civil::date(2026, 2, 14)]);
        let none = Place::new(None, None, None).unwrap();
        let r = plus(&none, "2026-11-02 09:00", 1.0, "09:00-17:00", None);
        assert_eq!(r.warnings[0].code, "no_country");
        let windows = hours("09:00-17:00").unwrap();
        let fail = |place: &Place, start: &str, seconds| {
            let start = datetime(start, None).unwrap();
            add(place, &start, seconds, &windows).unwrap_err()
        };
        let us = Place::new(Some("US"), None, None).unwrap();
        let e = fail(&us, "2050-12-30 09:00", 3600 * 100);
        assert_eq!(e.message, "The result would be outside 2000-2050.");
        assert_eq!(
            fail(&none, "9999-12-31 09:00", 3600 * 9).code,
            "result_out_of_range"
        );
        assert_eq!(
            fail(&none, "0001-01-01 10:00", -3600 * 2).code,
            "result_out_of_range"
        );
        assert_eq!(fail(&us, "1999-12-31 09:00", 3600).code, "no_holiday_data");
        let all = Place::new(None, None, Some(0b111_1111)).unwrap();
        assert_eq!(
            fail(&all, "2026-11-02 09:00", 3600).code,
            "no_business_days"
        );
        let r = plus(&none, "2026-11-02 09:00", 1000.0, "09:00-17:00", None);
        assert_eq!((r.skipped.len(), r.skipped_more), (48, 0));
        let r = plus(&none, "2026-01-05 09:00", 8000.0, "09:00-17:00", None);
        assert_eq!((r.skipped.len(), r.skipped_more), (100, 298));
    }
}
