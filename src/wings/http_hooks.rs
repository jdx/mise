//! Hooks called by `crate::http::send_once` to weave wings
//! authentication and URL rewriting into the existing HTTP
//! pipeline. Two entry points:
//!
//!   - [`prepare_request`] — async, called once per outbound
//!     request. Decides whether wings activation applies
//!     (gate), runs auto-refresh if the access token is
//!     within the leeway window, mutates the URL to the
//!     wings cache subdomain, and returns the
//!     `Authorization: Bearer <wings-jwt>` header to attach.
//!
//!     Returns an empty header map for the no-op case (wings
//!     disabled, no credentials, or URL host isn't an upstream
//!     we rewrite). The HTTP layer then proceeds as if wings
//!     wasn't here.
//!
//! Splitting this into its own module keeps `crate::http`
//! free of wings-specific control flow — the call site there
//! is one async function call followed by a header merge.

use eyre::Result;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use url::Url;

use crate::config::Settings;
use crate::wings::{client, credentials, url as wings_url};

/// Leeway (seconds) before a wings access token's `exp` at
/// which the auto-refresh path triggers. 5 min is enough
/// headroom that a `mise install` run that takes 4 minutes
/// to download all its tarballs doesn't have its first
/// request authenticate fine and its last request 401 from
/// a token that lapsed mid-stream.
const REFRESH_LEEWAY_SECS: i64 = 5 * 60;

/// Wings preparation for an outbound HTTP request.
///
/// Mutates `url` in place to point at the wings cache
/// subdomain when activation applies, and returns the
/// `HeaderMap` containing the Bearer token. Returns
/// `Ok(HeaderMap::new())` (empty) for any of:
///
///   - `wings.enabled = false`
///   - no credentials on disk (user hasn't logged in)
///   - URL host isn't one of the upstream origins we know
///     how to rewrite (npm registry, github.com, api.github,
///     objects.githubusercontent)
///
/// Auto-refresh fires when the access token is within
/// [`REFRESH_LEEWAY_SECS`] of expiry. The refresh is
/// coordinated through [`credentials::lock_refresh`] so two
/// concurrent requests don't both POST the refresh and trip
/// the proxy's rotation tripwire — the loser of the lock
/// race re-reads the cached credentials after the winner
/// finishes and uses the rotated access token.
///
/// Auto-refresh failures are logged and the request passes
/// through with the original (about-to-expire) token. The
/// proxy will 401 on a truly-expired token; the user sees
/// "wings session expired, please re-login" rather than the
/// CLI silently swallowing the refresh error.
pub async fn prepare_request(url: &mut Url) -> Result<HeaderMap> {
    if !Settings::get().wings.enabled {
        return Ok(HeaderMap::new());
    }

    // Cheap gate: bail before doing anything else if the URL
    // host isn't an upstream we'd rewrite. (Cache-subdomain
    // hosts like `npm.<wings.host>` also fall through this
    // branch — those land here only if the user pre-rewrote
    // their URL by hand, which we still want to authenticate.)
    let Some(host) = url.host_str().map(str::to_owned) else {
        return Ok(HeaderMap::new());
    };
    let upstream_match = wings_url::is_upstream_origin(&host);
    let cache_match = wings_url::is_wings_cache_host(&host);
    if !upstream_match && !cache_match {
        return Ok(HeaderMap::new());
    }

    // Two ways to obtain an access token:
    //
    //   - **Dev** (interactive): load from
    //     `credentials.json` (populated by
    //     `mise wings login`), auto-refresh inside the
    //     leeway window using the rotated refresh token.
    //   - **CI** (GHA): exchange the runner's OIDC JWT for
    //     a short-lived wings session via `POST /auth`. No
    //     refresh token — each `mise` process re-mints
    //     once per invocation and caches the result for
    //     the duration of the process.
    //
    // The dev path takes priority if both apply (a
    // developer running `mise install` on their laptop
    // who happens to also have GHA env vars from some
    // local act-style emulator wants to use their own
    // credentials, not the runner's identity). CI is the
    // fallback when no on-disk credentials exist.
    let access_token = if let Some(creds) = credentials::cached() {
        if creds.should_refresh(REFRESH_LEEWAY_SECS) {
            match maybe_refresh(&creds).await {
                Ok(fresh) => fresh.access_token,
                Err(e) => {
                    log::warn!(
                        "wings: auto-refresh failed; falling back to \
                         original token. Error: {e:#}",
                    );
                    creds.access_token
                }
            }
        } else {
            creds.access_token
        }
    } else if crate::wings::ci::gha_runner_present() {
        // Negative result is cached as `None` so a non-
        // subscribed CI run doesn't keep retrying the
        // exchange on every cache request. See `wings::ci`
        // for the cache shape + log-level rationale.
        match crate::wings::ci::cached_ci_token().await {
            Some(token) => token,
            None => return Ok(HeaderMap::new()),
        }
    } else {
        // No dev credentials, no GHA runner → wings is
        // enabled but there's nothing to authenticate with.
        // Pass through unchanged; the proxy will 401 if the
        // user hand-rewrote the URL, otherwise the original
        // host is hit directly.
        return Ok(HeaderMap::new());
    };

    // Apply the URL rewrite for upstream origins. Cache-host
    // requests skip the rewrite (already pointing at the
    // wings subdomain).
    if upstream_match {
        wings_url::rewrite(url);
    }

    let mut headers = HeaderMap::new();
    let bearer = format!("Bearer {access_token}");
    if let Ok(value) = HeaderValue::from_str(&bearer) {
        // `from_str` shouldn't fail for a JWT (only ASCII), but
        // guard anyway: a malformed token shouldn't panic the
        // request — the proxy will 401 instead.
        headers.insert(AUTHORIZATION, value);
    }
    Ok(headers)
}

/// Apply the auto-refresh under [`credentials::lock_refresh`].
/// Re-checks `should_refresh` after acquiring the lock so
/// that the first holder does the rotation and subsequent
/// holders find a fresh token already in cache.
///
/// Critically, when this task *is* the first holder and
/// needs to call `client::refresh`, it uses the *latest*
/// cached credentials — not the stale snapshot the caller
/// passed in. Without this re-read, a task that waited on
/// the lock could end up POSTing the previous token to
/// `/auth/dev/refresh` after another task already rotated
/// it; the proxy's rotation tripwire would 401 the replay,
/// defeating the lock's purpose. Cursor Bugbot Medium
/// flagged the prior shape on PR review.
async fn maybe_refresh(stale: &credentials::Credentials) -> Result<credentials::Credentials> {
    if stale.refresh_token_expired() {
        // Refresh token itself is expired — no rotation can
        // save us. Surface as an error so the caller falls
        // back; the user's next interactive run sees a 401
        // and re-logs in.
        eyre::bail!(
            "wings refresh token expired ({}s ago); run `mise wings login`",
            crate::wings::now_unix() - stale.refresh_expires_at,
        );
    }
    let _guard = credentials::lock_refresh().await;

    // Re-read the cache under the lock. If a different task
    // in this process already rotated while we waited on
    // the lock, the cache holds the rotated tokens and we
    // can return immediately.
    //
    // Cross-process visibility is *not* covered: `LOAD_ONCE`
    // in `credentials.rs` only reads the on-disk file once
    // per process, so a separate `mise` process running
    // `mise wings login` in another terminal won't be seen
    // here. Two `mise install` runs racing on the same
    // refresh would both POST `/auth/dev/refresh` with the
    // same starting token; the second one trips the
    // proxy's rotation tripwire and 401s. That's a known
    // limitation; cross-process refresh coordination would
    // need a flock on the credentials file or similar.
    // Cursor Bugbot Low flagged the previous comment as
    // overstating the cache behavior.
    let current = credentials::cached().unwrap_or_else(|| stale.clone());
    if !current.should_refresh(REFRESH_LEEWAY_SECS) {
        return Ok(current);
    }
    if current.refresh_token_expired() {
        eyre::bail!(
            "wings refresh token expired ({}s ago); run `mise wings login`",
            crate::wings::now_unix() - current.refresh_expires_at,
        );
    }

    let next = client::refresh(&current).await?;
    credentials::store(next.clone())?;
    Ok(next)
}
