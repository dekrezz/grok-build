//! Core telemetry tracking — outbound analytics removed.
//!
//! Upstream this module posted every event twice: to xAI's product-events
//! endpoint and to Mixpanel. Both send paths are deleted, so [`track`] now
//! assembles nothing that leaves the machine. See NO-TELEMETRY.md.
//!
//! The module and its public API are kept because the rest of the tree calls
//! into them (`is_enabled`, `session_metrics`, the local logs) and the mode
//! contract is still meaningful for local-only behaviour.

use std::sync::{Mutex, OnceLock};

use chrono::{Local, SecondsFormat};
use serde_json::json;

use crate::config::{TelemetryConfig, TelemetryMode, deployment_id_from_key};
use crate::http::OriginClientInfo;

/// Event property map shared by all telemetry modules.
pub type Metadata = serde_json::Map<String, serde_json::Value>;



#[derive(Clone)]
pub struct TelemetryClient {
    mode: TelemetryMode,
    user_id: Option<String>,
    team_id: Option<String>,
    deployment_id: Option<String>,
    shell_version: String,
    client_type: Option<String>,
    client_version: Option<String>,
    subscription_tier: Option<String>,
}

impl std::fmt::Debug for TelemetryClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The previous impl printed the analytics endpoint and a masked API key.
        // Neither is stored any more, so the mode is all there is to report.
        f.debug_struct("TelemetryClient")
            .field("mode", &self.mode)
            .finish()
    }
}

impl TelemetryClient {
    pub fn from_config(
        config: TelemetryConfig,
        mode: TelemetryMode,
        user_id: Option<String>,
        team_id: Option<String>,
        deployment_key: Option<String>,
        origin_client: Option<OriginClientInfo>,
        shell_version: String,
        subscription_tier: Option<String>,
        _http_client: reqwest::Client,
    ) -> Self {
        // `config`'s analytics fields (`events_url`, `events_api_key`,
        // `mixpanel_token`, `mixpanel_enabled`) are deliberately ignored: no
        // Mixpanel client and no events POST exist in this build. They remain on
        // `TelemetryConfig` so existing config files and env vars still parse.
        // `_http_client` is unused too; the parameter stays so callers in
        // shell/pager compile unchanged.
        let _ = &config;
        let deployment_id = deployment_key
            .filter(|s| !s.is_empty())
            .map(|k| deployment_id_from_key(&k));
        let (client_type, client_version) = match origin_client {
            Some(o) => (Some(o.product), o.version),
            None => (None, None),
        };

        Self {
            mode,
            user_id,
            team_id,
            deployment_id,
            shell_version,
            client_type,
            client_version,
            subscription_tier: subscription_tier.map(|t| normalize_tier(&t)),
        }
    }
}

/// Normalize a subscription tier string to a consistent lowercase_underscore
/// format for Mixpanel. Handles both CCP display names ("SuperGrok Heavy")
/// and JWT-derived keys ("supergrok_heavy").
fn normalize_tier(tier: &str) -> String {
    match tier {
        "SuperGrok Heavy" | "supergrok_heavy" => "supergrok_heavy",
        "SuperGrok Plus" | "supergrok_plus" => "supergrok_plus",
        "SuperGrok" | "supergrok" => "supergrok",
        "SuperGrok Lite" | "supergrok_lite" => "supergrok_lite",
        "X Premium+" | "x_premium_plus" => "x_premium_plus",
        "X Premium" | "x_premium" => "x_premium",
        "X Basic" | "x_basic" => "x_basic",
        "Free" | "free" => "free",
        // Team / console API keys — dedicated Mixpanel segment, not free.
        "API Key" | "api_key" => "api_key",
        other => return other.to_ascii_lowercase().replace(' ', "_"),
    }
    .to_string()
}

static TELEMETRY_CLIENT: OnceLock<Mutex<Option<TelemetryClient>>> = OnceLock::new();

/// Returns `true` when telemetry mode is `Enabled`.
/// Used by `log_event` — product analytics events only fire in `Enabled` mode.
pub fn is_enabled() -> bool {
    TELEMETRY_CLIENT
        .get()
        .and_then(|m| m.lock().ok())
        .is_some_and(|g| g.as_ref().is_some_and(|c| c.mode.is_enabled()))
}

/// Returns `true` when telemetry mode is `Enabled` or `SessionMetrics`.
/// Used by `session_metrics` — lifecycle events fire in both modes.
pub fn is_session_metrics_enabled() -> bool {
    TELEMETRY_CLIENT
        .get()
        .and_then(|m| m.lock().ok())
        .is_some_and(|g| g.as_ref().is_some_and(|c| c.mode.session_metrics_enabled()))
}

pub struct UserContext {
    pub country: String,
    pub language: String,
    pub timestamp: String,
}

impl UserContext {
    pub fn collect() -> Self {
        let default_language = whoami::Language::En(whoami::Country::Any);
        let lang = whoami::langs()
            .ok()
            .and_then(|mut langs| langs.next())
            .unwrap_or(default_language);
        Self {
            country: lang.country().to_string(),
            language: lang.to_string(),
            timestamp: Local::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        }
    }
}

/// Core telemetry emitter — now a sink with no outbound route.
///
/// Kept as the single funnel every caller already uses, so events stay
/// centralised and nothing changes at the call sites. Both send paths (xAI
/// product events, Mixpanel) were removed.
pub async fn track(event_name: &str, request_id: &str, ctx: &UserContext, mut metadata: Metadata) {
    let lock = TELEMETRY_CLIENT.get_or_init(|| Mutex::new(None));
    let client = {
        let guard = lock.lock().unwrap_or_else(|err| err.into_inner());
        match guard.clone() {
            Some(c) => c,
            None => return,
        }
    };

    let agent_id = crate::id::agent_id();
    let user_id = client.user_id.as_deref().unwrap_or(&agent_id);
    metadata.insert("agent_id".into(), json!(agent_id));
    if let Some(ref team_id) = client.team_id {
        metadata.insert("team_id".into(), json!(team_id));
    }
    if let Some(ref deployment_id) = client.deployment_id {
        metadata.insert("deployment_id".into(), json!(deployment_id));
    }
    metadata.insert("shell_version".into(), json!(client.shell_version));
    if let Some(ref client_type) = client.client_type {
        metadata.insert("client_type".into(), json!(client_type));
    }
    if let Some(ref client_version) = client.client_version {
        metadata.insert("client_version".into(), json!(client_version));
    }
    if let Some(ref subscription_tier) = client.subscription_tier {
        metadata.insert("subscription_tier".into(), json!(subscription_tier));
    }

    // ─── Outbound analytics removed ───────────────────────────────────────
    //
    // Upstream this POSTed each event twice: to xAI's product-events endpoint
    // (`events_url` + `x-api-key`) and to Mixpanel's `/track` API. Both send
    // paths are deleted, so no event ever leaves the machine. The metadata above
    // is still assembled because the local debug/unified logs consume the same
    // shape; here it is simply dropped.
    let _ = (event_name, request_id, user_id, &metadata, ctx);
}

/// Sync the user's Mixpanel profile once per init. Fire-and-forget.
///
/// Only runs in [`TelemetryMode::Enabled`]. SessionMetrics mode may emit
/// lifecycle events via [`track`], but must not write Mixpanel people
/// profiles (`engage`).
pub fn sync_profile() {
    let lock = TELEMETRY_CLIENT.get_or_init(|| Mutex::new(None));
    let client = {
        let guard = lock.lock().unwrap_or_else(|err| err.into_inner());
        match guard.clone() {
            Some(c) => c,
            None => return,
        }
    };

    // The single profile-sync gate: reads the installed client's mode, so every
    // caller (and any init race) resolves against what was actually installed.
    if !client.mode.is_enabled() {
        return;
    }

    // ─── Profile sync removed ─────────────────────────────────────────────
    //
    // Upstream this spawned a Mixpanel `engage` call that created/updated a user
    // profile keyed by `agent_id` (or the authenticated user id) with shell
    // version, client type/version, deployment id, team id and subscription
    // tier. That is the identity-building half of the analytics pipeline, so it
    // is gone entirely -- no profile is written anywhere.
    //
    // The mode gate above is kept so the call stays a cheap early return and the
    // `Enabled`-only contract remains documented and tested.
}

/// Initialize telemetry client. Safe to call multiple times.
///
/// - `Disabled` → no client
/// - `SessionMetrics` → client active (only `session_metrics::*` events fire)
/// - `Enabled` → client active (all events fire)
///
/// `shell_version` is stamped into every event payload as `shell_version`
/// (legacy field name preserved for analytics continuity); shell passes its
/// own `CARGO_PKG_VERSION`. `http_client` is owned by the caller (typically
/// shell's `shared_client()`) so the shared TLS-warmed pool is reused for
/// telemetry posts.
pub fn init(
    config: TelemetryConfig,
    mode: TelemetryMode,
    user_id: Option<String>,
    team_id: Option<String>,
    deployment_key: Option<String>,
    origin_client: Option<OriginClientInfo>,
    shell_version: String,
    subscription_tier: Option<String>,
    http_client: reqwest::Client,
) {
    let lock = TELEMETRY_CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|err| err.into_inner());
    *guard = if mode.is_disabled() {
        None
    } else {
        Some(TelemetryClient::from_config(
            config,
            mode,
            user_id,
            team_id,
            deployment_key,
            origin_client,
            shell_version,
            subscription_tier,
            http_client,
        ))
    };
    drop(guard);
    sync_profile();
}

/// Re-initialize the telemetry client if it was not created at startup
/// (e.g. because auth was not yet available). No-op when the client
/// is already set, so safe to call unconditionally after auth succeeds.
pub fn init_if_needed(
    config: TelemetryConfig,
    mode: TelemetryMode,
    user_id: Option<String>,
    team_id: Option<String>,
    deployment_key: Option<String>,
    origin_client: Option<OriginClientInfo>,
    shell_version: String,
    subscription_tier: Option<String>,
    http_client: reqwest::Client,
) {
    if mode.is_disabled() {
        return;
    }
    let lock = TELEMETRY_CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|err| err.into_inner());
    if guard.is_none() {
        *guard = Some(TelemetryClient::from_config(
            config,
            mode,
            user_id,
            team_id,
            deployment_key,
            origin_client,
            shell_version,
            subscription_tier,
            http_client,
        ));
        drop(guard);
        sync_profile();
    }
}

#[cfg(test)]
mod tests {
    use super::*;



    /// SessionMetrics must not attempt Mixpanel profile engage — sync_profile
    /// is a no-op unless mode is fully Enabled.
    #[test]
    fn sync_profile_is_noop_in_session_metrics_mode() {
        // No tokio runtime here BY DESIGN: if the gate wrongly falls through,
        // sync_profile's tokio::spawn panics and fails this test. Converting
        // this to #[tokio::test] would silently turn it into theater.
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "this test must run without a tokio runtime"
        );
        // Clear the global client even if an assert below panics.
        struct ClearClient;
        impl Drop for ClearClient {
            fn drop(&mut self) {
                let lock = TELEMETRY_CLIENT.get_or_init(|| Mutex::new(None));
                *lock.lock().unwrap_or_else(|err| err.into_inner()) = None;
            }
        }
        let _clear = ClearClient;

        // Mixpanel configured, but no events endpoint: the global must never
        // carry a live funnel out of this test.
        let cfg = TelemetryConfig {
            mixpanel_enabled: true,
            mixpanel_token: Some("test-token".into()),
            events_url: None,
            events_api_key: None,
            ..TelemetryConfig::default()
        };
        init(
            cfg,
            TelemetryMode::SessionMetrics,
            Some("user-1".into()),
            None,
            None,
            None,
            "0.0.0-test".into(),
            None,
            reqwest::Client::new(),
        );
        // Explicit call must no-op too (init already invoked it once).
        sync_profile();
        assert!(
            is_session_metrics_enabled(),
            "client must be live for session metrics"
        );
        assert!(!is_enabled(), "product analytics must stay off");
    }




    /// Mixpanel `subscription_tier` must be a stable snake_case key. Free
    /// users arrive as CCP display `"Free"` or JWT-fallback `"free"`; both
    /// must land as `"free"` (not omitted / not `"Free"`).
    #[test]
    fn normalize_tier_maps_display_and_claim_names() {
        assert_eq!(normalize_tier("Free"), "free");
        assert_eq!(normalize_tier("free"), "free");
        assert_eq!(normalize_tier("SuperGrok"), "supergrok");
        assert_eq!(normalize_tier("SuperGrok Heavy"), "supergrok_heavy");
        assert_eq!(normalize_tier("supergrok_heavy"), "supergrok_heavy");
        assert_eq!(normalize_tier("X Basic"), "x_basic");
        assert_eq!(normalize_tier("X Premium+"), "x_premium_plus");
        assert_eq!(normalize_tier("X Premium"), "x_premium");
        assert_eq!(normalize_tier("SuperGrok Lite"), "supergrok_lite");
        assert_eq!(normalize_tier("SuperGrok Plus"), "supergrok_plus");
        assert_eq!(normalize_tier("supergrok_plus"), "supergrok_plus");
        // API key is a dedicated Mixpanel segment — never free.
        assert_eq!(normalize_tier("API Key"), "api_key");
        assert_eq!(normalize_tier("api_key"), "api_key");
    }

}
