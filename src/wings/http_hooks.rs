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
//!     Returns `Ok(None)` for the no-op case (wings disabled,
//!     no credentials, or URL host isn't an upstream we
//!     rewrite). The HTTP layer then proceeds as if wings
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
    let upstream_match = is_upstream_origin(&host);
    let cache_match = wings_url::is_wings_cache_host(&host);
    if !upstream_match && !cache_match {
        return Ok(HeaderMap::new());
    }

    // Need credentials. If `cached()` returns `None` here
    // (user hasn't logged in), we leave the request alone —
    // the upstream URL is unmodified, no Authorization
    // header is added, and the request proceeds as if wings
    // weren't enabled. The user sees the normal failure path
    // (e.g. the cache subdomain returns 401 / 404 if they
    // hand-rewrote the URL without logging in).
    let Some(creds) = credentials::cached() else {
        return Ok(HeaderMap::new());
    };

    // Auto-refresh path: if the access token is close to
    // expiry, take the refresh-coordination lock and rotate.
    // The loser of the lock race re-reads the cache and
    // typically finds a fresh token already swapped in.
    let creds = if creds.should_refresh(REFRESH_LEEWAY_SECS) {
        match maybe_refresh(&creds).await {
            Ok(fresh) => fresh,
            Err(e) => {
                log::warn!(
                    "wings: auto-refresh failed; falling back to original token. \
                     Error: {e:#}",
                );
                creds
            }
        }
    } else {
        creds
    };

    // Apply the URL rewrite for upstream origins. Cache-host
    // requests skip the rewrite (already pointing at the
    // wings subdomain).
    if upstream_match {
        wings_url::rewrite(url, /* creds_present */ true);
    }

    let mut headers = HeaderMap::new();
    let bearer = format!("Bearer {}", creds.access_token);
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
async fn maybe_refresh(stale: &credentials::Credentials) -> Result<credentials::Credentials> {
    if stale.refresh_token_expired() {
        // Refresh token itself is expired — no rotation can
        // save us. Surface as an error so the caller falls
        // back; the user's next interactive run sees a 401
        // and re-logs in.
        eyre::bail!(
            "wings refresh token expired ({}s ago); run `mise wings login`",
            now_unix() - stale.refresh_expires_at,
        );
    }
    let _guard = credentials::lock_refresh().await;

    // Double-check after acquiring the lock. A different
    // task may have rotated under us while we waited.
    if let Some(fresh) = credentials::cached()
        && !fresh.should_refresh(REFRESH_LEEWAY_SECS)
    {
        return Ok(fresh);
    }

    let next = client::refresh(stale).await?;
    credentials::store(next.clone())?;
    Ok(next)
}

/// True iff `host` is one of the upstream origins the wings
/// rewriter knows about. Mirrors the match arms in
/// [`crate::wings::url::rewrite`]; a future origin added
/// there must also land here.
fn is_upstream_origin(host: &str) -> bool {
    matches!(
        host,
        "registry.npmjs.org" | "github.com" | "objects.githubusercontent.com" | "api.github.com"
    )
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_origin_set_pins_the_match_arms() {
        assert!(is_upstream_origin("registry.npmjs.org"));
        assert!(is_upstream_origin("github.com"));
        assert!(is_upstream_origin("api.github.com"));
        assert!(is_upstream_origin("objects.githubusercontent.com"));

        assert!(!is_upstream_origin("registry.npmjs.org.evil.com"));
        assert!(!is_upstream_origin("subdomain.github.com"));
        assert!(!is_upstream_origin("example.com"));
        assert!(!is_upstream_origin(""));
    }
}
