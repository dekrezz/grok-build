//! Sentry crash/error reporting — disabled in this build.
//!
//! Upstream this module initialised a real Sentry client from `SENTRY_DSN` (env
//! var or baked in at compile time) and shipped panics, stack traces,
//! breadcrumbs and tags to Sentry's servers, with a scrubber pass to strip
//! secrets and home-directory paths before sending.
//!
//! All outbound reporting has been removed. [`init`] now always returns an inert
//! `sentry::init(ClientOptions::default())` guard: with no DSN configured the
//! Sentry SDK captures nothing and opens no sockets. The DSN is deliberately
//! never read, so neither the environment nor a build-time bake can re-enable
//! reporting.
//!
//! The payload scrubber and its tests were dropped along with the send path --
//! they existed only to sanitise events that are no longer produced.
//!
//! The public surface (`Config`, [`init`], [`flush_on_shutdown`]) is kept intact
//! so the pager and signal handlers need no changes. See NO-TELEMETRY.md.

use std::sync::OnceLock;
use std::time::Duration;

use sentry::ClientInitGuard;
use sentry::ClientOptions;

const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

// ─── Host integration ─────────────────────────────────────────────────────

/// Per-host config; everything that varies between binaries lives here.
///
/// Retained for API compatibility. Since reporting is disabled, these fields are
/// no longer attached to any event.
pub struct Config {
    /// Sentry tag `client`, e.g. `"grok-pager"`.
    pub client: &'static str,
    pub client_version: &'static str,
    pub release: &'static str,
    /// Historically forced a no-op guard. Reporting is now always off.
    pub disabled: bool,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

// ─── Public API ────────────────────────────────────────────────────────────

/// Returns an inert Sentry guard. Nothing is captured and nothing is sent.
///
/// The config is still stored once so repeated calls stay cheap and the original
/// contract (guard must outlive the process) is unchanged.
pub fn init(config: Config) -> ClientInitGuard {
    let _config = CONFIG.get_or_init(|| config);
    sentry::init(ClientOptions::default())
}

/// Flush in-flight events. Call before `std::process::exit` in signal handlers.
///
/// Kept so shutdown paths are unchanged; with no DSN there is nothing queued, so
/// this returns immediately.
pub fn flush_on_shutdown() {
    if let Some(client) = sentry::Hub::current().client() {
        client.flush(Some(FLUSH_TIMEOUT));
    }
}
