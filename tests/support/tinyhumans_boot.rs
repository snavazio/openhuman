//! LOCAL-ONLY FORK: there is no hosted-backend transport to install.
//!
//! Upstream, this booted the SDK-backed TinyHumans transport for suites that
//! run the core in-process against a mock backend. The fork removed that
//! transport, so [`boot`] does nothing and [`is_booted`] is always false. It
//! is kept so the suites still compile; tests that need the hosted backend
//! fail with `BACKEND_UNAVAILABLE:` and are reviewed separately.
//!
//! Include with `#[path = "support/tinyhumans_boot.rs"] mod tinyhumans_boot;`
//! (or `../support/...` from `tests/raw_coverage/`).

#![allow(dead_code)]

/// No-op: nothing to install in a local-only build.
pub fn boot() {}

/// Always false: the hosted layer is never installed.
pub fn is_booted() -> bool {
    false
}
