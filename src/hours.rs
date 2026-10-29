//! Business hours: working windows on business days, counted between two date-times.

use std::fmt;

use jiff::civil::{Date, DateTime, Time};
use jiff::fmt::temporal::DateTimePrinter;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Unit, Zoned};

use crate::days::{Day, Entry, Place, Warning, day_warnings};
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
    /// The date on the clock.
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
}

/// Clock time as `2026-11-02T10:00:00`; an instant as RFC 3339 to the second. An offset with
/// seconds (Monrovia's -00:44:30 until 1972) becomes the closest whole-minute offset and the
/// clock moves with it, so the text still names the same instant (RFC 3339 §5.8).
impl fmt::Display for Moment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Moment::Civil(dt) => write!(f, "{dt}"),
            Moment::Zoned(z) => {
                let offset = z.offset().round(Unit::Minute).expect("tzdata offsets fit");
                let printer = DateTimePrinter::new().precision(Some(0));
                f.write_str(&printer.timestamp_with_offset_to_string(&z.timestamp(), offset))
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
/// intersected with the interval.
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
    place.covers(from)?;
    place.covers(to)?;
    let (a, b, zone) = (lo.second(), hi.second(), start.zone());
    let (mut seconds, mut business_days) = (0, 0);
    // A window that runs past midnight can reach in from the day before.
    let mut d = from
        .yesterday()
        .ok()
        .filter(|d| place.covers(*d).is_ok())
        .unwrap_or(from);
    loop {
        if place.day(d) == Day::Business {
            let worked: i64 = windows
                .iter()
                .map(|w| (edge(d, w.end, zone).min(b) - edge(d, w.start, zone).max(a)).max(0))
                .sum();
            seconds += worked;
            business_days += i64::from(worked > 0);
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
}
