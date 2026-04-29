//! Origin → wings cache subdomain URL rewriting.
//!
//! When `wings.enabled = true` and credentials are present,
//! mise transparently routes asset fetches through the wings
//! cache subdomains. The mapping is host-only — paths and
//! queries pass through verbatim — and applies to a fixed set
//! of upstream origins:
//!
//! | Upstream                    | Wings cache subdomain      | Use case                  |
//! |-----------------------------|----------------------------|---------------------------|
//! | `registry.npmjs.org`        | `npm.<wings.host>`         | npm tarballs              |
//! | `github.com/.../releases/…` | `gh.<wings.host>`          | GitHub release artifacts  |
//! | `api.github.com`            | `gh-api.<wings.host>`      | GitHub API metadata       |
//! | `objects.githubusercontent` | `gh.<wings.host>`          | GitHub release blob CDN   |
//!
//! Anything not in this set is left alone — the rewriter is
//! deliberately conservative. A user pinning a Java
//! distribution from `download.oracle.com`, say, keeps hitting
//! Oracle directly; routing that through wings would require
//! an explicit allowlist entry on the proxy side.
//!
//! ## "rewrite" vs "replace"
//!
//! mise already has a generic `url_replacements` setting —
//! arbitrary string-or-regex substitutions on outbound URLs.
//! That feature stays as-is; the wings rewriter is a separate
//! gate so users opting into wings don't have to write 4
//! replacement entries by hand, and so the gate respects the
//! "credentials present" half of the activation logic that a
//! plain string replacement can't express.

use url::Url;

use crate::config::Settings;

/// Cache subdomain prefixes that the proxy serves on
/// `<prefix>.<wings.host>`. Kept in code rather than driven
/// from the proxy so a misconfigured wings host doesn't
/// silently misroute requests — this list pins the contract.
const NPM_PREFIX: &str = "npm";
const GH_PREFIX: &str = "gh";
const GH_API_PREFIX: &str = "gh-api";

/// Apex hosts the rewriter knows how to redirect. A URL
/// whose host isn't in this set passes through unchanged.
const NPM_ORIGIN: &str = "registry.npmjs.org";
const GH_ORIGIN: &str = "github.com";
const GH_BLOB_ORIGIN: &str = "objects.githubusercontent.com";
const GH_API_ORIGIN: &str = "api.github.com";

/// Single source of truth for "is this host one of the
/// upstream origins wings knows how to rewrite?". Both this
/// module's [`rewrite`] match arms and `http_hooks`'s gate
/// check pull from this set, so an origin added here flows
/// through to both call sites without a separate edit.
/// Cursor Bugbot flagged the previous duplicate hardcoding
/// across the two modules on PR review.
pub const UPSTREAM_ORIGINS: &[&str] = &[NPM_ORIGIN, GH_ORIGIN, GH_BLOB_ORIGIN, GH_API_ORIGIN];

/// True iff `host` is one of [`UPSTREAM_ORIGINS`].
pub fn is_upstream_origin(host: &str) -> bool {
    UPSTREAM_ORIGINS.contains(&host)
}

/// True iff `host` is one of the wings cache subdomains for
/// the configured wings deployment. Used by the HTTP layer
/// on every outbound request to decide whether to attach
/// the wings Bearer token, so the implementation is
/// allocation-free: strip the apex suffix + the `.`
/// separator, then check the leftover prefix against the
/// static set.
///
/// Gemini flagged the previous shape — three `format!`
/// allocations per call — as a perf hit on `mise install`
/// runs that fetch many tarballs.
pub fn is_wings_cache_host(host: &str) -> bool {
    let apex = crate::wings::host();
    host.strip_suffix(apex)
        .and_then(|s| s.strip_suffix('.'))
        .is_some_and(|prefix| matches!(prefix, NPM_PREFIX | GH_PREFIX | GH_API_PREFIX))
}

/// Rewrite `url` in place to point at the appropriate wings
/// cache subdomain. The caller (`http_hooks::prepare_request`)
/// has already verified the activation conditions (settings
/// not opted-out, credentials available, host is an upstream
/// origin); this function just performs the host swap.
///
/// Returns `true` iff a rewrite was applied (the HTTP
/// layer logs that for debugging). A `Url` whose scheme
/// somehow rejects the new host (shouldn't happen in
/// practice — `<label>.<apex>` is always a valid hostname)
/// returns `false` and a warn log.
pub fn rewrite(url: &mut Url) -> bool {
    if !Settings::get().wings.enabled {
        return false;
    }
    let apex = crate::wings::host();

    let Some(host) = url.host_str().map(str::to_owned) else {
        return false;
    };

    let new_host = match host.as_str() {
        NPM_ORIGIN => format!("{NPM_PREFIX}.{apex}"),
        GH_ORIGIN | GH_BLOB_ORIGIN => format!("{GH_PREFIX}.{apex}"),
        GH_API_ORIGIN => format!("{GH_API_PREFIX}.{apex}"),
        _ => return false,
    };

    if url.set_host(Some(&new_host)).is_err() {
        // Shouldn't happen — `new_host` is always a valid
        // hostname of the form `<label>.<apex>`. If it does,
        // log + leave the URL alone rather than panicking.
        log::warn!("wings url rewrite: failed to set host to {new_host:?}");
        return false;
    }
    log::debug!("wings: rewrote {host} → {new_host}");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    use confique::Layer;

    static TEST_SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_wings_enabled<F>(enabled: bool, test_fn: F)
    where
        F: FnOnce(),
    {
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.wings.enabled = Some(enabled);
        crate::config::Settings::reset(Some(settings));
        test_fn();
        crate::config::Settings::reset(None);
    }

    #[test]
    fn no_rewrite_by_default() {
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();
        crate::config::Settings::reset(None);

        let mut url = Url::parse("https://registry.npmjs.org/lodash").unwrap();
        let rewrote = rewrite(&mut url);

        assert!(!rewrote);
        assert_eq!(url.host_str(), Some("registry.npmjs.org"));
        crate::config::Settings::reset(None);
    }

    #[test]
    fn rewrites_npm_registry_only_when_enabled() {
        with_wings_enabled(true, || {
            let mut url = Url::parse("https://registry.npmjs.org/lodash").unwrap();
            let rewrote = rewrite(&mut url);

            assert!(rewrote);
            let expected = format!("{NPM_PREFIX}.{}", crate::wings::host());
            assert_eq!(url.host_str(), Some(expected.as_str()));
        });
    }

    #[test]
    fn no_rewrite_when_explicitly_disabled() {
        with_wings_enabled(false, || {
            let mut url = Url::parse("https://registry.npmjs.org/lodash").unwrap();
            let rewrote = rewrite(&mut url);

            assert!(!rewrote);
            assert_eq!(url.host_str(), Some("registry.npmjs.org"));
        });
    }

    #[test]
    fn no_rewrite_for_unknown_origin() {
        with_wings_enabled(true, || {
            let mut url = Url::parse("https://example.com/x").unwrap();
            let rewrote = rewrite(&mut url);
            assert!(!rewrote);
            assert_eq!(url.host_str(), Some("example.com"));
        });
    }

    #[test]
    fn no_rewrite_when_url_has_no_host() {
        with_wings_enabled(true, || {
            // file:// URLs have no host — the rewriter should
            // bail rather than panic.
            let mut url = Url::parse("file:///tmp/x").unwrap();
            let rewrote = rewrite(&mut url);
            assert!(!rewrote);
        });
    }

    #[test]
    fn cache_host_detection_pins_subdomain_set() {
        let apex = crate::wings::host();
        assert!(is_wings_cache_host(&format!("npm.{apex}")));
        assert!(is_wings_cache_host(&format!("gh.{apex}")));
        assert!(is_wings_cache_host(&format!("gh-api.{apex}")));
        assert!(!is_wings_cache_host(&format!("api.{apex}")));
        assert!(!is_wings_cache_host(&format!("app.{apex}")));
        assert!(!is_wings_cache_host("npm.somewhere-else.dev"));
    }
}
