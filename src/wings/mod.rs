//! Client-side support for the [mise-wings](https://mise-wings.en.dev)
//! asset cache. Three concerns live here:
//!
//!   1. [`credentials`] — persistent storage of the wings session JWT
//!      and its rotated refresh token, in a 0600-mode JSON file under
//!      `MISE_STATE_DIR/wings/credentials.json`.
//!   2. [`client`] — typed HTTP calls against the proxy's `/auth/dev`
//!      / `/auth/dev/refresh` / `/auth/dev/revoke` endpoints.
//!   3. [`url`] — origin-to-cache-subdomain rewriting. Maps
//!      `registry.npmjs.org` → `npm.<wings.host>` (and friends) when
//!      `wings.enabled = true` and credentials are loaded; nothing
//!      else.
//!
//! The CLI surface (`mise wings login/logout/whoami/status`) lives
//! in `src/cli/wings/`. This module is the plumbing those commands —
//! and the HTTP client's transparent header / URL injection — both
//! sit on top of.
//!
//! ## Why a separate module from `tokens.rs`
//!
//! `tokens.rs` reads from netrc / git-credential / per-host TOML
//! files keyed on the host name. Wings credentials are scoped to
//! the user's *Clerk identity*, not a host: a single credential
//! authenticates against every cache subdomain. The lookup keying
//! is different enough that folding it into `tokens.rs` would either
//! break that file's per-host mental model or warp the wings shape
//! to fit it. Separate module, shared file conventions.

pub mod ci;
pub mod client;
pub mod credentials;
pub mod http_hooks;
pub mod url;

/// Current unix timestamp (whole seconds). Single shared
/// helper so the credential expiry math (`should_refresh`,
/// `refresh_token_expired`) and the HTTP hook's "how long
/// since refresh expired" log line agree on the clock.
/// Greptile flagged the prior duplicate definitions across
/// `credentials.rs` and `http_hooks.rs` on PR review.
pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
