//! Binary entry point for the standalone terminal frontend; all logic lives in
//! the `openhuman_tui` library (see `lib.rs` and `README.md`).

fn main() {
    // Load a repository-local `.env` before startup; explicit process
    // variables still take precedence. (LOCAL-ONLY FORK: Sentry removed.)
    let _ = dotenvy::dotenv();
    if let Err(error) = openhuman_tui::run_from_cli(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}
