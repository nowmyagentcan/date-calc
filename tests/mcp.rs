//! Talks to `date-calc mcp` over stdio the way MCP clients do.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_date-calc");

/// Sends each message, closes stdin, and returns the responses by id and the exit code.
fn session(messages: &[Value]) -> (Vec<Value>, i32) {
    let mut child = Command::new(BIN)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for m in messages {
        writeln!(stdin, "{m}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut responses: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    responses.sort_by_key(|r| r["id"].as_u64());
    (responses, out.status.code().unwrap())
}

/// The CLI's `--json` line for the same input.
fn cli(args: &[&str]) -> String {
    let out = Command::new(BIN).args(args).arg("--json").output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim_end().into()
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn call(id: u64, tool: &str, arguments: Value) -> Value {
    request(
        id,
        "tools/call",
        json!({"name": tool, "arguments": arguments}),
    )
}

const INSTRUCTIONS: &str = "Count business days and hours with official public holidays (including weekend days a government made workdays, as in China), add or subtract working days, list a country's holidays, and convert dates between Gregorian, Islamic, Hebrew, Chinese, Persian and Nepali calendars. Use these tools instead of computing dates yourself.";

const ON_ERROR: &str = " On bad input the result has isError true and one text block holding {\"error\":{\"code\",\"message\",\"hint\"}}, hint null when there is none.";

/// The six tools with the documented descriptions, arguments and hints.
fn check_tools(list: &Value) {
    let tools = list["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap();
    assert_eq!(tools.len(), 6);
    let place = ["country", "region", "weekend"];
    let clock = ["hours", "timezone"];
    for (name, description, required, properties) in [
        (
            "business_days_between",
            "Count business days between two dates in a country and region, skipping weekends and official public holidays and counting weekend days made workdays by decree. Both dates count by default, like Excel NETWORKDAYS; pass the same date twice to ask whether one day is a business day. Returns the signed count, the holidays and worked weekend days in range, elapsed calendar days and the data version.",
            &["start", "end"][..],
            [
                &["start", "end", "include_start", "include_end"][..],
                &place,
            ]
            .concat(),
        ),
        (
            "add_to_date",
            "Add or subtract business days (default, like Excel WORKDAY: the start is not counted), calendar days, weeks, months, years or business hours, using a country's weekends and public holidays. Returns the result and the non-business days skipped.",
            &["start", "amount"],
            [&["start", "amount", "unit", "roll"][..], &place, &clock].concat(),
        ),
        (
            "business_hours_between",
            "Count working time between two date-times on a country's business days within working hours (default 09:00-17:00, several windows allowed), in a time zone when given. Returns seconds, hours and the holidays in range.",
            &["start", "end"],
            [&["start", "end"][..], &place, &clock].concat(),
        ),
        (
            "list_holidays",
            "List official public holidays for a country, or one region, for a year or a date range, with holidays only some regions keep and weekend days worked by decree. Flags observed (substitute) and estimated dates.",
            &["country"],
            vec!["country", "region", "year", "from", "to"],
        ),
        (
            "convert_date",
            "Convert a date between Gregorian, Islamic Umm al-Qura, tabular Islamic, Hebrew, Chinese lunar, Persian solar and Nepali Bikram Sambat calendars. Returns the date in every calendar (or one), with month names and notes on observation-based dates.",
            &["date"],
            vec!["date", "from", "to"],
        ),
        (
            "list_countries",
            "List supported countries with their codes, regions, weekends, data years and the last year of decree-based schedules, or one country's regions and weekend history.",
            &[],
            vec!["country"],
        ),
    ] {
        let t = tool(name);
        assert_eq!(
            t["description"],
            format!("{description}{ON_ERROR}"),
            "{name}"
        );
        assert_eq!(
            t["annotations"],
            json!({"readOnlyHint": true, "idempotentHint": true, "openWorldHint": false})
        );
        let input = &t["inputSchema"];
        assert_eq!(
            input["required"].as_array().map_or(0, Vec::len),
            required.len(),
            "{name}"
        );
        for r in required {
            assert!(input["required"].as_array().unwrap().contains(&json!(r)));
        }
        let mut names: Vec<_> = input["properties"].as_object().unwrap().keys().collect();
        let mut want = properties.clone();
        names.sort();
        want.sort();
        assert_eq!(names, want, "{name}");
        assert_eq!(t["outputSchema"]["type"], "object", "{name}");
    }
    let add = &tool("add_to_date")["inputSchema"];
    assert_eq!(add["properties"]["amount"]["type"], "number");
    assert_eq!(add["properties"]["unit"]["default"], "business_days");
    assert_eq!(
        add["$defs"]["Unit"]["enum"],
        json!([
            "business_days",
            "days",
            "weeks",
            "months",
            "years",
            "business_hours"
        ])
    );
    assert_eq!(add["properties"]["roll"]["default"], "none");
    assert_eq!(add["properties"]["hours"]["default"], "09:00-17:00");
    let between = &tool("business_days_between")["inputSchema"]["properties"];
    assert_eq!(between["include_start"]["default"], true);
    let convert = &tool("convert_date")["inputSchema"]["properties"];
    assert_eq!(convert["from"]["default"], "gregorian");
    assert_eq!(convert["to"]["enum"].as_array().unwrap().len(), 7);
    let between = &tool("business_days_between")["outputSchema"];
    assert!(
        between["required"]
            .as_array()
            .unwrap()
            .contains(&json!("weekend_workdays"))
    );
    assert_eq!(
        tool("list_countries")["outputSchema"]["anyOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

/// A successful call: structured content and text both equal to the CLI's JSON line.
fn check_answer(response: &Value, cli_args: &[&str]) {
    let result = &response["result"];
    assert_eq!(result["isError"], false, "{cli_args:?}: {result}");
    let text = result["content"][0]["text"].as_str().unwrap();
    let want = cli(cli_args);
    assert_eq!(text, want, "{cli_args:?}");
    assert_eq!(
        result["structuredContent"],
        serde_json::from_str::<Value>(&want).unwrap()
    );
}

/// A failed call: an error result whose text is the error object, the CLI's when given.
fn check_error(response: &Value, code: &str, cli_args: Option<&[&str]>) {
    let result = &response["result"];
    assert_eq!(result["isError"], true, "{result}");
    assert!(result.get("structuredContent").is_none());
    assert_eq!(result["content"].as_array().unwrap().len(), 1);
    let text = result["content"][0]["text"].as_str().unwrap();
    let error: Value = serde_json::from_str(text).unwrap();
    assert_eq!(error["error"]["code"], code, "{text}");
    if let Some(args) = cli_args {
        assert_eq!(text, cli(args));
    }
}

#[test]
fn answers_after_an_initialize_handshake() {
    let calls = [
        (
            "business_days_between",
            json!({"start": "2026-02-01", "end": "2026-02-28", "country": "CN"}),
            &["between", "2026-02-01", "2026-02-28", "-c", "CN"][..],
        ),
        (
            "business_days_between",
            json!({"start": "2026-01-01", "end": "2026-01-31", "country": "DE", "region": "BY", "include_start": false}),
            &[
                "between",
                "2026-01-01",
                "2026-01-31",
                "-c",
                "DE",
                "-r",
                "BY",
                "--exclude-start",
            ],
        ),
        (
            "business_days_between",
            json!({"start": "2026-11-02", "end": "2026-11-06", "weekend": "FRI,SAT", "include_end": false}),
            &[
                "between",
                "2026-11-02",
                "2026-11-06",
                "--weekend",
                "FRI,SAT",
                "--exclude-end",
            ],
        ),
        (
            "add_to_date",
            json!({"start": "2026-02-13", "amount": 5, "country": "CN"}),
            &["add", "2026-02-13", "5", "-c", "CN"],
        ),
        (
            "add_to_date",
            json!({"start": "2026-03-02", "amount": -5.0, "country": "CN"}),
            &["add", "2026-03-02", "-5", "-c", "CN"],
        ),
        (
            "add_to_date",
            json!({"start": "2026-01-31", "amount": 1, "unit": "months", "roll": "modified_following", "country": "DE"}),
            &[
                "add",
                "2026-01-31",
                "1",
                "-u",
                "months",
                "--roll",
                "modified-following",
                "-c",
                "DE",
            ],
        ),
        (
            "add_to_date",
            json!({"start": "2026-11-02 15:00", "amount": 1.5, "unit": "business_hours", "country": "DE", "hours": "09:00-12:00,13:00-17:00", "timezone": "Europe/Berlin"}),
            &[
                "add",
                "2026-11-02 15:00",
                "1.5",
                "-u",
                "business-hours",
                "-c",
                "DE",
                "--hours",
                "09:00-12:00,13:00-17:00",
                "--tz",
                "Europe/Berlin",
            ],
        ),
        (
            "business_hours_between",
            json!({"start": "2026-10-30T16:00:00Z", "end": "2026-11-02T18:00:00Z", "country": "CA", "region": "BC", "timezone": "America/Vancouver"}),
            &[
                "hours",
                "2026-10-30T16:00:00Z",
                "2026-11-02T18:00:00Z",
                "-c",
                "CA",
                "-r",
                "BC",
                "--tz",
                "America/Vancouver",
            ],
        ),
        (
            "business_hours_between",
            json!({"start": "2026-11-02T10:00", "end": "2026-11-05T15:00", "country": "DE"}),
            &["hours", "2026-11-02T10:00", "2026-11-05T15:00", "-c", "DE"],
        ),
        (
            "list_holidays",
            json!({"country": "DE", "year": 2026}),
            &["holidays", "DE", "2026"],
        ),
        (
            "list_holidays",
            json!({"country": "DE", "region": "BY", "from": "2026-01-01", "to": "2026-03-31"}),
            &[
                "holidays",
                "DE",
                "-r",
                "BY",
                "--from",
                "2026-01-01",
                "--to",
                "2026-03-31",
            ],
        ),
        (
            "convert_date",
            json!({"date": "2026-10-27"}),
            &["convert", "2026-10-27"],
        ),
        (
            "convert_date",
            json!({"date": "5787-05L-01", "from": "hebrew", "to": "persian"}),
            &[
                "convert",
                "5787-05L-01",
                "--from",
                "hebrew",
                "--to",
                "persian",
            ],
        ),
        ("list_countries", json!({}), &["countries"]),
        (
            "list_countries",
            json!({"country": "DE"}),
            &["countries", "DE"],
        ),
    ];
    let errors = [
        (
            "business_days_between",
            json!({"start": "2027-01-04", "end": "2027-01-05", "country": "LK"}),
            "no_holiday_data",
            Some(&["between", "2027-01-04", "2027-01-05", "-c", "LK"][..]),
        ),
        (
            "business_hours_between",
            json!({"start": "2026-10-30T16:00:00Z", "end": "2026-11-02T18:00:00Z"}),
            "tz_required",
            Some(&["hours", "2026-10-30T16:00:00Z", "2026-11-02T18:00:00Z"]),
        ),
        (
            "add_to_date",
            json!({"start": "2026-01-01", "amount": 1, "roll": "following"}),
            "usage",
            Some(&["add", "2026-01-01", "1", "--roll", "following"]),
        ),
        (
            "add_to_date",
            json!({"start": "2026-01-01", "amount": 2000000}),
            "usage",
            None,
        ),
        (
            "convert_date",
            json!({"date": "30 Safar 1447"}),
            "bad_calendar_date",
            Some(&["convert", "30 Safar 1447"]),
        ),
        (
            "convert_date",
            json!({"date": "2026-10-27", "from": "julian"}),
            "unknown_calendar",
            Some(&["convert", "2026-10-27", "--from", "julian"]),
        ),
        (
            "list_holidays",
            json!({"country": "XX"}),
            "unknown_country",
            Some(&["holidays", "XX"]),
        ),
        (
            "list_holidays",
            json!({"country": "DE", "year": 0}),
            "date_out_of_range",
            None,
        ),
        (
            "list_holidays",
            json!({"country": "DE", "year": 2026, "from": "2026-01-01"}),
            "usage",
            None,
        ),
        (
            "list_countries",
            json!({"country": "Atlantis"}),
            "unknown_country",
            Some(&["countries", "Atlantis"]),
        ),
        (
            "add_to_date",
            json!({"start": "2026-01-01"}),
            "bad_request",
            None,
        ),
        (
            "add_to_date",
            json!({"start": "2026-01-01", "amount": "5"}),
            "bad_request",
            None,
        ),
        (
            "add_to_date",
            json!({"start": "2026-01-01", "amount": 5, "unit": "fortnights"}),
            "bad_request",
            None,
        ),
        (
            "business_days_between",
            json!({"start": "2026-01-01"}),
            "bad_request",
            None,
        ),
    ];
    let mut messages = vec![
        request(
            1,
            "initialize",
            json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "test", "version": "1.0.0"}}),
        ),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        request(2, "tools/list", json!({})),
    ];
    let mut id = 2;
    for (tool, args, _) in &calls {
        id += 1;
        messages.push(call(id, tool, args.clone()));
    }
    for (tool, args, _, _) in &errors {
        id += 1;
        messages.push(call(id, tool, args.clone()));
    }
    let (responses, code) = session(&messages);
    assert_eq!(code, 0);
    assert_eq!(responses.len() as u64, id);
    let init = &responses[0]["result"];
    assert_eq!(init["protocolVersion"], "2025-11-25");
    assert_eq!(
        init["serverInfo"],
        json!({"name": "date-calc", "version": "0.1.0"})
    );
    assert_eq!(init["instructions"], INSTRUCTIONS);
    assert_eq!(init["capabilities"]["tools"], json!({}));
    check_tools(&responses[1]);
    for (response, (_, _, args)) in responses[2..].iter().zip(&calls) {
        check_answer(response, args);
    }
    for (response, (_, _, code, args)) in responses[2 + calls.len()..].iter().zip(&errors) {
        check_error(response, code, *args);
    }
}

#[test]
fn answers_after_server_discover() {
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "1.0.0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let between = json!({"_meta": meta, "name": "business_days_between", "arguments": {"start": "2026-02-01", "end": "2026-02-28", "country": "CN"}});
    let (responses, code) = session(&[
        request(1, "server/discover", json!({"_meta": meta})),
        request(2, "tools/list", json!({"_meta": meta})),
        request(3, "tools/call", between),
    ]);
    assert_eq!(code, 0);
    let discover = &responses[0]["result"];
    let versions = discover["supportedVersions"].as_array().unwrap();
    assert!(versions.contains(&json!("2026-07-28")) && versions.contains(&json!("2025-11-25")));
    assert_eq!(
        discover["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "date-calc"
    );
    assert_eq!(discover["instructions"], INSTRUCTIONS);
    check_tools(&responses[1]);
    check_answer(
        &responses[2],
        &["between", "2026-02-01", "2026-02-28", "-c", "CN"],
    );
    assert_eq!(
        responses[2]["result"]["structuredContent"]["business_days"],
        16
    );
}

#[test]
fn exits_quietly_when_stdin_closes() {
    assert_eq!(session(&[]), (vec![], 0));
}

/// An initialize request (id 0) and the initialized notification.
fn handshake() -> Vec<Value> {
    let client = json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "test", "version": "1.0.0"}});
    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    vec![request(0, "initialize", client), initialized]
}

/// Where `value` breaks `schema`, in the subset of JSON Schema the tools publish: `$ref` into
/// `$defs`, `anyOf`, `type`, `required`, `properties` and `items`.
fn misfit(value: &Value, schema: &Value, root: &Value, path: &str) -> Option<String> {
    if let Some(name) = schema["$ref"].as_str() {
        let name = name.strip_prefix("#/$defs/").unwrap();
        return misfit(value, &root["$defs"][name], root, path);
    }
    if let Some(any) = schema["anyOf"].as_array() {
        let fits = any.iter().any(|s| misfit(value, s, root, path).is_none());
        return (!fits).then(|| format!("{path}: fits no anyOf branch"));
    }
    let kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    let types = match &schema["type"] {
        Value::Array(t) => t.iter().map(|t| t.as_str().unwrap()).collect(),
        t => t.as_str().into_iter().collect::<Vec<_>>(),
    };
    let numeric = kind == "integer" && types.contains(&"number");
    if !types.is_empty() && !types.contains(&kind) && !numeric {
        return Some(format!("{path}: {kind}, not {types:?}"));
    }
    for key in schema["required"].as_array().into_iter().flatten() {
        let key = key.as_str().unwrap();
        if value.get(key).is_none() {
            return Some(format!("{path}: no {key}"));
        }
    }
    let props = schema["properties"].as_object();
    let fields = value.as_object().into_iter().flatten();
    let fields = fields.filter_map(|(k, v)| Some((format!("{path}.{k}"), v, props?.get(k)?)));
    let items = value.as_array().into_iter().flatten().enumerate();
    let items = items.map(|(i, v)| (format!("{path}[{i}]"), v, &schema["items"]));
    fields
        .chain(items)
        .find_map(|(p, v, s)| misfit(v, s, root, &p))
}

#[test]
fn answers_fit_their_output_schemas() {
    let calls = [
        (
            "business_days_between",
            json!({"start": "2026-02-01", "end": "2026-02-28", "country": "CN"}),
        ),
        (
            "add_to_date",
            json!({"start": "2026-02-13", "amount": 5, "country": "CN"}),
        ),
        (
            "add_to_date",
            json!({"start": "2026-11-02", "amount": 1000, "country": "US"}),
        ),
        (
            "add_to_date",
            json!({"start": "2026-11-02T16:00", "amount": 1.5, "unit": "business_hours"}),
        ),
        (
            "business_hours_between",
            json!({"start": "2026-12-20T10:00", "end": "2026-12-30T15:00", "country": "DE"}),
        ),
        ("list_holidays", json!({"country": "DE", "year": 2026})),
        (
            "list_holidays",
            json!({"country": "DE", "region": "BY", "year": 2026}),
        ),
        ("convert_date", json!({"date": "2026-10-27"})),
        ("list_countries", json!({})),
        ("list_countries", json!({"country": "DE"})),
    ];
    let mut messages = handshake();
    messages.push(request(1, "tools/list", json!({})));
    messages.extend(
        (0..)
            .zip(&calls)
            .map(|(i, (tool, args))| call(2 + i, tool, args.clone())),
    );
    let (responses, _) = session(&messages);
    let responses = &responses[1..];
    let tools = responses[0]["result"]["tools"].as_array().unwrap();
    for ((tool, args), response) in calls.iter().zip(&responses[1..]) {
        let schema = &tools.iter().find(|t| t["name"] == *tool).unwrap()["outputSchema"];
        let answer = &response["result"]["structuredContent"];
        assert!(answer.is_object(), "{tool} {args}: {response}");
        assert_eq!(misfit(answer, schema, schema, "$"), None, "{tool} {args}");
    }
}

#[test]
fn refuses_a_region_without_a_country() {
    let args = json!({"start": "2026-01-06", "end": "2026-01-06", "region": "BY"});
    let mut messages = handshake();
    messages.push(call(1, "business_days_between", args));
    let (responses, _) = session(&messages);
    let result = &responses[1]["result"];
    assert_eq!(result["isError"], true, "{result}");
    let error: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error["error"]["code"], "country_required", "{error}");
}

#[test]
fn answers_each_of_many_outstanding_calls_under_its_own_id() {
    let mut messages = handshake();
    messages.extend((1..=64).map(|i| {
        call(
            i,
            "add_to_date",
            json!({"start": "2026-01-01", "amount": i, "unit": "days"}),
        )
    }));
    let (responses, _) = session(&messages);
    assert_eq!(responses.len(), 65);
    for (i, response) in (0..).zip(&responses) {
        assert_eq!(response["id"], i);
        if i > 0 {
            let want = jiff::civil::date(2026, 1, 1) + jiff::Span::new().days(i);
            let result = &response["result"]["structuredContent"]["result"];
            assert_eq!(result, &json!(want.to_string()), "{response}");
        }
    }
}
