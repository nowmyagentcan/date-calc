//! Runs the built binary the way people and agents do.

use std::io::Read;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn date_calc(args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_date-calc"));
    cmd.args(args)
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .stdin(Stdio::null());
    cmd
}

/// Exit code, stdout and stderr.
fn run(args: &[&str]) -> (i32, String, String) {
    let Output {
        status,
        stdout,
        stderr,
    } = date_calc(args).output().unwrap();
    let text = |b| String::from_utf8(b).unwrap();
    (status.code().unwrap(), text(stdout), text(stderr))
}

/// The `--json` answer: one line on stdout, nothing on stderr.
fn json(args: &[&str]) -> (i32, Value) {
    let args = [args, &["--json"]].concat();
    let (code, out, err) = run(&args);
    assert_eq!(err, "", "{args:?}");
    assert_eq!(out.matches('\n').count(), 1, "one line: {out}");
    (code, serde_json::from_str(&out).unwrap())
}

/// The answer of a command that must succeed.
fn answer(args: &[&str]) -> Value {
    let (code, v) = json(args);
    assert_eq!(code, 0, "{args:?}: {v}");
    v
}

fn codes(v: &Value) -> Vec<&str> {
    let warnings = v["warnings"].as_array().unwrap();
    warnings
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect()
}

fn data() -> Value {
    json!({"holidays": "python-holidays 0.107 (2026-10-19)", "tzdata": "2026f", "calendars": "icu_calendar 2.4.0", "bikram_sambat": "nepali-datetime 1.0.8.5 (2026-05-07)"})
}

fn holiday(date: &str, weekday: &str, name: &str) -> Value {
    json!({"date": date, "weekday": weekday, "name": name, "observed": false, "estimated": false})
}

#[test]
fn prints_the_version_with_its_data() {
    let want = "date-calc 0.1.0 (python-holidays 0.107 2026-10-19, tzdata 2026f)\n";
    for flag in ["--version", "-V"] {
        assert_eq!(run(&[flag]), (0, want.into(), String::new()));
    }
}

#[test]
fn counts_business_days_as_documented() {
    let v = answer(&["between", "2026-02-01", "2026-02-28", "-c", "CN"]);
    let want = json!({
        "start": "2026-02-01", "end": "2026-02-28", "include_start": true, "include_end": true,
        "country": {"code": "CN", "name": "China"}, "region": null,
        "business_days": 16, "calendar_days": 28, "weekend_days": 6, "holiday_days": 6,
        "elapsed_days": 27, "elapsed": "P27D",
        "holidays": [
            holiday("2026-02-16", "MON", "Chinese New Year's Eve"),
            holiday("2026-02-17", "TUE", "Chinese New Year (Spring Festival)"),
            holiday("2026-02-18", "WED", "Chinese New Year (Spring Festival)"),
            holiday("2026-02-19", "THU", "Chinese New Year (Spring Festival)"),
            holiday("2026-02-20", "FRI", "Day off (substituted from 02/14/2026)"),
            holiday("2026-02-23", "MON", "Day off (substituted from 02/28/2026)"),
        ],
        "weekend_workdays": ["2026-02-14", "2026-02-28"],
        "weekend": ["SAT", "SUN"],
        "warnings": [],
        "data": data(),
    });
    assert_eq!(v, want);
}

/// Arguments, business days, warnings (spec §10.3).
const BETWEEN: &[(&str, i64, &str)] = &[
    ("2026-09-20 2026-10-10 -c CN", 11, ""),
    ("2026-01-01 2026-01-31 -c DE", 21, "regions_differ"),
    ("2026-01-01 2026-01-31 -c DE -r BY", 20, ""),
    ("2026-01-01 2026-01-31 -c DE-BY", 20, ""),
    ("2026-01-01 2026-12-31 -c DE", 254, "regions_differ"),
    ("2026-01-01 2026-12-31 -c DE -r BY", 252, ""),
    ("2026-07-01 2026-07-31 -c US", 22, "regions_differ"),
    (
        "2026-12-24 2027-01-05 -c GB -r ENG",
        6,
        "schedule_not_announced",
    ),
    (
        "2026-12-24 2027-01-05 -c GB -r SCT",
        5,
        "schedule_not_announced",
    ),
    ("2026-05-01 2026-05-31 -c JP", 18, ""),
    ("2021-01-10 2021-01-10 -c AE", 1, ""),
    ("2023-01-08 2023-01-08 -c AE", 0, ""),
    ("2026-02-28 2026-02-01 -c CN", -16, ""),
    (
        "2026-02-01 2026-02-28 -c CN --exclude-start --exclude-end",
        15,
        "",
    ),
    ("2027-02-01 2027-02-28 -c CN", 16, "schedule_not_announced"),
    ("2026-11-02 2026-11-06", 5, "no_country"),
];

#[test]
fn counts_every_documented_interval() {
    for &(args, count, warnings) in BETWEEN {
        let args: Vec<_> = ["between"].into_iter().chain(args.split(' ')).collect();
        let v = answer(&args);
        assert_eq!(v["business_days"], count, "{args:?}");
        assert_eq!(codes(&v).join(" "), warnings, "{args:?}");
    }
    let v = answer(&["between", "2026-02-28", "2026-02-01", "-c", "CN"]);
    assert_eq!(
        (&v["elapsed_days"], &v["elapsed"]),
        (&json!(-27), &json!("-P27D"))
    );
    let v = answer(&["between", "2026-01-01", "2026-01-31", "-c", "DE"]);
    assert!(
        v["warnings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("Epiphany 2026-01-06")
    );
}

#[test]
fn adds_business_days_as_documented() {
    let v = answer(&["add", "2026-02-13", "5", "-c", "CN"]);
    let skip = |date: &str, name: &str| match name {
        "" => json!({"date": date, "reason": "weekend"}),
        _ => json!({"date": date, "reason": "holiday", "name": name}),
    };
    let new_year = "Chinese New Year (Spring Festival)";
    let want = json!({
        "start": "2026-02-13", "amount": 5, "unit": "business_days",
        "result": "2026-02-27", "weekday": "FRI",
        "country": {"code": "CN", "name": "China"}, "region": null, "roll": "none",
        "skipped": [
            skip("2026-02-15", ""),
            skip("2026-02-16", "Chinese New Year's Eve"),
            skip("2026-02-17", new_year),
            skip("2026-02-18", new_year),
            skip("2026-02-19", new_year),
            skip("2026-02-20", "Day off (substituted from 02/14/2026)"),
            skip("2026-02-21", ""),
            skip("2026-02-22", ""),
            skip("2026-02-23", "Day off (substituted from 02/28/2026)"),
        ],
        "weekend_workdays": ["2026-02-14"],
        "weekend": ["SAT", "SUN"],
        "warnings": [],
        "data": data(),
    });
    assert_eq!(v, want);
}

/// Arguments, result, warnings (spec §10.4).
const ADD: &[(&str, &str, &str)] = &[
    ("2026-03-02 -5 -c CN", "2026-02-24", ""),
    ("2026-07-02 1 -c US", "2026-07-06", "regions_differ"),
    ("2026-12-24 1 -c US", "2026-12-28", "regions_differ"),
    ("2026-02-14 0 -c CN", "2026-02-14", ""),
    ("2026-02-15 0 -c CN", "2026-02-15", "start_not_business_day"),
    ("2026-01-31 1 -u months", "2026-02-28", ""),
    (
        "2026-01-31 1 -u months --roll modified-following -c DE",
        "2026-02-27",
        "",
    ),
    (
        "2026-10-03 0 -u days --roll following -c DE",
        "2026-10-05",
        "",
    ),
    (
        "2026-11-02T16:00 3 -u business-hours -c DE",
        "2026-11-03T11:00:00",
        "",
    ),
    ("2026-11-03T10:00 -2 -u bh -c DE", "2026-11-02T16:00:00", ""),
    ("2026-11-02T15:00 2 -u bh -c DE", "2026-11-02T17:00:00", ""),
];

#[test]
fn adds_every_documented_amount() {
    for &(args, result, warnings) in ADD {
        let args: Vec<_> = ["add"].into_iter().chain(args.split(' ')).collect();
        let v = answer(&args);
        assert_eq!(v["result"], result, "{args:?}");
        assert_eq!(codes(&v).join(" "), warnings, "{args:?}");
    }
    let v = answer(&["add", "2026-11-02 16:00", "1.5", "-u", "bh", "-c", "DE"]);
    assert_eq!(v["amount"], 1.5);
    assert_eq!(
        (&v["hours"], &v["timezone"]),
        (&json!("09:00-17:00"), &json!(null))
    );
    let v = answer(&[
        "add",
        "2026-01-31",
        "1",
        "-u",
        "y",
        "--roll",
        "preceding",
        "-c",
        "DE",
    ]);
    assert_eq!(
        (&v["roll"], &v["result"]),
        (&json!("preceding"), &json!("2027-01-29"))
    );
}

#[test]
fn counts_business_hours_as_documented() {
    let v = answer(&["hours", "2026-11-02T10:00", "2026-11-05T15:00", "-c", "DE"]);
    let want = json!({
        "start": "2026-11-02T10:00:00", "end": "2026-11-05T15:00:00",
        "country": {"code": "DE", "name": "Germany"}, "region": null,
        "hours": "09:00-17:00", "timezone": null,
        "seconds": 104400, "business_hours": 29.0, "duration": "29:00", "business_days": 4,
        "holidays": [], "weekend_workdays": [], "weekend": ["SAT", "SUN"],
        "warnings": [], "data": data(),
    });
    assert_eq!(v, want);
    let split = ["--hours", "09:00-12:00,13:00-17:00"];
    let v = answer(
        &[
            &["hours", "2026-11-02 10:00", "2026-11-05 15:00", "-c", "DE"],
            &split[..],
        ]
        .concat(),
    );
    assert_eq!(
        (&v["duration"], &v["hours"]),
        (&json!("25:00"), &json!("09:00-12:00,13:00-17:00"))
    );
    let utc = [
        "2026-10-30T16:00:00Z",
        "2026-11-02T18:00:00Z",
        "-c",
        "CA",
        "-r",
        "BC",
    ];
    let v = answer(&[&["hours"], &utc[..], &["--tz", "America/Vancouver"]].concat());
    assert_eq!(v["duration"], "10:00");
    assert_eq!(v["start"], "2026-10-30T09:00:00-07:00");
    assert_eq!(v["timezone"], "America/Vancouver");
    let (code, v) = json(&[&["hours"], &utc[..]].concat());
    assert_eq!((code, &v["error"]["code"]), (1, &json!("tz_required")));
    let day = [
        "2026-03-08 00:00",
        "2026-03-09 00:00",
        "--hours",
        "00:00-24:00",
        "--weekend",
        "none",
    ];
    let v = answer(&[&["hours"], &day[..], &["--tz", "America/New_York"]].concat());
    assert_eq!(v["duration"], "23:00");
    let v = answer(&[&["hours"], &day[..]].concat());
    assert_eq!(v["duration"], "24:00");
}

#[test]
fn turns_away_bad_usage_with_exit_2() {
    for (args, message) in [
        (
            &["between", "2026-01-01", "2026-01-02", "-r", "BY"][..],
            "the following required arguments were not provided: --country <C>",
        ),
        (
            &["add", "2026-01-01", "1", "--roll", "following"],
            "--roll works only with --unit days, weeks, months or years.",
        ),
        (
            &["hours", "2026-01-01", "2026-01-02", "--exclude-end"],
            "--exclude-start and --exclude-end work only with between.",
        ),
        (
            &["add", "2026-01-01", "1000001"],
            "Amount is over the limit for business days (1,000,000).",
        ),
        (
            &["add", "2026-01-01", "1", "-u", "fortnights"],
            "invalid value 'fortnights' for '--unit <UNIT>' [possible values: business-days, days, weeks, months, years, business-hours]",
        ),
        (
            &["between", &"9".repeat(257), "2026-01-02"],
            "An argument is longer than 256 bytes.",
        ),
    ] {
        let (code, v) = json(args);
        assert_eq!(code, 2, "{args:?}");
        assert_eq!(v["error"]["code"], "usage", "{args:?}");
        assert_eq!(v["error"]["message"], message, "{args:?}");
        let clap = !message.ends_with('.');
        let hint = if clap {
            json!("For more information, try '--help'.")
        } else {
            json!(null)
        };
        assert_eq!(v["error"]["hint"], hint, "{args:?}");
        let (code, out, _) = run(args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
    }
}

#[test]
fn lists_holidays_by_country_and_region() {
    let v = answer(&["holidays", "CN", "2026"]);
    assert_eq!(v["holidays"].as_array().unwrap().len(), 22);
    let workdays = [
        "2026-01-04",
        "2026-02-14",
        "2026-02-28",
        "2026-05-09",
        "2026-09-20",
        "2026-10-10",
    ];
    assert_eq!(v["weekend_workdays"], json!(workdays));
    let v = answer(&["holidays", "DE", "2026"]);
    assert_eq!(
        (v["holidays"].as_array().unwrap().len(), &v["region"]),
        (9, &json!(null))
    );
    assert_eq!(
        v["holidays"][0],
        json!({"date": "2026-01-01", "weekday": "THU", "name": "New Year's Day", "observed": false, "estimated": false, "scope": "national"})
    );
    assert_eq!(
        v["regional"][0],
        json!({"date": "2026-01-06", "weekday": "TUE", "name": "Epiphany", "observed": false, "estimated": false, "regions": ["Augsburg", "BW", "BY", "ST"]})
    );
    assert_eq!(
        (&v["from"], &v["to"]),
        (&json!("2026-01-01"), &json!("2026-12-31"))
    );
    for args in [
        &["holidays", "DE", "2026", "-r", "BY"][..],
        &["holidays", "DE-BY", "2026"],
    ] {
        let v = answer(args);
        let regional = v["holidays"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|h| h["scope"] == "regional");
        let names: Vec<_> = regional.map(|h| h["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Epiphany", "Corpus Christi", "All Saints' Day"]);
        assert_eq!(v["holidays"].as_array().unwrap().len(), 12);
        assert_eq!(
            (&v["region"], &v["regional"]),
            (&json!({"code": "BY", "name": "Bayern"}), &json!([]))
        );
    }
    let eid = |v: &Value| -> Vec<bool> {
        let fitr = v["holidays"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|h| h["name"].as_str().unwrap().starts_with("Eid al-Fitr"));
        fitr.map(|h| h["estimated"].as_bool().unwrap()).collect()
    };
    let v = answer(&["holidays", "SA", "2026"]);
    assert!(!eid(&v).is_empty() && !eid(&v).contains(&true), "{v}");
    let v = answer(&["holidays", "AE", "2026"]);
    assert!(!eid(&v).is_empty() && !eid(&v).contains(&false), "{v}");
    assert_eq!(codes(&v), ["schedule_not_announced", "estimated_dates"]);
    let v = answer(&[
        "holidays",
        "US",
        "--from",
        "2026-07-01",
        "--to",
        "2026-07-31",
    ]);
    let flags: Vec<_> = v["holidays"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| (&h["date"], &h["observed"]))
        .collect();
    assert_eq!(
        flags,
        [
            (&json!("2026-07-03"), &json!(true)),
            (&json!("2026-07-04"), &json!(false))
        ]
    );
    let (code, v) = json(&[
        "holidays",
        "DE",
        "2026",
        "--from",
        "2026-01-01",
        "--to",
        "2026-02-01",
    ]);
    assert_eq!(
        (code, &v["error"]["message"]),
        (2, &json!("Give both --from and --to, or a YEAR."))
    );
}

#[test]
fn converts_dates_as_documented() {
    let v = answer(&["convert", "2026-10-27"]);
    let note_uq = "Saudi Arabia's official Umm al-Qura calendar. Religious dates set by moon sighting can differ by a day, and more in other countries.";
    let note_tab = "Tabular Islamic calendar (Friday epoch, 30-year cycle); usually 0-2 days from Umm al-Qura.";
    let want = json!({
        "input": "2026-10-27", "from": "gregorian",
        "gregorian": {"date": "2026-10-27", "weekday": "TUE", "iso_week": "2026-W44", "day_of_year": 300},
        "dates": [
            {"calendar": "islamic-umalqura", "year": 1448, "month": 5, "leap_month": false, "month_name": "Jumada al-Ula", "day": 16, "text": "16 Jumada al-Ula 1448 AH", "note": note_uq},
            {"calendar": "islamic-civil", "year": 1448, "month": 5, "leap_month": false, "month_name": "Jumada al-Ula", "day": 15, "text": "15 Jumada al-Ula 1448 AH", "note": note_tab},
            {"calendar": "hebrew", "year": 5787, "month": 2, "leap_month": false, "month_name": "Cheshvan", "day": 16, "text": "16 Cheshvan 5787", "note": null},
            {"calendar": "chinese", "year": 2026, "month": 9, "leap_month": false, "month_name": "Month 9", "day": 18, "text": "Month 9, day 18, year of the Horse (Bing-Wu)", "cyclic_year": "Bing-Wu", "zodiac": "Horse", "note": null},
            {"calendar": "persian", "year": 1405, "month": 8, "leap_month": false, "month_name": "Aban", "day": 5, "text": "5 Aban 1405 SH", "note": null},
            {"calendar": "bikram-sambat", "year": 2083, "month": 7, "leap_month": false, "month_name": "Kartik", "day": 10, "text": "10 Kartik 2083 BS", "note": null},
        ],
        "warnings": [],
        "data": data(),
    });
    assert_eq!(v, want);
    for (args, gregorian, warnings) in [
        ("1 Muharram 1448", "2026-06-16", ""),
        ("1448-01-01 --from islamic-umalqura", "2026-06-16", ""),
        ("1 Ramadan 1447", "2026-02-18", ""),
        ("1 Tishrei 5787", "2026-09-12", ""),
        ("5787-05L-01 --from hebrew", "2027-02-08", ""),
        ("1 Adar I 5787", "2027-02-08", ""),
        ("2026-01-01 --from chinese", "2026-02-17", ""),
        ("2025-06L-01 --from chinese", "2025-07-25", ""),
        ("1405-01-01 --from persian", "2026-03-21", ""),
        ("2083-01-01 --from bs", "2026-04-14", ""),
        ("10 Kartik 2083", "2026-10-27", ""),
        ("2084-01-01 --from bs", "2027-04-14", "bs_projected"),
        (
            "1882-11-11",
            "1882-11-11",
            "calendar_out_of_range calendar_out_of_range chinese_calculated",
        ),
    ] {
        let args: Vec<_> = ["convert"].into_iter().chain(args.split(' ')).collect();
        let v = answer(&args);
        assert_eq!(v["gregorian"]["date"], gregorian, "{args:?}");
        assert_eq!(codes(&v).join(" "), warnings, "{args:?}");
    }
    let v = answer(&["convert", "1882-11-11"]);
    assert!(
        v["dates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["calendar"] != "islamic-umalqura")
    );
    let v = answer(&["convert", "2026-10-27", "--to", "persian"]);
    assert_eq!(v["dates"].as_array().unwrap().len(), 1);
    for (args, code, hint) in [
        (
            "5786-05L-01 --from hebrew",
            "bad_calendar_date",
            json!(null),
        ),
        (
            "30 Safar 1447",
            "bad_calendar_date",
            json!("Safar 1447 has 29 days."),
        ),
        (
            "1882-11-11 --to islamic-umalqura",
            "calendar_out_of_range",
            json!(null),
        ),
        (
            "2026-10-27 --to julian",
            "unknown_calendar",
            json!(
                "Use gregorian, islamic-umalqura, islamic-civil, hebrew, chinese, persian or bikram-sambat."
            ),
        ),
    ] {
        let args: Vec<_> = ["convert"].into_iter().chain(args.split(' ')).collect();
        let (status, v) = json(&args);
        assert_eq!(
            (status, &v["error"]["code"], &v["error"]["hint"]),
            (1, &json!(code), &hint),
            "{args:?}"
        );
    }
}

#[test]
fn lists_countries_and_one_country() {
    let v = answer(&["countries"]);
    let countries = v["countries"].as_array().unwrap();
    assert_eq!(countries.len(), 250);
    let cn = countries.iter().find(|c| c["code"] == "CN").unwrap();
    assert_eq!(
        cn,
        &json!({"code": "CN", "name": "China", "aliases": ["CHN"], "region_count": 0, "weekend": ["SAT", "SUN"], "years": [2000, 2050], "decree_through": 2026})
    );
    assert_eq!(v["data"], data());
    let v = answer(&["countries", "DE"]);
    let regions = v["regions"].as_array().unwrap();
    assert_eq!((regions.len(), &v["region_count"]), (17, &json!(17)));
    assert!(regions.contains(&json!({"code": "BY", "names": ["Bayern"]})));
    assert!(regions.contains(&json!({"code": "Augsburg", "names": []})));
    let v = answer(&["countries", "united arab emirates"]);
    assert_eq!(
        v["weekend_history"],
        json!([
            {"from": "2000-01-01", "weekend": ["FRI", "SAT"], "region": null},
            {"from": "2022-01-01", "weekend": ["SAT", "SUN"], "region": null},
        ])
    );
}

#[test]
fn prints_text_for_people() {
    let text = |args: &str| {
        let (code, out, err) = run(&args.split(' ').collect::<Vec<_>>());
        assert_eq!((code, err.as_str()), (0, ""), "{args}");
        out
    };
    assert_eq!(
        text("between 2026-02-01 2026-02-28 -c CN"),
        "16 business days
2026-02-01 to 2026-02-28 (both counted) · China
28 days: 6 weekend, 6 holidays · 27 days apart
  Mon 2026-02-16  Chinese New Year's Eve
  Tue 2026-02-17  Chinese New Year (Spring Festival)
  Wed 2026-02-18  Chinese New Year (Spring Festival)
  Thu 2026-02-19  Chinese New Year (Spring Festival)
  Fri 2026-02-20  Day off (substituted from 02/14/2026)
  Mon 2026-02-23  Day off (substituted from 02/28/2026)
Worked weekend days: Sat 2026-02-14, Sat 2026-02-28
python-holidays 0.107 (2026-10-19)
"
    );
    assert_eq!(
        text("add 2026-01-31 1 -u months --roll modified-following -c DE"),
        "Fri 2026-02-27
1 month after Sat 2026-01-31 · Germany
Rolled modified-following past 1 day:
  Sat 2026-02-28  weekend
python-holidays 0.107 (2026-10-19)
"
    );
    assert_eq!(
        text("hours 2026-11-02T10:00 2026-11-05T15:00 -c DE"),
        "29:00 business hours (29.00 h)
Mon 2026-11-02 10:00 to Thu 2026-11-05 15:00 · Germany
Hours 09:00-17:00 on 4 business days · clock time
python-holidays 0.107 (2026-10-19)
"
    );
    let holidays = text("holidays DE 2026");
    assert!(
        holidays.starts_with(
            "Germany · 2026 · 9 national holidays\n  Thu 2026-01-01  New Year's Day\n"
        )
    );
    assert!(holidays.contains(
        "\nRegional:\n  Tue 2026-01-06  Epiphany                   Augsburg, BW, BY, ST\n"
    ));
    assert!(text("holidays DE 2026 -r BY").contains("\n  Tue 2026-01-06  Epiphany (regional)\n"));
    let us = text("holidays US --from 2026-07-01 --to 2026-07-31");
    assert!(us.starts_with("United States · 2026-07-01 to 2026-07-31 · 2 national holidays\n  Fri 2026-07-03  Independence Day (observed) (not in MA)\n"), "{us}");
    assert_eq!(
        text("convert 2026-10-27 --to hebrew"),
        "Gregorian  Tue 2026-10-27 · week 2026-W44 · day 300
Hebrew     16 Cheshvan 5787
icu_calendar 2.4.0 · nepali-datetime 1.0.8.5 (2026-05-07)
"
    );
    assert_eq!(
        text("countries AE"),
        "United Arab Emirates (AE, ARE) · 2000-2050
Weekend:
  from 2000-01-01  Fri, Sat
  from 2022-01-01  Sat, Sun
python-holidays 0.107 (2026-10-19)
"
    );
    assert!(text("countries").contains("\nCN  China "));
    let skipped = text("add 2000-01-03 50 -c US -r NY");
    assert!(skipped.contains("\n  … and 13 more\n"), "{skipped}");
    let zoned = text("add 2026-11-02T16:00 3 -u bh -c CA-BC --tz America/Vancouver");
    assert!(zoned.starts_with("Tue 2026-11-03 11:00 -07:00\n3 business hours after Mon 2026-11-02 16:00 -07:00 · Canada, British Columbia\nHours 09:00-17:00 · America/Vancouver\n"), "{zoned}");
    assert!(zoned.ends_with("python-holidays 0.107 (2026-10-19) · tzdata 2026f\n"));
    let (code, out, err) = run(&["between", "2026-02-30", "2026-03-05"]);
    let want = "✗ Not a date: '2026-02-30'.\n  Use YYYY-MM-DD, e.g. 2026-02-01, or today.\n";
    assert_eq!((code, out.as_str(), err.as_str()), (1, "", want));
}

#[test]
fn colours_only_when_asked_and_never_with_no_color() {
    let (_, out, _) = run(&["between", "2026-02-01", "2026-02-28", "-c", "CN"]);
    assert!(!out.contains('\x1b'));
    let forced = |no_color: &str| {
        let out = date_calc(&["between", "2026-02-01", "2026-02-28", "-c", "CN"])
            .env("CLICOLOR_FORCE", "1")
            .env("NO_COLOR", no_color)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap()
    };
    assert!(!forced("1").contains('\x1b'));
    if cfg!(not(windows)) {
        // Windows turns ANSI codes into console calls, which a pipe can't take.
        assert!(forced("").starts_with("\x1b[1m16 business days\x1b[0m\n\x1b[2m"));
    }
}

#[test]
fn shows_examples_in_help() {
    for args in [&["--help"][..], &["between", "--help"], &["convert", "-h"]] {
        let (code, out, _) = run(args);
        assert_eq!(code, 0);
        assert!(
            out.contains("\nExamples:\n  date-calc between 2026-02-01 2026-02-28 -c CN\n"),
            "{args:?}"
        );
        assert!(out.ends_with("  date-calc countries DE\n"), "{args:?}");
    }
    let (_, out, _) = run(&["--help"]);
    assert!(out.starts_with("Count business days, add workdays, list holidays, convert dates.\n"));
    let (_, out, _) = run(&["add", "--help"]);
    assert!(out.contains("[default: 09:00-17:00]"));
}

/// One run of every command, with lots of output so a closed pipe is hit.
const EVERY_COMMAND: &[&[&str]] = &[
    &["between", "2000-01-01", "2050-12-31", "-c", "DE"],
    &["add", "2000-01-03", "12000", "-c", "US"],
    &["hours", "2026-01-01", "2026-12-31", "-c", "US"],
    &[
        "holidays",
        "US",
        "--from",
        "2000-01-01",
        "--to",
        "2050-12-31",
    ],
    &["convert", "2026-10-27"],
    &["countries"],
    &["between", "bad", "2026-01-01"],
    &["add", "2026-01-01", "1", "--frobnicate"],
    &["--help"],
];

#[test]
fn ends_quietly_when_stdout_closes() {
    for &args in EVERY_COMMAND {
        for extra in [&[][..], &["--json"], &["--debug"], &["--json", "--debug"]] {
            let args = [args, extra].concat();
            let mut child = date_calc(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            drop(child.stdout.take());
            let mut err = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut err)
                .unwrap();
            let code = child.wait().unwrap().code();
            assert!(!err.contains("panicked"), "{args:?}: {err}");
            if !err.contains("bad") && !err.contains("frobnicate") {
                assert_eq!((code, err.as_str()), (Some(0), ""), "{args:?}");
            }
        }
    }
}

#[test]
fn answers_even_when_stderr_closes() {
    for &args in EVERY_COMMAND {
        for extra in [&[][..], &["--json", "--debug"], &["--debug"]] {
            let args = [args, extra].concat();
            let want = date_calc(&args).output().unwrap();
            let mut child = date_calc(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            drop(child.stderr.take());
            let mut out = Vec::new();
            child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
            let code = child.wait().unwrap().code();
            assert_eq!((code, out), (want.status.code(), want.stdout), "{args:?}");
        }
    }
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;

/// Waits up to 5 s for the child to end by itself; kills it otherwise.
fn ended(child: &mut std::process::Child) -> Option<i32> {
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 5 {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    None
}

fn stderr_of(child: &mut std::process::Child) -> String {
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    err
}

#[test]
fn servers_end_quietly_when_stdout_closes() {
    use std::io::Write;
    for args in [
        &["ui", "--no-open"][..],
        &["ui", "--no-open", "--debug"],
        &["mcp"],
        &["mcp", "--debug"],
    ] {
        let mut child = date_calc(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let mut stdin = child.stdin.take().unwrap();
        let _ = writeln!(stdin, "{INITIALIZE}");
        let code = ended(&mut child);
        drop(stdin);
        let err = stderr_of(&mut child);
        assert_eq!((code, err.as_str()), (Some(0), ""), "{args:?}");
    }
}

#[test]
fn mcp_ends_quietly_when_the_client_leaves_mid_session() {
    use std::io::Write;
    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_countries","arguments":{}}}"#;
    for half_line in [true, false] {
        let mut child = date_calc(&["mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        writeln!(stdin, "{INITIALIZE}").unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
        )
        .unwrap();
        if half_line {
            // The client dies halfway through a message.
            write!(stdin, "{}", &call[..30]).unwrap();
        } else {
            // The client stops reading while a large answer is on its way, then leaves.
            writeln!(stdin, "{call}").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(200));
            drop(child.stdout.take());
        }
        drop(stdin);
        let code = ended(&mut child);
        let err = stderr_of(&mut child);
        assert!(!err.contains("panicked"), "{err}");
        assert_eq!(code, Some(0), "{err}");
    }
}

#[test]
fn page_keeps_serving_when_clients_hang_up() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Shutdown, TcpStream};
    let mut child = date_calc(&["ui", "--no-open", "--debug"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port: u16 = line
        .trim_end()
        .trim_end_matches("  (Ctrl+C to stop)")
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let body = r#"{"op":"countries"}"#;
    let request = format!(
        "POST /api HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let connect = || TcpStream::connect(("127.0.0.1", port)).unwrap();
    // Leaves after half the headers, after half the body, and before reading the answer.
    connect().write_all(&request.as_bytes()[..20]).unwrap();
    connect()
        .write_all(&request.as_bytes()[..request.len() - 5])
        .unwrap();
    for _ in 0..20 {
        let mut s = connect();
        s.write_all(request.as_bytes()).unwrap();
        s.shutdown(Shutdown::Both).unwrap();
    }
    let mut s = connect();
    s.write_all(request.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"), "{answer}");
    assert!(answer.contains("\"countries\":[{\"code\":\"AD\""));
    assert_eq!(child.try_wait().unwrap(), None, "the server stopped");
    child.kill().unwrap();
    child.wait().unwrap();
    let err = stderr_of(&mut child);
    assert_eq!(err, "");
}

#[test]
fn servers_run_with_stderr_closed() {
    use std::io::{BufRead, BufReader};
    let mut child = date_calc(&["ui", "--no-open", "--debug"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stderr.take());
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.starts_with("Open http://127.0.0.1:"), "{line}");
    child.kill().unwrap();
    child.wait().unwrap();
    let port = line.trim_end().trim_end_matches("  (Ctrl+C to stop)");
    let port = port.rsplit(':').next().unwrap();
    let _taken = std::net::TcpListener::bind(("127.0.0.1", port.parse::<u16>().unwrap()));
    let mut busy = date_calc(&["ui", "--no-open", "--port", port, "--debug"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(busy.stderr.take());
    assert_eq!(ended(&mut busy), Some(3));
    let out = date_calc(&["mcp", "--debug"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn shows_the_cause_with_debug() {
    let (code, out, err) = run(&["between", "2026-13-01", "2026-12-01", "--debug"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(
        err.starts_with("debug: Error { code: \"bad_date\""),
        "{err}"
    );
    assert!(
        err.ends_with(
            "✗ Not a date: '2026-13-01'.\n  Use YYYY-MM-DD, e.g. 2026-02-01, or today.\n"
        )
    );
    let (code, v) = json(&["countries", "XX"]);
    assert_eq!((code, &v["error"]["code"]), (1, &json!("unknown_country")));
    let (_, out, err) = run(&["countries", "XX", "--json", "--debug"]);
    assert!(
        err.starts_with("debug: Error { code: \"unknown_country\""),
        "{err}"
    );
    assert!(out.starts_with("{\"error\":"));
}

#[test]
fn asks_for_a_command_without_a_terminal() {
    let want = "✗ No command given.\n  Try: date-calc between 2026-02-01 2026-02-28 -c CN\n";
    assert_eq!(run(&[]), (2, String::new(), want.into()));
    let (_, out, _) = run(&["--help"]);
    assert!(out.contains("\nUsage: date-calc [COMMAND]\n"));
}

/// Arguments, code, message, hint (spec §8.1).
const ERRORS: &[(&str, &str, &str, Option<&str>)] = &[
    (
        "between x 2026-01-01",
        "bad_date",
        "Not a date: 'x'.",
        Some("Use YYYY-MM-DD, e.g. 2026-02-01, or today."),
    ),
    (
        "hours 2026-11-02T25:00 2026-11-03",
        "bad_datetime",
        "Not a date and time: '2026-11-02T25:00'.",
        Some(
            "Use YYYY-MM-DDTHH:MM, e.g. 2026-11-02T09:30, optionally with Z or an offset, or now.",
        ),
    ),
    (
        "between 10000-01-01 2026-01-01",
        "date_out_of_range",
        "10000-01-01 is outside 0001-01-01 to 9999-12-31.",
        None,
    ),
    (
        "between 2026-01-01 2026-01-02 -c XX",
        "unknown_country",
        "Unknown country: 'XX'.",
        Some("Use an ISO code such as DE or DEU, or run: date-calc countries"),
    ),
    (
        "countries Atlantis",
        "unknown_country",
        "Unknown country: 'Atlantis'.",
        Some("Use an ISO code such as DE or DEU, or run: date-calc countries"),
    ),
    (
        "between 2026-01-01 2026-01-02 -c DE -r ZZ",
        "unknown_region",
        "Germany has no region 'ZZ'.",
        Some(
            "Regions: BB, BE, BW, BY, HB, HE, HH, MV, NI, NW, RP, SH, … Run: date-calc countries DE",
        ),
    ),
    (
        "holidays GB-ZZ 2026",
        "unknown_region",
        "United Kingdom has no region 'ZZ'.",
        Some("Regions: ENG, NIR, SCT, WLS. Run: date-calc countries GB"),
    ),
    (
        "between 2026-01-01 2026-01-02 -c CN -r ZZ",
        "unknown_region",
        "China has no region 'ZZ'.",
        Some("China has no regions in the holiday data."),
    ),
    (
        "between 2027-01-04 2027-01-05 -c LK",
        "no_holiday_data",
        "Holiday data for Sri Lanka covers 2003-2026; 2027-01-04 is outside it.",
        Some("Pick dates inside that range, or leave out --country to skip only weekends."),
    ),
    (
        "holidays DE 2051",
        "no_holiday_data",
        "Holiday data for Germany covers 2000-2050; 2051-01-01 is outside it.",
        Some("Pick dates inside that range, or leave out --country to skip only weekends."),
    ),
    (
        "between 2026-01-01 2026-01-02 --weekend XYZ",
        "bad_weekend",
        "Not a weekend: 'XYZ'.",
        Some("Use day names like SAT,SUN, a 7-digit mask from Monday like 0000011, or none."),
    ),
    (
        "add 2026-01-01 1 --weekend 1111111",
        "no_business_days",
        "Every day is a weekend day, so there are no business days.",
        Some("Change --weekend."),
    ),
    (
        "hours 2026-01-01 2026-01-02 --hours 9-5",
        "bad_hours",
        "Not working hours: '9-5'.",
        Some("Use HH:MM-HH:MM, several separated by commas, e.g. 09:00-12:00,13:00-17:00."),
    ),
    (
        "hours 2026-01-01 2026-01-02 --hours 09:00-12:00,11:00-13:00",
        "overlapping_hours",
        "Working hours overlap: 09:00-12:00 and 11:00-13:00.",
        Some("Give windows that don't overlap, in any order."),
    ),
    (
        "hours 2026-01-01 2026-01-02 --tz Mars/Base",
        "unknown_timezone",
        "Unknown time zone: 'Mars/Base'.",
        Some("Use an IANA name such as Europe/Berlin or America/New_York."),
    ),
    (
        "hours 2026-10-30T16:00:00Z 2026-11-02T18:00:00Z",
        "tz_required",
        "2026-10-30T16:00:00Z has a UTC offset, so the business's time zone is needed.",
        Some("Add --tz, e.g. --tz Europe/Berlin, or give local times without an offset."),
    ),
    (
        "add 2026-01-01 abc",
        "bad_amount",
        "Not a valid amount: 'abc'.",
        Some("Use a whole number like 10 or -5 (business hours may have 2 decimals, like 1.5)."),
    ),
    (
        "add 9999-12-31 1 -u days",
        "result_out_of_range",
        "The result would be outside 0001-9999.",
        Some("Use a smaller amount."),
    ),
    (
        "add 2050-12-30 5 -c DE",
        "result_out_of_range",
        "The result would be outside 2000-2050.",
        Some("Use a smaller amount."),
    ),
    (
        "convert 2026-10-27 --from julian",
        "unknown_calendar",
        "Unknown calendar: 'julian'.",
        Some(
            "Use gregorian, islamic-umalqura, islamic-civil, hebrew, chinese, persian or bikram-sambat.",
        ),
    ),
    (
        "convert 30 Safar 1447",
        "bad_calendar_date",
        "There is no 30 Safar 1447 in the Umm al-Qura calendar.",
        Some("Safar 1447 has 29 days."),
    ),
    (
        "convert 1882-11-11 --to islamic-umalqura",
        "calendar_out_of_range",
        "The Umm al-Qura table covers 1300-1600 AH (1882-11-12 to 2174-11-25).",
        None,
    ),
    (
        "convert 2101-01-01 --from bs",
        "calendar_out_of_range",
        "The Bikram Sambat table covers 1975-2100 BS (1918-04-13 to 2044-04-12).",
        None,
    ),
];

#[test]
fn reports_every_error_with_its_message_and_hint() {
    for &(args, code, message, hint) in ERRORS {
        let words: Vec<_> = args.split(' ').collect();
        let (status, v) = json(&words);
        let want = json!({"error": {"code": code, "message": message, "hint": hint}});
        assert_eq!((status, v), (1, want), "{args}");
        let (status, out, err) = run(&words);
        let hint = hint.map(|h| format!("  {h}\n")).unwrap_or_default();
        assert_eq!(
            (status, out, err),
            (1, String::new(), format!("✗ {message}\n{hint}")),
            "{args}"
        );
    }
}

/// Arguments, the warning code, and the start of its message (spec §8.2).
const WARNINGS: &[(&str, &str, &str)] = &[
    (
        "between 2026-11-02 2026-11-06",
        "no_country",
        "No country given, so only weekends (Sat, Sun) are skipped.",
    ),
    (
        "hours 2026-11-02 2026-11-03 --weekend none",
        "no_country",
        "No country given, so only weekends (none) are skipped.",
    ),
    (
        "between 2026-01-01 2026-01-31 -c DE",
        "regions_differ",
        "Germany has regional holidays in this range (Epiphany 2026-01-06 (Augsburg, BW, BY, ST)); this uses national holidays only. Add --region, e.g. BB.",
    ),
    (
        "between 2027-02-01 2027-02-28 -c CN",
        "schedule_not_announced",
        "China announces one-off holidays or workday swaps most years; the bundled data has them through 2026. Later dates follow the regular rules and may change.",
    ),
    (
        "holidays AE 2026",
        "estimated_dates",
        "Some holiday dates are estimates (lunar calendar): ",
    ),
    (
        "add 2026-02-15 0 -c CN",
        "start_not_business_day",
        "2026-02-15 is not a business day in China.",
    ),
    (
        "convert 1882-11-11",
        "calendar_out_of_range",
        "The Umm al-Qura table covers 1300-1600 AH (1882-11-12 to 2174-11-25); left out.",
    ),
    (
        "convert 2084-01-01 --from bs",
        "bs_projected",
        "Bikram Sambat dates after 2083 BS come from a projected table; Nepal publishes each year's official calendar about a year ahead.",
    ),
    (
        "convert 2110-06-01 --to chinese",
        "chinese_calculated",
        "Chinese dates outside 1900-2102 are calculated, not from the published tables, and may be a day off.",
    ),
    (
        "between 2026-02-01 2026-02-28 -c CN --weekend SAT,SUN",
        "weekend_override",
        "Weekend days worked by decree still count as business days: 2026-02-14, 2026-02-28.",
    ),
];

#[test]
fn reaches_every_warning() {
    for &(args, code, message) in WARNINGS {
        let words: Vec<_> = args.split(' ').collect();
        let v = answer(&words);
        let warnings = v["warnings"].as_array().unwrap();
        let found = warnings.iter().find(|w| w["code"] == code);
        let text = found
            .and_then(|w| w["message"].as_str())
            .unwrap_or_default();
        assert!(text.starts_with(message), "{args}: {v}");
        let (_, out, _) = run(&words);
        assert!(out.contains(&format!("\n! {text}\n")), "{args}: {out}");
    }
}
