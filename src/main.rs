use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::LazyLock;

use clap::{Args, Parser, Subcommand, ValueEnum};
use date_calc::days::Roll;
use date_calc::error::Error;
use date_calc::input::Unit;
use date_calc::report::{self, Op, Options};

const SENTENCE: &str = "Count business days, add workdays, list holidays, convert dates.";

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
  date-calc hours \"2026-11-02 10:00\" \"2026-11-05 15:00\" -c DE --hours 09:00-12:00,13:00-17:00";

#[derive(Parser)]
#[command(
    name = "date-calc",
    bin_name = "date-calc",
    about = SENTENCE,
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
    /// The operation, its options, and whether to print JSON.
    fn into_op(self) -> (Op, Options, bool) {
        let mut o = Options::default();
        let mut set = |p: Place, clock: Option<Clock>| {
            (o.country, o.region, o.weekend) = (p.country, p.region, p.weekend);
            if let Some(c) = clock {
                (o.hours, o.timezone) = (Some(c.hours), c.tz);
            }
        };
        let (op, json) = match self {
            Command::Between {
                start,
                end,
                place,
                exclude_start,
                exclude_end,
                out,
            } => {
                set(place, None);
                (o.exclude_start, o.exclude_end) = (exclude_start, exclude_end);
                (Op::Between { start, end }, out.json)
            }
            Command::Add {
                start,
                amount,
                unit,
                roll,
                place,
                clock,
                out,
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
                let op = Op::Add {
                    start,
                    amount,
                    unit,
                    roll,
                };
                (op, out.json)
            }
            Command::Hours {
                start,
                end,
                place,
                clock,
                out,
            } => {
                set(place, Some(clock));
                (Op::Hours { start, end }, out.json)
            }
        };
        (op, o, json)
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
fn parse(args: &[OsString]) -> Result<(Op, Options, bool), Error> {
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
    let json = args.iter().any(|a| a == "--json");
    let answer = parse(&args).and_then(|(op, o, _)| report::run(&op, &o));
    match answer {
        Ok(answer) => {
            println!(
                "{}",
                serde_json::to_string(&answer).expect("answers are plain data")
            );
            ExitCode::SUCCESS
        }
        Err(e) if json => {
            println!("{}", report::failure(&e));
            ExitCode::from(e.exit_code())
        }
        Err(e) => {
            eprintln!("✗ {}", e.message);
            if let Some(hint) = &e.hint {
                eprintln!("  {hint}");
            }
            ExitCode::from(e.exit_code())
        }
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
