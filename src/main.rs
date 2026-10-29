fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => println!("date-calc {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!("Usage: date-calc --version");
            std::process::exit(2);
        }
    }
}
