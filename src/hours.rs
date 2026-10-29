//! Business hours: working windows on business days, counted between two date-times.

use std::fmt;

use jiff::civil::{Date, DateTime, Time};

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

/// A point in time: clock time without a zone.
#[derive(Debug, Clone, PartialEq)]
pub enum Moment {
    Civil(DateTime),
}

impl Moment {
    pub fn date(&self) -> Date {
        match self {
            Moment::Civil(dt) => dt.date(),
        }
    }

    /// Seconds on one line shared by every moment of the same kind.
    fn second(&self) -> i64 {
        match self {
            Moment::Civil(dt) => dt.duration_since(EPOCH).as_secs(),
        }
    }
}

/// Where a window edge falls on day `d`, on the same line as `Moment::second`.
fn edge(d: Date, at: i32) -> i64 {
    d.to_datetime(Time::midnight())
        .duration_since(EPOCH)
        .as_secs()
        + i64::from(at)
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
    let (a, b) = (lo.second(), hi.second());
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
                .map(|w| (edge(d, w.end).min(b) - edge(d, w.start).max(a)).max(0))
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
    use crate::input::{datetime, hours};

    fn worked(country: Option<&str>, start: &str, end: &str, windows: &str) -> Hours {
        let place = Place::new(country, None, None).unwrap();
        let (s, e) = (datetime(start).unwrap(), datetime(end).unwrap());
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
            datetime("2026-12-31").unwrap(),
            datetime("2027-01-04").unwrap(),
        );
        let e = between(&place, &s, &e, &hours("09:00-17:00").unwrap()).unwrap_err();
        assert_eq!(e.code, "no_holiday_data");
    }
}
