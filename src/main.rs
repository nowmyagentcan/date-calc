use std::ffi::OsString;
use std::io::{self, ErrorKind, IsTerminal, Write};
use std::process::ExitCode;
use std::sync::LazyLock;

use clap::{Args, Parser, Subcommand, ValueEnum};
use date_calc::days::Roll;
use date_calc::error::Error;
use date_calc::input::Unit;
use date_calc::report::{self, Op, Options};
use date_calc::text;

static VERSION: LazyLock<String> = LazyLock::new(|| {
    let (version, released) = date_calc::data::source();
    format!(
        "{} (python-holidays {version} {released}, tzdata {})",
        env!("CARGO_PKG_VERSION"),
        report::TZDATA
    )
});

const EXAMPLES: &str = "\x1b[1;4mExamples:\x1b[0m
  date-calc between 2026-02-01 2026-02-28 -c CN
  date-calc between today 2026-12-31 -c DE-BY --exclude-start
  date-calc add 2026-11-02 10 -c US
  date-calc add 2026-01-31 1 -u months --roll modified-following -c GB-ENG
  date-calc hours \"2026-11-02 10:00\" \"2026-11-05 15:00\" -c DE --hours 09:00-12:00,13:00-17:00
  date-calc holidays JP 2027
  date-calc convert 2026-10-27
  date-calc convert 1 Ramadan 1448
  date-calc countries DE";

#[derive(Parser)]
#[command(
    name = "date-calc",
    bin_name = "date-calc",
    about = text::SENTENCE,
    version = VERSION.as_str(),
    after_help = EXAMPLES,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Count business days between two dates
    #[command(after_help = EXAMPLES)]
    Between {
        /// First date: YYYY-MM-DD or today
        start: String,
        /// Last date: YYYY-MM-DD or today
        end: String,
        #[command(flatten)]
        place: Place,
        /// Don't count the start date
        #[arg(long)]
        exclude_start: bool,
        /// Don't count the end date
        #[arg(long)]
        exclude_end: bool,
        #[command(flatten)]
        out: Out,
    },
    /// Add or subtract business days, calendar units or business hours
    #[command(after_help = EXAMPLES, allow_negative_numbers = true)]
    Add {
        /// Start: a date, or a date-time for business hours
        start: String,
        /// Signed amount, e.g. 10 or -5 (business hours: 1.5)
        amount: String,
        /// What the amount counts
        #[arg(short, long, value_name = "UNIT", default_value = "business-days")]
        unit: UnitArg,
        /// Move a calendar-unit result that isn't a business day [default: none]
        #[arg(long, value_name = "R")]
        roll: Option<RollArg>,
        #[command(flatten)]
        place: Place,
        #[command(flatten)]
        clock: Clock,
        #[command(flatten)]
        out: Out,
    },
    /// Count business hours between two date-times
    #[command(after_help = EXAMPLES)]
    Hours {
        /// Start: YYYY-MM-DDTHH:MM, "YYYY-MM-DD HH:MM", an instant with Z or an offset, or now
        start: String,
        /// End, in the same forms
        end: String,
        #[command(flatten)]
        place: Place,
        #[command(flatten)]
        clock: Clock,
        #[command(flatten)]
        out: Out,
    },
    /// List a country's public holidays
    #[command(after_help = EXAMPLES)]
    Holidays {
        /// Country: ISO code, name, or CC-REGION like DE-BY
        country: String,
        /// Year [default: the current year]
        #[arg(value_parser = clap::value_parser!(i16).range(1..=9999))]
        year: Option<i16>,
        /// First date of a range, with --to
        #[arg(long, value_name = "D")]
        from: Option<String>,
        /// Last date of a range, with --from
        #[arg(long, value_name = "D")]
        to: Option<String>,
        /// Region of the country, e.g. BY [default: the whole country]
        #[arg(short, long, value_name = "R")]
        region: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Show a date in other calendars
    #[command(after_help = EXAMPLES)]
    Convert {
        /// YYYY-MM-DD (with --from, that calendar's month number; L marks a leap month), or D MonthName YYYY
        #[arg(required = true)]
        date: Vec<String>,
        /// Calendar of the date: gregorian, islamic-umalqura, islamic-civil, hebrew, chinese, persian, bikram-sambat [default: gregorian]
        #[arg(long, value_name = "CAL")]
        from: Option<String>,
        /// Show only this calendar [default: all]
        #[arg(long, value_name = "CAL")]
        to: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// List countries, their regions and weekends
    #[command(after_help = EXAMPLES)]
    Countries {
        /// One country, with its regions and weekend history
        country: Option<String>,
        #[command(flatten)]
        out: Out,
    },
}

#[derive(Args)]
struct Place {
    /// Country: ISO code, name, or CC-REGION like DE-BY [default: none, weekends only]
    #[arg(short, long, value_name = "C")]
    country: Option<String>,
    /// Region of the country, e.g. BY [default: the whole country]
    #[arg(short, long, value_name = "R", requires = "country")]
    region: Option<String>,
    /// Weekend: day names like SAT,SUN, a mask like 0000011, or none [default: the country's]
    #[arg(long, value_name = "W")]
    weekend: Option<String>,
}

#[derive(Args)]
struct Clock {
    /// Working hours, several separated by commas
    #[arg(long, value_name = "H", default_value = report::DEFAULT_HOURS)]
    hours: String,
    /// IANA time zone for the times [default: none, clock time]
    #[arg(long, value_name = "ZONE")]
    tz: Option<String>,
}

#[derive(Args)]
struct Out {
    /// Print one JSON object
    #[arg(long, display_order = 100)]
    json: bool,
    /// Show the cause of an error
    #[arg(long, display_order = 100)]
    debug: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum UnitArg {
    #[value(alias = "bd")]
    BusinessDays,
    #[value(alias = "d")]
    Days,
    #[value(alias = "w")]
    Weeks,
    Months,
    #[value(alias = "y")]
    Years,
    #[value(alias = "bh")]
    BusinessHours,
}

#[derive(Clone, Copy, ValueEnum)]
enum RollArg {
    None,
    Following,
    Preceding,
    ModifiedFollowing,
    ModifiedPreceding,
}

impl Command {
    /// The operation and its options.
    fn into_op(self) -> (Op, Options) {
        let mut o = Options::default();
        let mut set = |p: Place, clock: Option<Clock>| {
            (o.country, o.region, o.weekend) = (p.country, p.region, p.weekend);
            if let Some(c) = clock {
                (o.hours, o.timezone) = (Some(c.hours), c.tz);
            }
        };
        let op = match self {
            Command::Between {
                start,
                end,
                place,
                exclude_start,
                exclude_end,
                ..
            } => {
                set(place, None);
                (o.exclude_start, o.exclude_end) = (exclude_start, exclude_end);
                Op::Between { start, end }
            }
            Command::Add {
                start,
                amount,
                unit,
                roll,
                place,
                clock,
                ..
            } => {
                set(place, Some(clock));
                let unit = match unit {
                    UnitArg::BusinessDays => Unit::BusinessDays,
                    UnitArg::Days => Unit::Days,
                    UnitArg::Weeks => Unit::Weeks,
                    UnitArg::Months => Unit::Months,
                    UnitArg::Years => Unit::Years,
                    UnitArg::BusinessHours => Unit::BusinessHours,
                };
                let roll = match roll.unwrap_or(RollArg::None) {
                    RollArg::None => Roll::None,
                    RollArg::Following => Roll::Following,
                    RollArg::Preceding => Roll::Preceding,
                    RollArg::ModifiedFollowing => Roll::ModifiedFollowing,
                    RollArg::ModifiedPreceding => Roll::ModifiedPreceding,
                };
                Op::Add {
                    start,
                    amount,
                    unit,
                    roll,
                }
            }
            Command::Hours {
                start,
                end,
                place,
                clock,
                ..
            } => {
                set(place, Some(clock));
                Op::Hours { start, end }
            }
            Command::Holidays {
                country,
                year,
                from,
                to,
                region,
                ..
            } => {
                (o.country, o.region) = (Some(country), region);
                Op::Holidays { year, from, to }
            }
            Command::Convert { date, from, to, .. } => {
                let date = date.join(" ");
                Op::Convert { date, from, to }
            }
            Command::Countries { country, .. } => {
                o.country = country;
                Op::Countries
            }
        };
        (op, o)
    }
}

/// The limits clap can't state: argument length, and flags that belong to one command.
fn check(args: &[OsString]) -> Result<(), Error> {
    if args.iter().any(|a| a.len() > 256) {
        return Err(Error::new("usage", "An argument is longer than 256 bytes."));
    }
    let excluding = args
        .iter()
        .any(|a| a == "--exclude-start" || a == "--exclude-end");
    if excluding && args[1] != "between" {
        return Err(Error::new(
            "usage",
            "--exclude-start and --exclude-end work only with between.",
        ));
    }
    Ok(())
}

/// Parses the command line; with `--json`, clap's complaint comes back as the error object.
fn parse(args: &[OsString]) -> Result<(Op, Options), Error> {
    check(args)?;
    match Cli::try_parse_from(args) {
        Ok(cli) => Ok(cli.command.into_op()),
        Err(e) if e.use_stderr() && args.iter().any(|a| a == "--json") => {
            let text = e.render().to_string();
            let first = text.split("\n\n").next().unwrap_or_default();
            let message = first.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut error = Error::new("usage", message.trim_start_matches("error: "));
            error.hint = text.lines().rfind(|l| !l.is_empty()).map(String::from);
            Err(error)
        }
        Err(e) => e.exit(),
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let (json, debug) = (flag("--json"), flag("--debug"));
    let (out, code) = match parse(&args).and_then(|(op, o)| report::run(&op, &o)) {
        Ok(answer) if json => (json_line(&answer), 0),
        Ok(answer) => (people(text::answer(&answer), &io::stdout()), 0),
        Err(e) => {
            if debug {
                let _ = writeln!(anstream::stderr(), "debug: {e:?}");
            }
            if json {
                (
                    report::failure(&e)
                        + "
",
                    e.exit_code(),
                )
            } else {
                fail(&e);
                (String::new(), e.exit_code())
            }
        }
    };
    emit(&out, code, debug)
}

fn json_line(answer: &report::Answer) -> String {
    serde_json::to_string(answer).expect("answers are plain data")
        + "
"
}

/// Writes the answer to stdout; a closed stdout ends quietly.
fn emit(out: &str, code: u8, debug: bool) -> ExitCode {
    let mut stdout = anstream::stdout().lock();
    match stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::from(code),
        Err(e) if e.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            if debug {
                let _ = writeln!(anstream::stderr(), "debug: {e:?}");
            }
            fail(&Error::new("io", format!("Couldn't write output: {e}.")));
            ExitCode::from(3)
        }
    }
}

/// An error's two lines on stderr; a closed stderr loses them quietly.
fn fail(e: &Error) {
    let _ = write!(
        anstream::stderr(),
        "{}",
        people(text::error(e), &io::stderr())
    );
}

/// Text for a person, in ASCII when their console can't show Unicode.
fn people(text: String, stream: &impl IsTerminal) -> String {
    if text::unicode(stream.is_terminal()) {
        text
    } else {
        text::ascii(&text)
    }
}

#[cfg(test)]
mod tests {
    use jiff::{civil::date, tz::Offset};

    #[test]
    fn bundles_tzdata_2026f() {
        assert_eq!(jiff_tzdb::VERSION, Some("2026f"));
        // 2026f keeps Cuba on -04 after 2026-11-01; 2026e moved it to -05.
        let havana = date(2026, 11, 2)
            .at(0, 0, 0, 0)
            .in_tz("America/Havana")
            .unwrap();
        assert_eq!(havana.offset(), Offset::constant(-4));
    }

    #[test]
    fn checks_the_command_line() {
        use clap::CommandFactory;
        super::Cli::command().debug_assert();
    }
}
