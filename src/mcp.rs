//! The MCP server on stdio: one tool per operation, answering with the CLI's `--json` objects.

use std::sync::Arc;

use rmcp::handler::server::tool::{schema_for_input, schema_for_output};
use rmcp::model::{CallToolResult, ContentBlock, JsonObject};
use rmcp::service::ServerInitializeError;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Number, Value};

use crate::days::Roll;
use crate::error::Error;
use crate::input::Unit;
use crate::report::{
    self, Added, Between, Converted, Countries, Country, DEFAULT_HOURS, Holidays, Hours, Op,
    Options,
};

/// How every tool reports bad input, appended to each description.
macro_rules! on_error {
    () => {
        " On bad input the result has isError true and one text block holding {\"error\":{\"code\",\"message\",\"hint\"}}, hint null when there is none."
    };
}

/// Where business days are counted.
#[derive(Deserialize, JsonSchema)]
struct Place {
    /// ISO code (DE, DEU), English name, or CC-REGION like DE-BY; leave out to skip only weekends
    country: Option<String>,
    /// Region of the country, by code or name, e.g. BY
    region: Option<String>,
    /// Weekend instead of the country's: day names like FRI,SAT, a mask from Monday like 0000011, or none
    weekend: Option<String>,
}

/// Working hours and the business's time zone.
#[derive(Deserialize, JsonSchema)]
struct Clock {
    /// Working hours, several separated by commas, e.g. 09:00-12:00,13:00-17:00
    #[serde(default = "default_hours")]
    hours: String,
    /// IANA time zone, e.g. Europe/Berlin; required when start or end has a UTC offset; leave out for clock time
    timezone: Option<String>,
}

fn default_hours() -> String {
    DEFAULT_HOURS.into()
}

#[derive(Deserialize, JsonSchema)]
struct BetweenArgs {
    /// First date: YYYY-MM-DD or today
    start: String,
    /// Last date: YYYY-MM-DD or today
    end: String,
    #[serde(flatten)]
    place: Place,
    /// Count the start date
    #[serde(default = "report::yes")]
    include_start: bool,
    /// Count the end date
    #[serde(default = "report::yes")]
    include_end: bool,
}

#[derive(Deserialize, JsonSchema)]
struct AddArgs {
    /// A date (YYYY-MM-DD or today), or a date-time for business hours
    start: String,
    /// Signed amount, e.g. 10 or -5; business hours may have 2 decimals, like 1.5
    amount: Number,
    #[serde(default)]
    unit: Unit,
    #[serde(flatten)]
    place: Place,
    /// How to move a calendar-unit result that isn't a business day
    #[serde(default)]
    roll: Roll,
    #[serde(flatten)]
    clock: Clock,
}

#[derive(Deserialize, JsonSchema)]
struct HoursArgs {
    /// YYYY-MM-DDTHH:MM, an instant with Z or an offset, or now
    start: String,
    /// End, in the same forms
    end: String,
    #[serde(flatten)]
    place: Place,
    #[serde(flatten)]
    clock: Clock,
}

#[derive(Deserialize, JsonSchema)]
struct HolidaysArgs {
    /// ISO code (DE, DEU), English name, or CC-REGION like DE-BY
    country: String,
    /// Region of the country, by code or name, e.g. BY
    region: Option<String>,
    /// A year; leave out year, from and to for the current year
    #[schemars(range(min = 1, max = 9999))]
    year: Option<i16>,
    /// First date of a range (YYYY-MM-DD), with to
    from: Option<String>,
    /// Last date of a range (YYYY-MM-DD), with from
    to: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ConvertArgs {
    /// YYYY-MM-DD (with from, that calendar's month number; L marks a leap month, e.g. 5787-05L-01), or D MonthName YYYY like 1 Ramadan 1448
    date: String,
    /// Calendar of the date
    #[schemars(extend("enum" = CALENDARS, "default" = "gregorian"))]
    from: Option<String>,
    /// Show only this calendar; leave out for all
    #[schemars(extend("enum" = CALENDARS))]
    to: Option<String>,
}

const CALENDARS: [&str; 7] = [
    "gregorian",
    "islamic-umalqura",
    "islamic-civil",
    "hebrew",
    "chinese",
    "persian",
    "bikram-sambat",
];

#[derive(Deserialize, JsonSchema)]
struct CountriesArgs {
    /// One country (ISO code or name) for its regions and weekend history; leave out for all
    country: Option<String>,
}

/// Every country, or one with its regions.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum CountriesAnswer {
    All(Countries),
    One(Country),
}

/// An amount as the CLI reads it: `5.0` is the whole number 5.
fn amount(n: &Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e15 => (f as i64).to_string(),
        _ => n.to_string(),
    }
}

fn place(p: Place, o: &mut Options) {
    (o.country, o.region, o.weekend) = (p.country, p.region, p.weekend);
}

fn clock(c: Clock, o: &mut Options) {
    (o.hours, o.timezone) = (Some(c.hours), c.timezone);
}

#[derive(Clone)]
struct Server;

#[tool_router]
impl Server {
    #[tool(
        description = concat!("Count business days between two dates in a country and region, skipping weekends and official public holidays and counting weekend days made workdays by decree. Both dates count by default, like Excel NETWORKDAYS; pass the same date twice to ask whether one day is a business day. Returns the signed count, the holidays and worked weekend days in range, elapsed calendar days and the data version.", on_error!()),
        input_schema = input::<BetweenArgs>(),
        output_schema = schema_for_output::<Between>(),
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn business_days_between(&self, args: JsonObject) -> CallToolResult {
        call(args, |a: BetweenArgs, o| {
            place(a.place, o);
            (o.exclude_start, o.exclude_end) = (!a.include_start, !a.include_end);
            Op::Between {
                start: a.start,
                end: a.end,
            }
        })
    }

    #[tool(
        description = concat!("Add or subtract business days (default, like Excel WORKDAY: the start is not counted), calendar days, weeks, months, years or business hours, using a country's weekends and public holidays. Returns the result and the non-business days skipped.", on_error!()),
        input_schema = input::<AddArgs>(),
        output_schema = schema_for_output::<Added>(),
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn add_to_date(&self, args: JsonObject) -> CallToolResult {
        call(args, |a: AddArgs, o| {
            place(a.place, o);
            clock(a.clock, o);
            Op::Add {
                start: a.start,
                amount: amount(&a.amount),
                unit: a.unit,
                roll: a.roll,
            }
        })
    }

    #[tool(
        description = concat!("Count working time between two date-times on a country's business days within working hours (default 09:00-17:00, several windows allowed), in a time zone when given. Returns seconds, hours and the holidays in range.", on_error!()),
        input_schema = input::<HoursArgs>(),
        output_schema = schema_for_output::<Hours>(),
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn business_hours_between(&self, args: JsonObject) -> CallToolResult {
        call(args, |a: HoursArgs, o| {
            place(a.place, o);
            clock(a.clock, o);
            Op::Hours {
                start: a.start,
                end: a.end,
            }
        })
    }

    #[tool(
        description = concat!("List official public holidays for a country, or one region, for a year or a date range, with holidays only some regions keep and weekend days worked by decree. Flags observed (substitute) and estimated dates.", on_error!()),
        input_schema = input::<HolidaysArgs>(),
        output_schema = schema_for_output::<Holidays>(),
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn list_holidays(&self, args: JsonObject) -> CallToolResult {
        call(args, |a: HolidaysArgs, o| {
            (o.country, o.region) = (Some(a.country), a.region);
            Op::Holidays {
                year: a.year,
                from: a.from,
                to: a.to,
            }
        })
    }

    #[tool(
        description = concat!("Convert a date between Gregorian, Islamic Umm al-Qura, tabular Islamic, Hebrew, Chinese lunar, Persian solar and Nepali Bikram Sambat calendars. Returns the date in every calendar (or one), with month names and notes on observation-based dates.", on_error!()),
        input_schema = input::<ConvertArgs>(),
        output_schema = schema_for_output::<Converted>(),
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn convert_date(&self, args: JsonObject) -> CallToolResult {
        call(args, |a: ConvertArgs, _| Op::Convert {
            date: a.date,
            from: a.from,
            to: a.to,
        })
    }

    #[tool(
        description = concat!("List supported countries with their codes, regions, weekends, data years and the last year of decree-based schedules, or one country's regions and weekend history.", on_error!()),
        input_schema = input::<CountriesArgs>(),
        output_schema = output::<CountriesAnswer>(),
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn list_countries(&self, args: JsonObject) -> CallToolResult {
        call(args, |a: CountriesArgs, o| {
            o.country = a.country;
            Op::Countries
        })
    }
}

#[tool_handler(
    name = "date-calc",
    instructions = "Count business days and hours with official public holidays (including weekend days a government made workdays, as in China), add or subtract working days, list a country's holidays, and convert dates between Gregorian, Islamic, Hebrew, Chinese, Persian and Nepali calendars. Use these tools instead of computing dates yourself."
)]
impl ServerHandler for Server {}

fn input<T: JsonSchema + 'static>() -> Arc<JsonObject> {
    schema_for_input::<T>().expect("tool arguments are objects")
}

/// An output schema with an object at its root, as 2025-11-25 clients require.
fn output<T: JsonSchema + 'static>() -> Arc<JsonObject> {
    let mut schema = (*schema_for_output::<T>()).clone();
    schema.insert("type".into(), "object".into());
    Arc::new(schema)
}

/// The answer as structured content, or the error object as an error result. Errors carry
/// no structured content, so clients don't check them against the output schema.
fn call<T: DeserializeOwned>(
    args: JsonObject,
    op: impl FnOnce(T, &mut Options) -> Op,
) -> CallToolResult {
    let answer = serde_json::from_value(Value::Object(args))
        .map_err(|e| Error::new("bad_request", format!("Can't read the arguments: {e}.")))
        .and_then(|args| {
            let mut o = Options::default();
            let op = op(args, &mut o);
            report::run(&op, &o)
        });
    match answer {
        Ok(a) => {
            CallToolResult::structured(serde_json::to_value(a).expect("answers are plain data"))
        }
        Err(e) => CallToolResult::error(vec![ContentBlock::text(report::failure(&e))]),
    }
}

/// Serves until the client closes stdin.
pub fn serve() -> Result<(), Error> {
    let io = |e: &dyn std::fmt::Display| Error::new("io", format!("Couldn't read input: {e}."));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| io(&e))?;
    runtime.block_on(async {
        match Server.serve(rmcp::transport::stdio()).await {
            Ok(service) => service.waiting().await.map(drop).map_err(|e| io(&e)),
            // stdin closed before the handshake ended: the client left, which is no failure.
            Err(ServerInitializeError::ConnectionClosed(_)) => Ok(()),
            // stdout closed before the answer went out: the client left too.
            Err(ServerInitializeError::TransportError { error, .. })
                if error
                    .error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe) =>
            {
                Ok(())
            }
            Err(e) => Err(io(&e)),
        }
    })
}
