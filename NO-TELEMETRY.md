# Telemetry removal

This fork removes vendor telemetry from `grok`. The goal is narrow and concrete:
**no usage data, analytics event, crash report or session trace is sent to xAI or
any third party**, and no configuration — local, baked-in, or server-delivered —
can turn that back on.

## What was removed

### 1. Mixpanel product analytics — deleted

The `xai-mixpanel` crate is gone: removed from the workspace members, from the
workspace dependency table, from `xai-grok-telemetry`'s dependencies, and the
directory itself is deleted.

It was a client for `https://api.mixpanel.com/track` and
`https://api.mixpanel.com/engage`. `TelemetryClient` no longer constructs it and
no longer carries a `mixpanel` field.

### 2. xAI product-events endpoint — send path deleted

`xai-grok-telemetry/src/client.rs::track()` used to POST every event to
`events_url` with an `x-api-key` header. That POST is deleted. The payload
carried, among other things, agent id, team id, deployment id, shell version,
client type/version, subscription tier, country and language.

### 3. Mixpanel profile sync — deleted

`client.rs::sync_profile()` used to spawn a Mixpanel `engage` call that built a
persistent user profile keyed by agent/user id, with shell version, client
type/version, deployment id, team id and subscription tier. Removed entirely.

### 4. Build-time credential baking — removed

`TelemetryConfig::default()` read three compile-time bakes via `option_env!`:

- `GROK_TELEMETRY_BUILD_EVENTS_URL`
- `GROK_TELEMETRY_BUILD_EVENTS_API_KEY`
- `GROK_TELEMETRY_BUILD_MIXPANEL_TOKEN`

A release build could therefore ship with a live analytics endpoint and
credentials embedded in the binary. The bakes are removed and the fields are
hardcoded empty. The unit test `default_never_configures_an_analytics_sink`
guards this: if someone reintroduces an `option_env!` bake, the test fails.

Note that the public tree already had the *internal* defaults blanked
(`internal_defaults()` returned all-`None`), so a from-source build was already
credential-free. The `option_env!` layer was the remaining injection point.

### 5. Sentry crash reporting — made inert

`xai-grok-telemetry/src/sentry.rs` no longer reads `SENTRY_DSN` from the
environment or from a compile-time bake. `init()` always returns
`sentry::init(ClientOptions::default())`, which with no DSN captures nothing and
opens no sockets.

The payload scrubber (secret redaction, home-path and username masking) and its
tests were removed along with the send path — they existed only to sanitise
events that are no longer produced. The public surface (`Config`, `init`,
`flush_on_shutdown`) is unchanged, so the pager and signal handlers needed no
edits.

### 6. Remote activation — closed

This is the most important change, and the one that was never a user-visible
setting.

`xai-grok-shell/src/agent/config.rs::resolve_telemetry_mode()` consulted
server-delivered `remote_settings` (`telemetry_mode` / `telemetry_enabled`)
*after* local config. The backend could therefore enable telemetry on a client
that had never opted in locally. That branch is removed — only an admin
requirement, `GROK_TELEMETRY_ENABLED`, or `[features] telemetry` can move off the
disabled default.

`resolve_trace_upload()` had the same hole via the remote `trace_upload_enabled`
feature flag. Session traces carry prompts and file contents, so that flag is no
longer consulted either; uploads now require an explicit local opt-in.

## What was deliberately left alone

Not everything that greps as "telemetry" is surveillance. These are untouched, on
purpose:

- **Local logging and tracing** — `debug_log.rs`, `unified_log.rs`,
  `instrumentation.rs`, the `tracing` layers. These write to files on your own
  disk and are what makes the tool debuggable. Nothing is transmitted.
- **External OpenTelemetry** (`external/`, `otel_layer/`, `otlp_http.rs`) — an
  opt-in exporter that ships a curated usage schema to *your own* OTLP collector,
  off by default. It only sends where you explicitly point it, so removing it
  would delete a feature rather than close a leak.
- **`xai-crash-handler`** — writes crash reports locally; it has no HTTP client
  and no network code.
- **Update checks and announcements** — needed for the tool to tell you a new
  version exists. `announcements` only *parses* server-sent content; it does not
  report anything about you.
- **Coding data sharing** (`coding_data_sharing`) — already defaults to opt-out
  upstream, so no change was needed.

## Verifying it yourself

The send paths are gone from source, so the cheapest check is a grep:

```sh
# no Mixpanel crate, no Mixpanel endpoints anywhere
grep -rn "xai_mixpanel\|api.mixpanel.com" --include='*.rs' --include='*.toml' .

# no outbound POST left in the telemetry client
grep -n "\.post(\|\.send()" crates/codegen/xai-grok-telemetry/src/client.rs

# the anti-baking guard test
cargo test -p xai-grok-telemetry default_never_configures_an_analytics_sink
```

At runtime the honest check is a network one: run the binary under a proxy or
`lsof` / Little Snitch and confirm the only destinations are the xAI API
endpoints the agent needs in order to function (inference, auth, updates).

## Caveat

`grok` is a client for a hosted model. Your prompts and the file contents the
agent reads are sent to xAI's API because that is how inference works — that is
the product, not telemetry, and removing it would leave a non-functioning binary.
What this change removes is the *additional* analytics, crash-reporting and
trace-upload layer on top of that.

Building from source is what makes any of this meaningful: a prebuilt binary from
`x.ai/cli` is not covered by it.

## Build status

These changes are source-level edits to the crates listed above. They have not
been compiled in this working tree — verify with `cargo check --workspace` (needs
`protoc`, or `bin/protoc` via `cargo install dotslash`) before relying on them.
