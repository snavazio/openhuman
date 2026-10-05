//! The entry point for the OpenHuman core application.
//!
//! This file is responsible for:
//! - Initializing error tracking with Sentry.
//! - Setting up secret scrubbing for outgoing error reports.
//! - Dispatching command-line arguments to the core logic in `openhuman_core`.

/// Main application entry point.
///
/// It initializes the Sentry SDK for error monitoring, ensuring that sensitive
/// information is redacted before being sent to the server. After setup, it
/// delegates execution to the core library based on CLI arguments.
fn main() {
    restore_default_sigpipe();

    // Load `.env` early. `dotenvy::dotenv()` is a no-op for variables already
    // present in the process environment, and the CLI dispatcher later calls
    // `load_dotenv_for_cli` which honors `OPENHUMAN_DOTENV_PATH`; this early
    // call handles the common default case (repo-local `.env`) so startup-time
    // consumers (config overrides) see the same values as runtime RPC handlers.
    let _ = dotenvy::dotenv();

    // Collect command-line arguments, skipping the binary name.
    let args: Vec<String> = std::env::args().skip(1).collect();

    // LOCAL-ONLY FORK: no hosted-backend transport is installed.

    // `run` / `serve` start the JSON-RPC server from `openhuman-rpc`, which the
    // core cannot depend on; hand it the launcher before dispatching.
    openhuman_rpc::server::install_cli_server();

    // Delegate to the core library to handle the command.
    if let Err(err) = openhuman_core::run_core_from_args(&args) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

#[cfg(unix)]
fn restore_default_sigpipe() {
    // Rust ignores SIGPIPE at startup. That makes writes to a closed pipe
    // return EPIPE, which the print macros turn into a panic. CLI tools should
    // instead terminate quietly when a downstream reader such as `head` exits.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_default_sigpipe() {}

// ---------------------------------------------------------------------------
// Release / environment resolution for Sentry
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Secret scrubbing
// ---------------------------------------------------------------------------
