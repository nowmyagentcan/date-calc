//! The local page: the HTML itself, and `date-calc ui` answering over HTTP on 127.0.0.1.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

const PAGE: &str = include_str!("../src/ui.html");
const BIN: &str = env!("CARGO_BIN_EXE_date-calc");

#[test]
fn page_reaches_no_other_host() {
    let allowed = [
        "https://nowmyagentcan.com",
        "https://github.com/nowmyagentcan/date-calc",
        "https://x.com/chayp05",
        "http://www.w3.org/2000/svg",
    ];
    for (i, _) in PAGE.match_indices("://") {
        let start = PAGE[..i]
            .rfind(|c: char| !c.is_ascii_alphanumeric())
            .map_or(0, |p| p + 1);
        let url = &PAGE[start..];
        assert!(
            allowed.iter().any(|a| url.starts_with(a)),
            "unexpected URL: {}",
            &url[..url.len().min(60)]
        );
    }
    for loader in ["<script src", "<link rel=\"stylesheet\"", "@import", "url("] {
        assert!(!PAGE.contains(loader), "page loads a file: {loader}");
    }
}

#[test]
fn page_has_the_title_and_one_button() {
    assert!(PAGE.contains("<title>Date calculator</title>"));
    assert!(PAGE.contains("<button class=\"go\">Calculate</button>"));
    assert!(PAGE.contains("fetch(\"/api\""));
}

/// `date-calc ui` on a free port, stopped when dropped.
struct Server {
    child: Child,
    port: u16,
}

impl Server {
    fn start(args: &[&str]) -> Server {
        let mut child = Command::new(BIN)
            .arg("ui")
            .args(args)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line
            .strip_prefix("Open http://127.0.0.1:")
            .and_then(|rest| rest.strip_suffix("  (Ctrl+C to stop)\n"))
            .unwrap_or_else(|| panic!("{line:?}"))
            .parse()
            .unwrap();
        Server { child, port }
    }

    /// Sends `head` (request line and headers, `{port}` filled in) and `body`; returns the
    /// status, the headers and the body.
    fn send(&self, head: &str, body: &[u8]) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        let head = head.replace("{port}", &self.port.to_string());
        stream
            .write_all(format!("{head}\r\n\r\n").as_bytes())
            .unwrap();
        stream.write_all(body).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        let status = headers[9..12].parse().unwrap();
        (status, headers.to_owned(), body.to_owned())
    }

    fn post(&self, request: &Value) -> (u16, Value) {
        let body = request.to_string();
        let head = format!(
            "POST /api HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nOrigin: http://127.0.0.1:{{port}}\r\nContent-Type: application/json\r\nContent-Length: {}",
            body.len()
        );
        let (status, _, body) = self.send(&head, body.as_bytes());
        (status, serde_json::from_str(&body).unwrap())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

/// The CLI's `--json` answer and its text output for the same input.
fn cli(args: &[&str]) -> (Value, String) {
    let run = |extra: &[&str]| {
        let out = Command::new(BIN)
            .args(args)
            .args(extra)
            .env_remove("CLICOLOR_FORCE")
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap()
    };
    (serde_json::from_str(&run(&["--json"])).unwrap(), run(&[]))
}

#[test]
fn answers_every_query_form_like_the_cli() {
    let server = Server::start(&["--no-open"]);
    let (status, headers, body) = server.send("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}", b"");
    assert_eq!(status, 200);
    assert_eq!(body, PAGE);
    assert!(headers.contains("Content-Type: text/html; charset=utf-8\r\n"));
    assert!(headers.contains("Content-Security-Policy: default-src 'none';"));
    for (request, op, args) in [
        (
            json!({"query": "2026-02-01 to 2026-02-28", "country": "CN", "include_start": true, "include_end": true}),
            "between",
            &["between", "2026-02-01", "2026-02-28", "-c", "CN"][..],
        ),
        (
            json!({"query": "2026-01-01 until 2026-01-31", "country": "DE", "region": "BY", "include_start": false, "include_end": true}),
            "between",
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
            json!({"query": "2026-11-02 .. 2026-11-06", "weekend": "FRI,SAT"}),
            "between",
            &[
                "between",
                "2026-11-02",
                "2026-11-06",
                "--weekend",
                "FRI,SAT",
            ],
        ),
        (
            json!({"query": "2026-11-02 10:00 to 2026-11-05 15:00", "country": "DE", "hours": "09:00-12:00,13:00-17:00"}),
            "hours",
            &[
                "hours",
                "2026-11-02 10:00",
                "2026-11-05 15:00",
                "-c",
                "DE",
                "--hours",
                "09:00-12:00,13:00-17:00",
            ],
        ),
        (
            json!({"query": "2026-10-30T16:00:00Z to 2026-11-02T18:00:00Z", "country": "CA-BC", "timezone": "America/Vancouver"}),
            "hours",
            &[
                "hours",
                "2026-10-30T16:00:00Z",
                "2026-11-02T18:00:00Z",
                "-c",
                "CA-BC",
                "--tz",
                "America/Vancouver",
            ],
        ),
        (
            json!({"query": "2026-11-02 +10", "country": "US", "hours": "09:00-17:00"}),
            "add",
            &["add", "2026-11-02", "10", "-c", "US"],
        ),
        (
            json!({"query": "2026-03-02 -5 business days", "country": "CN"}),
            "add",
            &["add", "2026-03-02", "-5", "-c", "CN"],
        ),
        (
            json!({"query": "2026-01-31 1 month"}),
            "add",
            &["add", "2026-01-31", "1", "-u", "months"],
        ),
        (
            json!({"query": "2026-11-02 16:00 +3 h", "country": "DE"}),
            "add",
            &["add", "2026-11-02 16:00", "3", "-u", "bh", "-c", "DE"],
        ),
        (
            json!({"query": "2027", "country": "JP"}),
            "holidays",
            &["holidays", "JP", "2027"],
        ),
        (
            json!({"query": "holidays 2026", "country": "DE", "region": "BY"}),
            "holidays",
            &["holidays", "DE", "2026", "-r", "BY"],
        ),
        (
            json!({"query": "2026-10-27"}),
            "convert",
            &["convert", "2026-10-27"],
        ),
        (
            json!({"query": "5787-05L-01 hebrew"}),
            "convert",
            &["convert", "5787-05L-01", "--from", "hebrew"],
        ),
        (
            json!({"query": "1 Ramadan 1448"}),
            "convert",
            &["convert", "1 Ramadan 1448"],
        ),
    ] {
        let (status, reply) = server.post(&request);
        let (result, text) = cli(args);
        assert_eq!(status, 200, "{request}: {reply}");
        assert_eq!(
            reply,
            json!({"op": op, "result": result, "text": text}),
            "{args:?}"
        );
    }
    for (request, args) in [
        (json!({"op": "countries"}), &["countries"][..]),
        (
            json!({"op": "countries", "country": "DE"}),
            &["countries", "DE"],
        ),
    ] {
        let reply = json!({"op": "countries", "result": cli(args).0});
        assert_eq!(server.post(&request), (200, reply), "{args:?}");
    }
    for (request, code, args) in [
        (
            json!({"op": "countries", "country": "XX"}),
            "unknown_country",
            Some(&["countries", "XX"][..]),
        ),
        (json!({"query": "holidays"}), "country_required", None),
        (
            json!({"query": "2026-01-06 to 2026-01-06", "region": "BY"}),
            "country_required",
            None,
        ),
        (json!({"query": ""}), "bad_query", None),
        (json!({"query": "next tuesday"}), "bad_query", None),
        (
            json!({"query": "2026-02-01 から 2026-02-28"}),
            "bad_query",
            None,
        ),
        (json!({"query": "x💥yz 2026"}), "bad_query", None),
        (
            json!({"query": "2026-02-30 to 2026-03-05", "country": "CN"}),
            "bad_date",
            Some(&["between", "2026-02-30", "2026-03-05", "-c", "CN"]),
        ),
        (
            json!({"query": "2027-01-04 to 2027-01-05", "country": "LK"}),
            "no_holiday_data",
            Some(&["between", "2027-01-04", "2027-01-05", "-c", "LK"]),
        ),
        (
            json!({"query": "2026-10-30T16:00:00Z to 2026-11-02T18:00:00Z"}),
            "tz_required",
            Some(&["hours", "2026-10-30T16:00:00Z", "2026-11-02T18:00:00Z"]),
        ),
        (
            json!({"query": "2026-11-02 +1 h", "timezone": "Mars/Base"}),
            "unknown_timezone",
            None,
        ),
    ] {
        let (status, reply) = server.post(&request);
        assert_eq!(
            (status, &reply["error"]["code"]),
            (422, &json!(code)),
            "{request}: {reply}"
        );
        if let Some(args) = args {
            assert_eq!(reply, cli(args).0);
        }
    }
    for request in [
        json!({"country": "DE"}),
        json!({"op": "between", "query": "2026"}),
        json!({"query": 2026}),
        json!("2026-10-27"),
    ] {
        let (status, error) = server.post(&request);
        assert_eq!(
            (status, &error["error"]["code"]),
            (400, &json!("bad_request")),
            "{request}"
        );
    }
    let head = "POST /api HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://LOCALHOST:{port}\r\nContent-Length: 5";
    let (status, _, body) = server.send(head, b"{nope");
    assert_eq!(status, 400);
    assert!(
        body.starts_with(r#"{"error":{"code":"bad_request","message":"Can't read the request: "#)
    );
}

#[test]
fn refuses_other_hosts_origins_routes_and_large_requests() {
    let server = Server::start(&["--no-open"]);
    let other = server.port.wrapping_add(1);
    for (head, status) in [
        ("GET / HTTP/1.1\r\nHost: localhost:{port}".to_owned(), 200),
        (
            "GET /?from=bookmark HTTP/1.1\r\nHost: localhost:{port}".into(),
            200,
        ),
        ("GET /api?x HTTP/1.1\r\nHost: localhost:{port}".into(), 404),
        ("GET / HTTP/1.1".into(), 403),
        ("GET / HTTP/1.1\r\nHost: evil.example".into(), 403),
        ("GET / HTTP/1.1\r\nHost: evil.example:{port}".into(), 403),
        (format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{other}"), 403),
        (
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://evil.example".into(),
            403,
        ),
        (
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://127.0.0.1:{port}".into(),
            403,
        ),
        (
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null".into(),
            403,
        ),
        ("GET /api HTTP/1.1\r\nHost: 127.0.0.1:{port}".into(), 404),
        (
            "GET /favicon.ico HTTP/1.1\r\nHost: 127.0.0.1:{port}".into(),
            404,
        ),
        (
            "POST / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0".into(),
            404,
        ),
        ("DELETE /api HTTP/1.1\r\nHost: 127.0.0.1:{port}".into(), 404),
    ] {
        assert_eq!(server.send(&head, b"").0, status, "{head}");
    }
    let (_, _, body) = server.send("GET / HTTP/1.1\r\nHost: evil.example", b"");
    assert_eq!(
        body,
        r#"{"error":{"code":"forbidden","message":"Only this computer's page can use date-calc ui.","hint":null}}"#
    );
    let big = vec![b' '; 16 * 1024 + 1];
    let head = format!(
        "POST /api HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nContent-Length: {}",
        big.len()
    );
    let (status, _, body) = server.send(&head, &big);
    assert_eq!(status, 413);
    assert_eq!(
        body,
        r#"{"error":{"code":"too_large","message":"The request is larger than 16 KB.","hint":null}}"#
    );
    let request = r#"{"query":"2026-10-27"}"#;
    let padded = format!("{request}{}", " ".repeat(16 * 1024 - request.len()));
    let head = format!(
        "POST /api HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nContent-Length: {}",
        padded.len()
    );
    assert_eq!(server.send(&head, padded.as_bytes()).0, 200);
    let head = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nX-Pad: {}",
        "a".repeat(17 * 1024)
    );
    assert_eq!(server.send(&head, b"").0, 413);
}

#[test]
fn refuses_repeated_headers_and_short_bodies() {
    let server = Server::start(&["--no-open"]);
    let body = r#"{"query":"2026-10-27"}"#;
    let post = |headers: &str, length: usize| {
        let head = format!("POST /api HTTP/1.1\r\n{headers}\r\nContent-Length: {length}");
        server.send(&head, body.as_bytes()).0
    };
    let local = "Host: 127.0.0.1:{port}";
    assert_eq!(post(local, body.len()), 200);
    for headers in [
        "Host: evil.example\r\nHost: 127.0.0.1:{port}",
        "Host: 127.0.0.1:{port}\r\nHost: 127.0.0.1:{port}",
        "Host: 127.0.0.1:{port}\r\nOrigin: http://evil.example\r\nOrigin: http://127.0.0.1:{port}",
        "Host: 127.0.0.1:{port}\r\nContent-Length: 0",
    ] {
        assert_eq!(post(headers, body.len()), 400, "{headers}");
    }
    assert_eq!(post(local, body.len() + 10), 400);
    let hidden =
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nNoColon\r\nOrigin: http://evil.example";
    assert_eq!(server.send(hidden, b"").0, 400);
}

#[test]
fn refuses_malformed_header_names_framing_and_request_lines() {
    let server = Server::start(&["--no-open"]);
    let body = r#"{"query":"2026-10-27"}"#;
    let length = format!("Content-Length: {}", body.len());
    let head = format!("POST /api HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\n{length}");
    for head in [
        format!("{head}\r\nOrigin : http://evil.example"),
        format!("{head}\r\nBad Header: value"),
        format!("{head}\r\nTransfer-Encoding: chunked"),
        format!("{head}\r\nTransfer-Encoding: identity"),
        head.replace("Length: ", "Length: +"),
        head.replace("Length: ", "Length: -"),
        head.replace(" HTTP/1.1", ""),
        head.replace("HTTP/1.1", "XYZ"),
        head.replace("POST /api", "POST  /api"),
    ] {
        assert_eq!(server.send(&head, body.as_bytes()).0, 400, "{head}");
    }
}

#[test]
fn reads_opaque_header_bytes_and_refuses_control_bytes() {
    let server = Server::start(&["--no-open"]);
    let body = r#"{"query":"2026-10-27"}"#;
    let status = |line: &[u8], header: &[u8]| {
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        let host = format!(
            "\r\nHost: 127.0.0.1:{}\r\nContent-Length: {}\r\n",
            server.port,
            body.len()
        );
        let request = [line, host.as_bytes(), header, b"\r\n\r\n", body.as_bytes()].concat();
        stream.write_all(&request).unwrap();
        let mut response = Vec::new();
        // A server that drops the request may reset the connection; that reads as no status.
        let _ = stream.read_to_end(&mut response);
        String::from_utf8_lossy(response.get(9..12).unwrap_or_default()).into_owned()
    };
    let post = b"POST /api HTTP/1.1".as_slice();
    // RFC 9110 §5.5: field values may carry opaque bytes 80-FF; an unknown field is ignored.
    for byte in [0x80, 0xe9, 0xff] {
        assert_eq!(status(post, &[b"X-Other: ", &[byte][..]].concat()), "200");
    }
    assert_eq!(status(post, b"\xe9: x"), "400");
    // Control bytes are neither field content nor part of a target (RFC 9110 §5.5, RFC 9112 §3).
    assert_eq!(status(post, b"X-Other: a\0b"), "400");
    assert_eq!(status(post, b"X-Other: a\rb"), "400");
    assert_eq!(status(post, b"X-Other: a\tb"), "200");
    assert_eq!(status(b"POST /api\tx HTTP/1.1", b"X-Other: a"), "400");
    assert_eq!(status(b"POST /api\xe9 HTTP/1.1", b"X-Other: a"), "400");
    assert_eq!(status(post, b"X-Other: a"), "200");
    let raw = b"{\"query\":\"\xff\"}";
    let head = format!(
        "POST /api HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nContent-Length: {}",
        raw.len()
    );
    assert_eq!(server.send(&head, raw).0, 400);
}

#[test]
fn answers_concurrent_clients_while_one_stalls() {
    let server = Server::start(&["--no-open"]);
    let _stalled = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    let request = json!({"query": "2026-01-01 to 2026-12-31", "country": "DE-BY"});
    let answers: Vec<(u16, Value)> = std::thread::scope(|s| {
        let posts: Vec<_> = (0..16).map(|_| s.spawn(|| server.post(&request))).collect();
        posts.into_iter().map(|p| p.join().unwrap()).collect()
    });
    assert_eq!(answers[0].0, 200);
    assert_eq!(answers[0].1["result"]["business_days"], 252);
    assert!(answers.iter().all(|a| *a == answers[0]));
}

#[test]
fn reports_a_busy_port() {
    let taken = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    let out = Command::new(BIN)
        .args(["ui", "--no-open", "--port", &port])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        format!("✗ Port {port} is in use.\n  Use --port 0 for a free port.\n")
    );
}

/// A stand-in for xdg-open and open that writes down the address it was given.
#[cfg(unix)]
#[test]
fn opens_the_browser_unless_told_not_to() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("date-calc-browser-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opened = dir.join("opened");
    for opener in ["xdg-open", "open"] {
        let script = dir.join(opener);
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho \"$1\" > '{}'\n", opened.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
    let opened_url = || std::fs::read_to_string(&opened).ok();
    let start = |args: &[&str]| {
        let mut child = Command::new(BIN)
            .arg("ui")
            .args(args)
            .env("PATH", &path)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        child.kill().unwrap();
        child.wait().unwrap();
        line
    };
    start(&["--no-open"]);
    assert_eq!(opened_url(), None);
    let line = start(&[]);
    let url = line.split_whitespace().nth(1).unwrap();
    assert_eq!(opened_url().as_deref(), Some(format!("{url}\n").as_str()));
    std::fs::remove_dir_all(&dir).unwrap();
}
