//! The business day, add and holiday examples every face must answer the same way.

use date_calc::days::{self, Day, Place, Roll};
use date_calc::hours;
use date_calc::input::{self, Unit};
use jiff::civil::{Date, date};

/// `CC`, `CC/REGION`, `CC-REGION`, or `-` for no country.
fn place(country: &str) -> Place {
    let (country, region) = match country.split_once('/') {
        Some((c, r)) => (c, Some(r)),
        None => (country, None),
    };
    Place::new(Some(country).filter(|&c| c != "-"), region, None).unwrap()
}

fn codes(warnings: &[days::Warning]) -> Vec<&str> {
    warnings.iter().map(|w| w.code).collect()
}

/// Country, start, end, counted ends, business days, warnings. python-holidays 0.107's
/// get_working_days_count agrees on BR, MY/01 and IN/MH.
const BETWEEN: &str = "
CN      2026-02-01  2026-02-28  both   16
CN      2026-09-20  2026-10-10  both   11
DE      2026-01-01  2026-01-31  both   21    regions_differ
DE/BY   2026-01-01  2026-01-31  both   20
DE-BY   2026-01-01  2026-01-31  both   20
DE      2026-01-01  2026-12-31  both   254   regions_differ
DE/BY   2026-01-01  2026-12-31  both   252
US      2026-07-01  2026-07-31  both   22    regions_differ
GB/ENG  2026-12-24  2027-01-05  both   6     schedule_not_announced
GB/SCT  2026-12-24  2027-01-05  both   5     schedule_not_announced
JP      2026-05-01  2026-05-31  both   18
AE      2021-01-10  2021-01-10  both   1
AE      2023-01-08  2023-01-08  both   0
CN      2026-02-28  2026-02-01  both   -16
CN      2026-02-01  2026-02-28  none   15
CN      2026-02-28  2026-02-01  end    -15
CN      2027-02-01  2027-02-28  both   16    schedule_not_announced
-       2026-11-02  2026-11-06  both   5     no_country
BR      2026-01-01  2026-12-31  both   252   regions_differ
MY/01   2020-01-01  2020-12-31  both   247
IN/MH   2001-01-01  2035-12-31  both   8525  schedule_not_announced estimated_dates
";

#[test]
fn business_days_between() {
    for case in BETWEEN.lines().filter(|l| !l.is_empty()) {
        let f: Vec<_> = case.split_whitespace().collect();
        let (start, end) = (input::date(f[1]).unwrap(), input::date(f[2]).unwrap());
        let (inc_start, inc_end) = match f[3] {
            "both" => (true, true),
            "end" => (false, true),
            _ => (false, false),
        };
        let r = days::between(&place(f[0]), start, end, inc_start, inc_end).unwrap();
        assert_eq!(r.business_days.to_string(), f[4], "{case}");
        assert_eq!(codes(&r.warnings), f[5..], "{case}");
        assert_eq!(
            r.calendar_days,
            r.business_days.abs() + r.weekend_days + r.holiday_days,
            "{case}"
        );
    }
    let e = days::between(&place("LK"), date(2027, 1, 4), date(2027, 1, 5), true, true);
    let e = e.unwrap_err();
    assert_eq!((e.code, e.exit_code()), ("no_holiday_data", 1));
    assert_eq!(
        e.message,
        "Holiday data for Sri Lanka covers 2003-2026; 2027-01-04 is outside it."
    );
}

#[test]
fn between_matches_the_documented_example() {
    let r = days::between(
        &place("CN"),
        date(2026, 2, 1),
        date(2026, 2, 28),
        true,
        true,
    );
    let r = r.unwrap();
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
    let holidays: Vec<_> = r
        .holidays
        .iter()
        .map(|h| (h.date.to_string(), h.name, h.observed, h.estimated))
        .collect();
    let cny = "Chinese New Year (Spring Festival)";
    assert_eq!(
        holidays,
        [
            ("2026-02-16".into(), "Chinese New Year's Eve", false, false),
            ("2026-02-17".into(), cny, false, false),
            ("2026-02-18".into(), cny, false, false),
            ("2026-02-19".into(), cny, false, false),
            (
                "2026-02-20".into(),
                "Day off (substituted from 02/14/2026)",
                false,
                false
            ),
            (
                "2026-02-23".into(),
                "Day off (substituted from 02/28/2026)",
                false,
                false
            ),
        ]
    );
    assert_eq!(r.weekend_workdays, [date(2026, 2, 14), date(2026, 2, 28)]);
    assert_eq!(input::weekend_text(r.weekend), "Sat, Sun");
    assert!(r.warnings.is_empty());

    let r = days::between(
        &place("CN"),
        date(2026, 2, 28),
        date(2026, 2, 1),
        true,
        true,
    );
    let r = r.unwrap();
    assert_eq!(
        (r.business_days, r.elapsed_days, r.elapsed.as_str()),
        (-16, -27, "-P27D")
    );
}

fn add(country: &str, start: &str, amount: &str, unit: Unit, roll: Roll) -> days::Added {
    let start = input::date(start).unwrap();
    let amount = input::amount(amount, unit).unwrap();
    days::add(&place(country), start, amount, unit, roll).unwrap()
}

/// Country, start, amount, unit, roll, result, warnings.
const ADD: &str = "
CN  2026-02-13  5      bd      none                2026-02-27
CN  2026-03-02  -5     bd      none                2026-02-24
US  2026-07-02  1      bd      none                2026-07-06  regions_differ
US  2026-12-24  +1     bd      none                2026-12-28  regions_differ
CN  2026-02-14  0      bd      none                2026-02-14
CN  2026-02-15  0      bd      none                2026-02-15  start_not_business_day
-   2026-01-31  1      months  none                2026-02-28
DE  2026-01-31  1      months  modified-following  2026-02-27
DE  2026-10-03  0      days    following           2026-10-05
US  2000-01-03  12000  bd      none                2047-12-09  regions_differ
";

#[test]
fn add_to_a_date() {
    for case in ADD.lines().filter(|l| !l.is_empty()) {
        let f: Vec<_> = case.split_whitespace().collect();
        let unit = match f[3] {
            "bd" => Unit::BusinessDays,
            "days" => Unit::Days,
            _ => Unit::Months,
        };
        let roll = match f[4] {
            "following" => Roll::Following,
            "modified-following" => Roll::ModifiedFollowing,
            _ => Roll::None,
        };
        let r = add(f[0], f[1], f[2], unit, roll);
        assert_eq!(r.result.to_string(), f[5], "{case}");
        assert_eq!(codes(&r.warnings), f[6..], "{case}");
    }
}

#[test]
fn add_matches_the_documented_example() {
    let r = add("CN", "2026-02-13", "5", Unit::BusinessDays, Roll::None);
    let skipped: Vec<_> = r
        .skipped
        .iter()
        .map(|&(d, day)| {
            let (reason, name) = match day {
                Day::Holiday(name) => ("holiday", Some(name)),
                _ => ("weekend", None),
            };
            (d.to_string(), reason, name)
        })
        .collect();
    let cny = Some("Chinese New Year (Spring Festival)");
    assert_eq!(
        skipped,
        [
            ("2026-02-15".into(), "weekend", None),
            (
                "2026-02-16".into(),
                "holiday",
                Some("Chinese New Year's Eve")
            ),
            ("2026-02-17".into(), "holiday", cny),
            ("2026-02-18".into(), "holiday", cny),
            ("2026-02-19".into(), "holiday", cny),
            (
                "2026-02-20".into(),
                "holiday",
                Some("Day off (substituted from 02/14/2026)")
            ),
            ("2026-02-21".into(), "weekend", None),
            ("2026-02-22".into(), "weekend", None),
            (
                "2026-02-23".into(),
                "holiday",
                Some("Day off (substituted from 02/28/2026)")
            ),
        ]
    );
    assert_eq!(
        (r.unit.name(), r.roll, r.skipped_more),
        ("business_days", Roll::None, 0)
    );
    assert_eq!(r.weekend_workdays, [date(2026, 2, 14)]);
}

fn year(country: &str, y: i16) -> days::Listing {
    days::holidays(&place(country), date(y, 1, 1), date(y, 12, 31)).unwrap()
}

#[test]
fn holiday_listings() {
    let cn = year("CN", 2026);
    assert_eq!(cn.holidays.len(), 22);
    let worked: Vec<_> = cn.weekend_workdays.iter().map(Date::to_string).collect();
    assert_eq!(
        worked,
        [
            "2026-01-04",
            "2026-02-14",
            "2026-02-28",
            "2026-05-09",
            "2026-09-20",
            "2026-10-10"
        ]
    );

    let de = year("DE", 2026);
    assert_eq!(de.holidays.len(), 9);
    assert!(de.holidays.iter().all(|h| h.except_regions.is_empty()));
    let epiphany = &de.regional[0];
    assert_eq!(
        (
            epiphany.entry.date,
            epiphany.entry.name,
            &epiphany.regions[..]
        ),
        (
            date(2026, 1, 6),
            "Epiphany",
            &["Augsburg", "BW", "BY", "ST"][..]
        )
    );

    let by = year("DE/BY", 2026);
    let scopes: Vec<_> = by
        .holidays
        .iter()
        .map(|h| (h.entry.name, h.scope))
        .collect();
    assert_eq!(scopes.len(), 12);
    for name in ["Epiphany", "Corpus Christi", "All Saints' Day"] {
        assert!(scopes.contains(&(name, "regional")), "{name}");
    }

    let sa = year("SA", 2026);
    assert!(
        sa.holidays
            .iter()
            .any(|h| h.entry.name.starts_with("Eid al-Fitr") && !h.entry.estimated)
    );
    let ae = year("AE", 2026);
    assert!(
        ae.holidays
            .iter()
            .any(|h| h.entry.name.starts_with("Eid al-Fitr") && h.entry.estimated)
    );
    assert!(codes(&ae.warnings).contains(&"estimated_dates"));
}

/// The engine's share of the spec's time targets, with room left for process start.
#[test]
#[cfg_attr(debug_assertions, ignore = "timing needs --release")]
fn meets_the_time_targets() {
    let check = |name: &str, budget_ms: u128, run: &dyn Fn()| {
        let t = std::time::Instant::now();
        run();
        let ms = t.elapsed().as_millis();
        println!("{name}: {ms} ms");
        assert!(ms <= budget_ms, "{name}: {ms} ms > {budget_ms} ms");
    };
    let between = |country, from, to| {
        drop(days::between(&place(country), from, to, true, true).unwrap());
    };
    check("CN Feb 2026", 15, &|| {
        between("CN", date(2026, 2, 1), date(2026, 2, 28))
    });
    check("IN-MH 2001-2035", 60, &|| {
        between("IN/MH", date(2001, 1, 1), date(2035, 12, 31))
    });
    check("no country 0001-9999", 150, &|| {
        between("-", date(1, 1, 1), date(9999, 12, 31))
    });
    check("US +12000", 60, &|| {
        drop(add(
            "US",
            "2000-01-03",
            "12000",
            Unit::BusinessDays,
            Roll::None,
        ))
    });
    check("US hours 2026 in New York", 60, &|| {
        let ny = input::timezone("America/New_York").unwrap();
        let start = input::datetime("2026-01-01", Some(&ny)).unwrap();
        let end = input::datetime("2026-12-31 23:59", Some(&ny)).unwrap();
        let windows = input::hours("09:00-17:00").unwrap();
        drop(hours::between(&place("US"), &start, &end, &windows).unwrap());
    });
}
