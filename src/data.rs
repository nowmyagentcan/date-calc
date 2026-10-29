//! The embedded holiday data (`data/holidays.txt`, written by `scripts/export_holidays.py`).
//! Only the requested country's block is parsed.

use jiff::civil::Date;

use crate::error::Error;

static DATA: &str = include_str!("../data/holidays.txt");

/// The country line (`C code name aliases first last decree_through`) and the block's first lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Country {
    pub code: &'static str,
    pub name: &'static str,
    pub aliases: Vec<&'static str>,
    pub years: (i16, i16),
    /// The last year with one-off holidays or workday swaps, for countries that announce them most years.
    pub decree_through: Option<i16>,
    pub region_count: usize,
    /// The national weekend at the start of the data years.
    pub weekend: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub code: &'static str,
    pub names: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holiday {
    pub date: Date,
    pub name: &'static str,
    pub observed: bool,
    pub estimated: bool,
    /// The region that adds this holiday, or `None` for a national one.
    pub region: Option<usize>,
    /// Regions that don't keep this national holiday.
    pub except: Vec<usize>,
}

/// A weekend in force from a date on: bit 0 is Monday, bit 6 Sunday.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weekend {
    pub region: Option<usize>,
    pub from: Date,
    pub days: u8,
}

/// One country's data. Region arguments are indexes into `regions`; `None` means national.
#[derive(Debug)]
pub struct Block {
    pub country: Country,
    pub regions: Vec<Region>,
    /// Sorted by date.
    pub holidays: Vec<Holiday>,
    pub weekends: Vec<Weekend>,
    /// Weekend days worked by decree, sorted by date.
    pub workdays: Vec<(Option<usize>, Date)>,
}

/// The python-holidays version and release date.
pub fn source() -> (&'static str, &'static str) {
    let v: Vec<_> = DATA[DATA.find("\nV\t").expect("version line") + 3..]
        .split(['\t', '\n'])
        .take(2)
        .collect();
    (v[0], v[1])
}

/// Each country's block, starting with the country line's fields after `C\t`.
fn blocks() -> impl Iterator<Item = &'static str> {
    DATA[DATA.find("\nC\t").expect("country lines") + 3..].split("\nC\t")
}

/// Every country, in code order, without parsing their holidays.
pub fn countries() -> impl Iterator<Item = Country> {
    blocks().map(Country::parse)
}

/// Finds a country by alpha-2, alpha-3, alias or English name (case, spaces and punctuation
/// ignored) and the region in it. `CC-REGION` names the region when the whole text isn't a
/// country (`Guinea-Bissau` is one); an explicit `region` wins over it.
pub fn find(country: &str, region: Option<&str>) -> Result<(Block, Option<usize>), Error> {
    let (text, region) = match find_block(country) {
        Some(text) => (text, region),
        None => country
            .split_once('-')
            .and_then(|(c, r)| Some((find_block(c)?, region.or(Some(r)))))
            .ok_or_else(|| {
                Error::new("unknown_country", format!("Unknown country: '{country}'."))
                    .hint("Use an ISO code such as DE or DEU, or run: date-calc countries")
            })?,
    };
    let block = Block::parse(text);
    let region = region.map(|r| block.region(r)).transpose()?;
    Ok((block, region))
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn find_block(country: &str) -> Option<&'static str> {
    let key = squash(country);
    blocks().find(|&b| {
        let c = Country::parse(b);
        let mut names = [c.code, c.name].into_iter().chain(c.aliases);
        !key.is_empty() && names.any(|n| squash(n) == key)
    })
}

pub(crate) fn weekend_days(mask: &str) -> u8 {
    mask.bytes().rev().fold(0, |m, b| m << 1 | (b - b'0'))
}

fn index(regions: &[Region], code: &str) -> Option<usize> {
    regions.iter().position(|r| r.code == code)
}

fn ymd(s: &str) -> Date {
    let n = |r: std::ops::Range<usize>| s[r].parse().expect("date in holiday data");
    jiff::civil::date(n(0..4), n(4..6) as i8, n(6..8) as i8)
}

impl Country {
    /// Reads the country line, the region lines after it and the first weekend line.
    fn parse(block: &'static str) -> Self {
        let mut lines = block.lines();
        let f: Vec<_> = lines.next().unwrap().split('\t').collect();
        let region_count = lines.by_ref().take_while(|l| l.starts_with("R\t")).count();
        let first_weekend = block.lines().nth(region_count + 1).unwrap();
        Self {
            code: f[0],
            name: f[1],
            aliases: f[2].split(',').filter(|a| !a.is_empty()).collect(),
            years: (f[3].parse().unwrap(), f[4].parse().unwrap()),
            decree_through: f[5].parse().ok(),
            region_count,
            weekend: weekend_days(first_weekend.split('\t').nth(3).unwrap()),
        }
    }
}

impl Block {
    fn parse(text: &'static str) -> Self {
        let mut block = Self {
            country: Country::parse(text),
            regions: vec![],
            holidays: vec![],
            weekends: vec![],
            workdays: vec![],
        };
        let mut names = vec![];
        let mut except = vec![];
        for line in text.lines().skip(1) {
            let f: Vec<_> = line.split('\t').collect();
            let region = |code| index(&block.regions, code);
            match f[0] {
                "R" => block.regions.push(Region {
                    code: f[1],
                    names: f[2..].to_vec(),
                }),
                "W" => block.weekends.push(Weekend {
                    region: region(f[1]),
                    from: f[2].parse().expect("date in holiday data"),
                    days: weekend_days(f[3]),
                }),
                "N" => names.push(f[2]),
                "H" => {
                    let flags = f.get(4).copied().unwrap_or("");
                    block.holidays.push(Holiday {
                        date: ymd(f[2]),
                        name: names[f[3].parse::<usize>().unwrap()],
                        observed: flags.contains('o'),
                        estimated: flags.contains('e'),
                        region: region(f[1]),
                        except: vec![],
                    })
                }
                "X" => except.push((
                    region(f[1]).unwrap(),
                    ymd(f[2]),
                    names[f[3].parse::<usize>().unwrap()],
                )),
                "K" => block.workdays.push((region(f[1]), ymd(f[2]))),
                _ => unreachable!("unknown line in holiday data: {line}"),
            }
        }
        block.holidays.sort_by_key(|h| h.date);
        block.workdays.sort_by_key(|w| w.1);
        for (region, date, name) in except {
            let i = block.holidays.partition_point(|h| h.date < date);
            block.holidays[i..]
                .iter_mut()
                .find(|h| h.region.is_none() && h.date == date && h.name == name)
                .expect("national holiday for an X line")
                .except
                .push(region);
        }
        block
    }

    /// A region by code or name, case-insensitive.
    pub fn region(&self, text: &str) -> Result<usize, Error> {
        let key = text.to_lowercase();
        let found = self.regions.iter().position(|r| {
            let mut names = std::iter::once(r.code).chain(r.names.iter().copied());
            names.any(|n| n.to_lowercase() == key)
        });
        let Country { code, name, .. } = self.country;
        found.ok_or_else(|| {
            let codes: Vec<_> = self.regions.iter().map(|r| r.code).collect();
            let hint = match codes.len() {
                0 => format!("{name} has no regions in the holiday data."),
                1..=12 => format!(
                    "Regions: {}. Run: date-calc countries {code}",
                    codes.join(", ")
                ),
                _ => format!(
                    "Regions: {}, … Run: date-calc countries {code}",
                    codes[..12].join(", ")
                ),
            };
            Error::new("unknown_region", format!("{name} has no region '{text}'.")).hint(hint)
        })
    }

    /// Whether the region (or the whole country) keeps this holiday.
    pub fn keeps(&self, holiday: &Holiday, region: Option<usize>) -> bool {
        match holiday.region {
            None => region.is_none_or(|r| !holiday.except.contains(&r)),
            r => r == region,
        }
    }

    /// The holidays kept from `from` to `to`, both included, in date order.
    pub fn holidays(
        &self,
        region: Option<usize>,
        from: Date,
        to: Date,
    ) -> impl Iterator<Item = &Holiday> {
        let start = self.holidays.partition_point(|h| h.date < from);
        self.holidays[start..]
            .iter()
            .take_while(move |h| h.date <= to)
            .filter(move |h| self.keeps(h, region))
    }

    /// The weekend days in force on a date: bit 0 is Monday, bit 6 Sunday.
    pub fn weekend(&self, region: Option<usize>, date: Date) -> u8 {
        let own = region.filter(|&r| self.weekends.iter().any(|w| w.region == Some(r)));
        let mut timeline = self.weekends.iter().filter(|w| w.region == own);
        let mut days = timeline.next().expect("a weekend line").days;
        for w in timeline.take_while(|w| w.from <= date) {
            days = w.days;
        }
        days
    }

    /// Whether a weekend day is worked by decree on this date.
    pub fn worked_by_decree(&self, region: Option<usize>, date: Date) -> bool {
        let i = self.workdays.partition_point(|w| w.1 < date);
        self.workdays[i..]
            .iter()
            .take_while(|w| w.1 == date)
            .any(|w| w.0.is_none() || w.0 == region)
    }

    /// Fails with `no_holiday_data` when the date is outside the country's data years.
    pub fn covers(&self, date: Date) -> Result<(), Error> {
        let (y0, y1) = self.country.years;
        if (y0..=y1).contains(&date.year()) {
            return Ok(());
        }
        Err(Error::new(
            "no_holiday_data",
            format!(
                "Holiday data for {} covers {y0}-{y1}; {date} is outside it.",
                self.country.name
            ),
        )
        .hint("Pick dates inside that range, or leave out --country to skip only weekends."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    const SAT_SUN: u8 = 0b110_0000;
    const FRI_SAT: u8 = 0b011_0000;

    fn names(b: &Block, region: Option<usize>, from: Date, to: Date) -> Vec<&'static str> {
        b.holidays(region, from, to).map(|h| h.name).collect()
    }

    fn region(b: &Block, code: &str) -> Option<usize> {
        index(&b.regions, code)
    }

    fn get(country: &str) -> Block {
        find(country, None).unwrap().0
    }

    fn found(country: &str, region: Option<&str>) -> (&'static str, Option<&'static str>) {
        let (b, r) = find(country, region).unwrap();
        (b.country.code, r.map(|r| b.regions[r].code))
    }

    #[test]
    fn finds_countries_by_code_alias_or_name() {
        for text in ["DE", "de", "DEU", "Germany", " germany "] {
            assert_eq!(found(text, None), ("DE", None), "{text}");
        }
        assert_eq!(found("UK", None).0, "GB");
        assert_eq!(found("united kingdom", None).0, "GB");
        assert_eq!(found("Korea", None).0, "KR");
        assert_eq!(found("Guinea-Bissau", None).0, "GW");
        assert_eq!(found("bosnia and herzegovina", None).0, "BA");
    }

    #[test]
    fn finds_regions_by_code_or_alias() {
        assert_eq!(found("DE-BY", None), ("DE", Some("BY")));
        assert_eq!(found("de-bayern", None), ("DE", Some("BY")));
        assert_eq!(found("DE", Some("BADEN-WÜRTTEMBERG")), ("DE", Some("BW")));
        assert_eq!(found("DE", Some("baden-württemberg")), ("DE", Some("BW")));
        assert_eq!(found("DE", Some("augsburg")), ("DE", Some("Augsburg")));
        assert_eq!(found("DE-BY", Some("BW")), ("DE", Some("BW")));
        assert_eq!(found("GB-ENG", None), ("GB", Some("ENG")));
    }

    #[test]
    fn unknown_country_and_region_errors() {
        for text in ["Atlantis", "XX-BY", ""] {
            let e = find(text, None).unwrap_err();
            assert_eq!(e.code, "unknown_country");
            assert_eq!(e.message, format!("Unknown country: '{text}'."));
            assert_eq!(
                e.hint.as_deref(),
                Some("Use an ISO code such as DE or DEU, or run: date-calc countries")
            );
        }
        let e = find("DE-ZZ", None).unwrap_err();
        assert_eq!(e.code, "unknown_region");
        assert_eq!(e.message, "Germany has no region 'ZZ'.");
        assert_eq!(
            e.hint.as_deref(),
            Some(
                "Regions: BB, BE, BW, BY, HB, HE, HH, MV, NI, NW, RP, SH, … Run: date-calc countries DE"
            )
        );
        let e = find("GB", Some("Kent")).unwrap_err();
        assert_eq!(
            e.hint.as_deref(),
            Some("Regions: ENG, NIR, SCT, WLS. Run: date-calc countries GB")
        );
        let e = find("CN", Some("BJ")).unwrap_err();
        assert_eq!(e.message, "China has no region 'BJ'.");
        assert_eq!(
            e.hint.as_deref(),
            Some("China has no regions in the holiday data.")
        );
    }

    #[test]
    fn lists_countries_without_parsing_holidays() {
        let all: Vec<_> = countries().collect();
        assert_eq!(all.len(), 250);
        let de = all.iter().find(|c| c.code == "DE").unwrap();
        assert_eq!(
            (de.region_count, de.weekend, de.years),
            (17, SAT_SUN, (2000, 2050))
        );
        let ae = all.iter().find(|c| c.code == "AE").unwrap();
        assert_eq!((ae.region_count, ae.weekend), (0, FRI_SAT));
        assert_eq!(ae.aliases, ["ARE"]);
        let gb = all.iter().find(|c| c.code == "GB").unwrap();
        assert_eq!(gb.aliases, ["GBR", "UK"]);
        assert_eq!(gb.decree_through, Some(2026));
    }

    #[test]
    fn every_block_parses() {
        assert_eq!(source(), ("0.107", "2026-10-19"));
        let blocks: Vec<_> = blocks().map(Block::parse).collect();
        assert_eq!(blocks.len(), 250);
        assert_eq!(blocks.iter().map(|b| b.regions.len()).sum::<usize>(), 626);
        assert_eq!(
            blocks
                .iter()
                .filter(|b| b.country.decree_through.is_some())
                .count(),
            34
        );
        for b in &blocks {
            assert_eq!(
                b.weekends[0].from,
                date(b.country.years.0, 1, 1),
                "{}",
                b.country.code
            );
        }
    }

    #[test]
    fn china_2026_holidays_and_decree_workdays() {
        let cn = get("CN");
        assert_eq!(cn.country.name, "China");
        assert_eq!(cn.country.aliases, ["CHN"]);
        assert_eq!(cn.country.decree_through, Some(2026));
        assert!(cn.regions.is_empty());
        let feb = names(&cn, None, date(2026, 2, 1), date(2026, 2, 28));
        assert_eq!(
            feb,
            [
                "Chinese New Year's Eve",
                "Chinese New Year (Spring Festival)",
                "Chinese New Year (Spring Festival)",
                "Chinese New Year (Spring Festival)",
                "Day off (substituted from 02/14/2026)",
                "Day off (substituted from 02/28/2026)",
            ]
        );
        assert_eq!(
            cn.holidays(None, date(2026, 1, 1), date(2026, 12, 31))
                .count(),
            22
        );
        let worked: Vec<_> = cn
            .workdays
            .iter()
            .filter(|w| w.1.year() == 2026)
            .map(|w| w.1)
            .collect();
        assert_eq!(
            worked,
            [
                date(2026, 1, 4),
                date(2026, 2, 14),
                date(2026, 2, 28),
                date(2026, 5, 9),
                date(2026, 9, 20),
                date(2026, 10, 10)
            ]
        );
        assert!(cn.worked_by_decree(None, date(2026, 2, 14)));
        assert!(!cn.worked_by_decree(None, date(2026, 2, 21)));
    }

    #[test]
    fn flags_observed_and_estimated_dates() {
        let us = get("US");
        let july: Vec<_> = us
            .holidays(None, date(2026, 7, 3), date(2026, 7, 4))
            .collect();
        assert_eq!((july[0].name, july[0].observed), ("Independence Day", true));
        assert_eq!(
            (july[1].name, july[1].observed),
            ("Independence Day", false)
        );

        let ae = get("AE");
        assert!(
            ae.holidays(None, date(2026, 1, 1), date(2026, 12, 31))
                .any(|h| h.estimated)
        );
        let sa = get("SA");
        assert!(
            !sa.holidays(None, date(2026, 1, 1), date(2026, 12, 31))
                .any(|h| h.estimated)
        );
    }

    #[test]
    fn regions_add_and_drop_national_holidays() {
        let de = get("DE");
        let by = region(&de, "BY");
        assert_eq!(de.regions[by.unwrap()].names, ["Bayern"]);
        let year = (date(2026, 1, 1), date(2026, 12, 31));
        assert_eq!(de.holidays(None, year.0, year.1).count(), 9);
        assert_eq!(de.holidays(by, year.0, year.1).count(), 12);
        assert_eq!(
            names(&de, by, date(2026, 1, 6), date(2026, 1, 6)),
            ["Epiphany"]
        );
        assert!(names(&de, None, date(2026, 1, 6), date(2026, 1, 6)).is_empty());

        // A national holiday a territory doesn't keep (an X line).
        let us = get("US");
        let national = names(&us, None, date(2026, 7, 4), date(2026, 7, 4));
        let dropped = us.holidays.iter().find(|h| !h.except.is_empty()).unwrap();
        assert!(!us.keeps(dropped, Some(dropped.except[0])));
        assert!(us.keeps(dropped, None));
        assert_eq!(national, ["Independence Day"]);
    }

    #[test]
    fn weekends_change_by_date_and_region() {
        let ae = get("AE");
        assert_eq!(ae.weekend(None, date(2021, 12, 31)), FRI_SAT);
        assert_eq!(ae.weekend(None, date(2022, 1, 1)), SAT_SUN);

        let my = get("MY");
        let johor = region(&my, "01");
        assert_eq!(my.weekend(None, date(2020, 6, 1)), SAT_SUN);
        assert_eq!(my.weekend(johor, date(2013, 12, 31)), SAT_SUN);
        assert_eq!(my.weekend(johor, date(2020, 6, 1)), FRI_SAT);
        assert_eq!(my.weekend(johor, date(2025, 1, 1)), SAT_SUN);
        assert_eq!(my.weekend(region(&my, "02"), date(2026, 1, 1)), FRI_SAT);
    }

    #[test]
    fn coverage_years_and_decree_schedules() {
        let lk = get("LK");
        assert_eq!(lk.country.years, (2003, 2026));
        assert_eq!(lk.covers(date(2026, 12, 31)), Ok(()));
        let e = lk.covers(date(2027, 1, 4)).unwrap_err();
        assert_eq!(e.code, "no_holiday_data");
        assert_eq!(
            e.message,
            "Holiday data for Sri Lanka covers 2003-2026; 2027-01-04 is outside it."
        );
        assert_eq!(
            e.hint.as_deref(),
            Some("Pick dates inside that range, or leave out --country to skip only weekends.")
        );
        assert_eq!(get("IN").country.years, (2001, 2035));
        assert_eq!(get("RU").country.decree_through, Some(2027));
        assert_eq!(get("DE").country.decree_through, None);
    }
}
