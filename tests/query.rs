//! Query lines from the page and interactive mode, with any text people may type.

use date_calc::query::parse;

#[test]
fn reads_any_unicode_without_panicking() {
    let words = [
        "から",
        "💥x",
        "日本語です",
        "x💥yz",
        "é1:00",
        "ü",
        "–2026",
        "١٢:٣٤٥",
    ];
    let before = [
        "",
        "2026-11-02 ",
        "2026-11-02 10:00 ",
        "2026-11-02 to ",
        "1 ",
        "2026 ",
    ];
    for b in before {
        for w in words {
            let line = format!("{b}{w} 2026-11-05");
            let _ = parse(&line);
            let _ = parse(&format!("{b}{w}"));
        }
    }
}
