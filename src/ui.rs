//! The local page on 127.0.0.1: `GET /` serves it and `POST /api` answers it.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::Error;
use crate::report::{self, Op, Options, failure};
use crate::{query, text};

const PAGE: &str = include_str!("ui.html");
/// The most a request may send in headers, and again in its body.
const LIMIT: u64 = 16 * 1024;
/// The page reaches nothing but its own server.
const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; connect-src 'self'; base-uri 'none'; form-action 'none'";

type Response = (u16, &'static str, String);

/// Serves on 127.0.0.1:`port` (0 picks a free port) until the process is stopped, and opens
/// the page in the default browser when `open` is set.
pub fn serve(port: u16, open: bool) -> Result<(), Error> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| {
        if e.kind() == io::ErrorKind::AddrInUse {
            Error::new("port_in_use", format!("Port {port} is in use."))
                .hint("Use --port 0 for a free port.")
        } else {
            Error::new("io", format!("Couldn't start the page: {e}."))
        }
    })?;
    let port = listener.local_addr().map_or(port, |a| a.port());
    let url = format!("http://127.0.0.1:{port}");
    // With stdout closed nobody learns the address, so end quietly as other commands do.
    match writeln!(io::stdout(), "Open {url}  (Ctrl+C to stop)") {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(()),
        Err(e) => return Err(Error::new("io", format!("Couldn't write output: {e}."))),
        Ok(()) => {}
    }
    if open {
        browse(url);
    }
    for stream in listener.incoming().flatten() {
        // A client that hangs up or stalls loses only its own connection.
        thread::spawn(move || handle(&stream, port));
    }
    Ok(())
}

/// Asks the OS to open `url`; when it can't, the printed address is enough.
fn browse(url: String) {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Some openers wait for the browser to close, so they run beside the server.
    thread::spawn(move || command.status());
}

/// One request, one response, then the connection closes.
fn handle(stream: &TcpStream, port: u16) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let (status, kind, body) = respond(&mut BufReader::new(stream.take(LIMIT)), port)?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Content Too Large",
        _ => "Unprocessable Content",
    };
    let mut out = stream;
    write!(
        out,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: {CSP}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.shutdown(Shutdown::Write)?;
    // Read what the client still sends, so closing doesn't reset the connection before it
    // has read the answer.
    io::copy(&mut stream.take(1 << 20), &mut io::sink()).map(drop)
}

fn respond(reader: &mut BufReader<io::Take<&TcpStream>>, port: u16) -> io::Result<Response> {
    // Lines are bytes: a header value may carry opaque ones (RFC 9110 §5.5).
    let (mut request, mut line) = (Vec::new(), Vec::new());
    reader.read_until(b'\n', &mut request)?;
    let (mut host, mut origin, mut length) = (None, None, None);
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(match reader.get_ref().limit() {
                0 => error(
                    413,
                    "too_large",
                    "The request headers are larger than 16 KB.",
                ),
                _ => error(400, "bad_request", "The request ended early."),
            });
        }
        if line.iter().all(|b| b"\r\n".contains(b)) {
            break;
        }
        let token = |n: &[u8]| {
            !n.is_empty()
                && n.iter()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(b))
        };
        // Field values are visible bytes, spaces and tabs (RFC 9110 §5.5), checked before the
        // spaces and tabs around them are trimmed, so no other control hides at an edge.
        let content = |v: &[u8]| v.iter().all(|&b| b == b'\t' || b >= b' ' && b != 0x7f);
        let line = line.strip_suffix(b"\n").unwrap_or(&line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line
            .iter()
            .position(|&b| b == b':')
            .filter(|&i| token(&line[..i]) && content(&line[i + 1..]))
        else {
            return Ok(error(400, "bad_request", "Can't read a request header."));
        };
        let field = match line[..colon].to_ascii_lowercase().as_slice() {
            b"host" => &mut host,
            b"origin" => &mut origin,
            b"content-length" => &mut length,
            // Bodies come only with a Content-Length; chunked ones aren't read.
            b"transfer-encoding" => {
                return Ok(error(
                    400,
                    "bad_request",
                    "Send the body with a Content-Length.",
                ));
            }
            _ => continue,
        };
        let value = String::from_utf8_lossy(line[colon + 1..].trim_ascii());
        // A second value could disagree with the first one checked (RFC 9112 §3.2, §6.3).
        if field.replace(value.into_owned()).is_some() {
            return Ok(error(400, "bad_request", "A request header is repeated."));
        }
    }
    // RFC 9112 §3: the request line is visible ASCII parted by spaces.
    let request = std::str::from_utf8(&request)
        .map(|r| r.trim_end_matches(['\r', '\n']))
        .ok()
        .filter(|r| r.bytes().all(|b| b == b' ' || b.is_ascii_graphic()));
    let mut parts = request.unwrap_or_default().split(' ');
    let (Some(method), Some(target), Some("HTTP/1.1" | "HTTP/1.0"), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Ok(error(400, "bad_request", "Can't read the request line."));
    };
    let local = |scheme: &str, value: &str| {
        ["127.0.0.1", "localhost"]
            .iter()
            .any(|h| value.eq_ignore_ascii_case(&format!("{scheme}{h}:{port}")))
    };
    if !host.is_some_and(|h| local("", &h)) || origin.is_some_and(|o| !local("http://", &o)) {
        return Ok(error(
            403,
            "forbidden",
            "Only this computer's page can use date-calc ui.",
        ));
    }
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    match (method, path) {
        ("GET", "/") => Ok((200, "text/html; charset=utf-8", PAGE.into())),
        ("POST", "/api") => {
            // Digits only: Rust would also read a leading `+`.
            let digits = |l: &String| l.bytes().all(|b| b.is_ascii_digit());
            let Some(length) = length.map_or(Some(0), |l| l.parse().ok().filter(|_| digits(&l)))
            else {
                return Ok(error(400, "bad_request", "Can't read the request length."));
            };
            if length > LIMIT {
                return Ok(error(413, "too_large", "The request is larger than 16 KB."));
            }
            let buffered = reader.buffer().len() as u64;
            reader.get_mut().set_limit(length.saturating_sub(buffered));
            let mut body = Vec::new();
            reader.take(length).read_to_end(&mut body)?;
            if (body.len() as u64) < length {
                return Ok(error(400, "bad_request", "The request ended early."));
            }
            Ok(match answer(&body) {
                Ok(answer) => (200, JSON, answer.to_string()),
                Err(e) if e.code == "bad_request" => (400, JSON, failure(&e)),
                Err(e) => (422, JSON, failure(&e)),
            })
        }
        _ => Ok(error(404, "not_found", "There's nothing here.")),
    }
}

/// What the page sends: a query line with the form's settings, or `"op": "countries"`.
#[derive(Deserialize)]
struct Request {
    op: Option<String>,
    query: Option<String>,
    country: Option<String>,
    region: Option<String>,
    weekend: Option<String>,
    hours: Option<String>,
    timezone: Option<String>,
    #[serde(default = "report::yes")]
    include_start: bool,
    #[serde(default = "report::yes")]
    include_end: bool,
}

/// `{"op", "result", "text"}` for a query (`text` is the CLI's text without colour), or
/// `{"op": "countries", "result"}`.
fn answer(body: &[u8]) -> Result<Value, Error> {
    let r: Request = serde_json::from_slice(body)
        .map_err(|e| Error::new("bad_request", format!("Can't read the request: {e}.")))?;
    let fields = [&r.country, &r.region, &r.weekend, &r.hours, &r.timezone];
    report::short(fields.into_iter().flatten().map(String::as_str))?;
    let o = Options {
        country: r.country,
        region: r.region,
        weekend: r.weekend,
        hours: r.hours,
        timezone: r.timezone,
        exclude_start: !r.include_start,
        exclude_end: !r.include_end,
    };
    let op = match (r.op.as_deref(), &r.query) {
        (Some("countries"), _) => Op::Countries,
        (None, Some(q)) => query::parse(q)?,
        _ => {
            return Err(Error::new(
                "bad_request",
                "Send a query, or \"op\": \"countries\".",
            ));
        }
    };
    let answer = report::run(&op, &o)?;
    let mut reply = json!({"op": answer.op(), "result": answer});
    if op != Op::Countries {
        let text = anstream::adapter::strip_str(&text::answer(&answer)).to_string();
        reply["text"] = text.into();
    }
    Ok(reply)
}

const JSON: &str = "application/json";

fn error(status: u16, code: &'static str, message: &str) -> Response {
    (status, JSON, failure(&Error::new(code, message)))
}
