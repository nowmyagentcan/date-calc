//! The bundled Bikram Sambat table: its shape, its end dates, and medic/bikram-sambat's table.

use jiff::civil::{Date, date};

const TABLE: &str = include_str!("../data/calendar_bs.csv");
const MEDIC: &str = include_str!("bs_medic.txt");

/// Year and month lengths per line after the header.
fn rows(text: &str, sep: char) -> Vec<(i32, Vec<i32>)> {
    let row = |line: &str| {
        let mut n = line.split(sep).map(|f| f.parse().unwrap());
        (n.next().unwrap(), n.collect())
    };
    text.lines()
        .filter(|l| l.as_bytes()[0].is_ascii_digit())
        .map(row)
        .collect()
}

fn days(months: &[i32]) -> i64 {
    months.iter().map(|&n| i64::from(n)).sum()
}

#[test]
fn table_runs_from_1975_to_2100_bs() {
    assert_eq!(
        TABLE.lines().next(),
        Some(
            "Year,Baisakh,Jestha,Ashar,Shrawan,Bhadra,Asoj,Kartik,Mangsir,Poush,Magh,Falgun,Chait"
        )
    );
    let rows = rows(TABLE, ',');
    let years: Vec<_> = rows.iter().map(|r| r.0).collect();
    assert_eq!(years, (1975..=2100).collect::<Vec<_>>());
    assert!(rows.iter().all(|r| r.1.len() == 12));
    // 1 Baisakh 1975 BS = 1918-04-13; the last day, 2100-12-30 BS, is 2044-04-12.
    let total: i64 = rows.iter().map(|r| days(&r.1)).sum();
    let span = date(2044, 4, 13) - date(1918, 4, 13);
    assert_eq!(total, i64::from(span.get_days()));
}

#[test]
fn table_matches_medic_from_1999_to_2083_bs() {
    let table = rows(TABLE, ',');
    let medic = rows(MEDIC, ' ');
    let start: Date = MEDIC
        .lines()
        .find_map(|l| l.strip_prefix("start "))
        .unwrap()
        .parse()
        .unwrap();
    // Both tables put 1 Baisakh 1999 BS on the same day...
    let before: i64 = table
        .iter()
        .take_while(|r| r.0 < 1999)
        .map(|r| days(&r.1))
        .sum();
    assert_eq!(start, date(1918, 4, 13) + jiff::Span::new().days(before));
    // ... and agree on every month after it but Baisakh and Jestha 2062 (31/31 here, 30/32
    // there; Hamro Patro has 31/31), so every date in 1999-2083 BS matches but those 62.
    assert_eq!(medic.first().unwrap().0, 1999);
    assert_eq!(medic.last().unwrap().0, 2083);
    for (year, months) in &medic {
        let ours = &table.iter().find(|r| r.0 == *year).unwrap().1;
        if *year == 2062 {
            assert_eq!((&ours[..2], &months[..2]), (&[31, 31][..], &[30, 32][..]));
            assert_eq!(ours[2..], months[2..]);
        } else {
            assert_eq!(ours, months, "{year}");
        }
    }
}
