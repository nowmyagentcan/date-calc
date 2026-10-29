//! Runs the built binary the way people and agents do.

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
