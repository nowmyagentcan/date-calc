//! Every command in README.md runs, and every pasted output matches the binary's.

use std::process::{Command, Stdio};

const README: &str = include_str!("../README.md");

/// Splits a command line the way a POSIX shell does for these examples: spaces, double quotes,
/// and a trailing `  # comment`.
fn words(line: &str) -> Vec<String> {
    let line = line.split("  #").next().unwrap();
    let mut out = Vec::new();
    let (mut word, mut quoted) = (None::<String>, false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                word.get_or_insert_default();
            }
            ' ' if !quoted => out.extend(word.take()),
            _ => word.get_or_insert_default().push(c),
        }
    }
    out.extend(word);
    out
}

/// Exit code and stdout of `date-calc` with the words after its name.
fn run(line: &str) -> (i32, String) {
    let words = words(line);
    assert_eq!(words[0], "date-calc", "{line}");
    let out = Command::new(env!("CARGO_BIN_EXE_date-calc"))
        .args(&words[1..])
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.is_empty(), "{line}: {stderr}");
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

/// The README's fenced blocks as (language, body).
fn blocks() -> Vec<(&'static str, &'static str)> {
    README
        .split("```")
        .skip(1)
        .step_by(2)
        .map(|b| b.split_once('\n').unwrap())
        .collect()
}

#[test]
fn every_example_runs() {
    let mut ran = 0;
    for (lang, body) in blocks() {
        if lang != "sh" {
            continue;
        }
        for line in body.lines().filter(|l| l.starts_with("date-calc ")) {
            let (code, out) = run(line);
            assert_eq!(code, 0, "{line}");
            if line.contains("--json") {
                serde_json::from_str::<serde_json::Value>(&out).expect(line);
            }
            ran += 1;
        }
    }
    assert_eq!(ran, 11);
}

#[test]
fn every_pasted_output_matches() {
    let mut ran = 0;
    for (lang, body) in blocks() {
        if lang != "text" {
            continue;
        }
        let (line, expected) = body.split_once('\n').unwrap();
        let (code, out) = run(line.strip_prefix("$ ").unwrap());
        assert_eq!((code, out.as_str()), (0, expected), "{line}");
        ran += 1;
    }
    assert_eq!(ran, 2);
}
