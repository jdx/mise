//! CI authentication path for `mise wings`. When mise is run
//! from a GitHub Actions runner with `wings.enabled = true`
//! and **no on-disk dev credentials**, this module fetches
//! the runner's OIDC token and exchanges it for a wings
//! session JWT via the proxy's `POST /auth` route.
//!
//! ## Why no `mise wings login` for CI
//!
//! GHA runners already have a verified machine identity
//! (the OIDC token, signed by GitHub's issuer with
//! `repository`, `repository_owner`, `actor`, etc. claims).
//! The proxy's `/auth` route accepts that JWT directly and
//! mints a wings CI session. No interactive login. No
//! long-lived secret to rotate. No `mise wings login` step
//! in workflow YAML. The user opts in by setting
//! `MISE_WINGS_ENABLED=1` (or `wings.enabled = true` in a
//! committed `mise.toml`); everything else is automatic.
//!
//! ## Token lifetime
//!
//! CI sessions are short-lived and don't carry a refresh
//! token (the proxy's `DevAuthResponse` returns
//! `refresh_token` for `/auth/dev`, but the CI `/auth` route
//! returns access only — re-minting from OIDC is cheap and
//! the OIDC token itself is short-lived enough that we'd
//! re-authenticate anyway). We exchange once per `mise`
//! process and cache the result in a `tokio::sync::OnceCell`
//! for the rest of the process's lifetime.
//!
//! For long-running `mise` processes (the daemon a future
//! `mise hook` mode might spawn), a periodic re-mint will be
//! needed; the `OnceCell` here is fine for the typical
//! one-shot CLI invocation pattern. Worst case today: a
//! single `mise install` run that takes longer than the
//! session TTL (~6 h default) hits a 401 on the last
//! download; the user retries the run. Acceptable for v1.
//!
//! ## Why cache in this module
//!
//! `credentials::CACHE` is the dev-side store backed by a
//! `RwLock<Option<Credentials>>` and a JSON file on disk.
//! The CI token has no business living in either: not on
//! disk (it's per-process), and the dev cache's shape
//! (refresh token, schema versioning, identity fields) is
//! all overhead a CI token doesn't need. A separate
//! in-memory `OnceCell<String>` here keeps the two paths
//! out of each other's way.

use std::env;

use eyre::{Context, Result};
use serde::Deserialize;

use crate::config::Settings;

/// GHA exposes two env vars to fetch the runner's OIDC
/// token: a one-time URL to GET, and a Bearer secret. Both
/// must be present; if either is missing, this is not an
/// OIDC-enabled run (the workflow forgot
/// `permissions: id-token: write`, or this isn't GHA at all).
const ID_TOKEN_REQUEST_URL_ENV: &str = "ACTIONS_ID_TOKEN_REQUEST_URL";
const ID_TOKEN_REQUEST_TOKEN_ENV: &str = "ACTIONS_ID_TOKEN_REQUEST_TOKEN";

/// Lazy-initialized cache for the CI session JWT. Computed
/// on first access; reused for the rest of the process. The
/// `OnceCell` form coordinates concurrent first-callers
/// (only one wins the exchange; the others await its result).
static CI_TOKEN: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();

/// True iff the GHA OIDC env vars are both set. The HTTP
/// hook calls this *before* attempting an exchange so the
/// not-on-CI case doesn't even build a `reqwest::Client`.
pub fn gha_runner_present() -> bool {
    env::var_os(ID_TOKEN_REQUEST_URL_ENV).is_some()
        && env::var_os(ID_TOKEN_REQUEST_TOKEN_ENV).is_some()
}

/// Get the wings CI session JWT, exchanging the runner's
/// OIDC token for one on first call. Subsequent callers
/// share the cached value.
///
/// Returns `Err` on any failure in the exchange chain
/// (OIDC fetch, proxy POST, JSON decode). The HTTP-hook
/// caller logs the error and falls through to no-auth —
/// the wings cache will return whatever it returns to an
/// unauthenticated request, which is the same thing a
/// user with `wings.enabled = true` and no creds gets in
/// the dev path. Best-effort, fail-open.
pub async fn cached_ci_token() -> Result<String> {
    CI_TOKEN
        .get_or_try_init(exchange_runner_oidc)
        .await
        .cloned()
}

/// Run the OIDC → wings session exchange end-to-end. Two
/// HTTP calls:
///
///   1. `GET <ACTIONS_ID_TOKEN_REQUEST_URL>?audience=<host>`
///      with `Authorization: Bearer
///      <ACTIONS_ID_TOKEN_REQUEST_TOKEN>` → `{value: <oidc-jwt>}`
///   2. `POST https://api.<wings.host>/auth` with
///      `Authorization: Bearer <oidc-jwt>` and an empty body
///      → `{token, expires_in, token_type}`
///
/// The OIDC fetch's `audience` query parameter must match
/// the proxy's `EXPECTED_AUDIENCE` env var, which is set to
/// `wings.host`. Wiring it from settings means a
/// staging/private deployment (`wings.host =
/// mise-wings-staging.en.dev`) just works without further
/// config.
async fn exchange_runner_oidc() -> Result<String> {
    let request_url = env::var(ID_TOKEN_REQUEST_URL_ENV)
        .wrap_err_with(|| format!("env var {ID_TOKEN_REQUEST_URL_ENV} not set"))?;
    let request_token = env::var(ID_TOKEN_REQUEST_TOKEN_ENV)
        .wrap_err_with(|| format!("env var {ID_TOKEN_REQUEST_TOKEN_ENV} not set"))?;
    let host = Settings::get().wings.host.clone();
    if host.is_empty() {
        eyre::bail!("wings.host is empty — set MISE_WINGS_HOST or unset MISE_WINGS_ENABLED");
    }

    let client = reqwest::Client::builder()
        .timeout(Settings::get().http_timeout())
        .user_agent(format!("mise/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .wrap_err("building HTTP client for wings CI auth")?;

    // Step 1: fetch the runner's OIDC token, scoped to the
    // wings audience. GHA returns `{value: "<jwt>"}`.
    let oidc_url = if request_url.contains('?') {
        format!("{request_url}&audience={host}")
    } else {
        format!("{request_url}?audience={host}")
    };
    #[derive(Deserialize)]
    struct OidcEnvelope {
        value: String,
    }
    let oidc: OidcEnvelope = client
        .get(&oidc_url)
        .bearer_auth(&request_token)
        .send()
        .await
        .wrap_err("fetching GHA OIDC token")?
        .error_for_status()
        .wrap_err("GHA OIDC issuer returned non-2xx")?
        .json()
        .await
        .wrap_err("decoding GHA OIDC response")?;

    // Step 2: exchange at the wings proxy.
    #[derive(Deserialize)]
    struct AuthResponse {
        token: String,
        // Other fields (expires_in, token_type) intentionally
        // ignored — the CI cache is process-scoped, so the
        // token's exp doesn't drive any local refresh
        // decision. The proxy will 401 a stale token; the
        // hook caller logs that and falls through.
    }
    let exchange_url = format!("https://api.{host}/auth");
    let auth: AuthResponse = client
        .post(&exchange_url)
        .bearer_auth(&oidc.value)
        .json(&serde_json::json!({}))
        .send()
        .await
        .wrap_err_with(|| format!("POST {exchange_url}"))?
        .error_for_status()
        .wrap_err_with(|| format!("wings {exchange_url} returned non-2xx"))?
        .json()
        .await
        .wrap_err("decoding wings /auth response")?;

    log::debug!(
        "wings: minted CI session via GHA OIDC ({} chars)",
        auth.token.len()
    );
    Ok(auth.token)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `gha_runner_present` is the cheap branch the hot path
    /// uses to decide whether to even attempt the exchange.
    /// Pin its semantics: both env vars set → true; either
    /// one missing → false.
    #[test]
    fn gha_runner_present_requires_both_env_vars() {
        // Test in isolation by snapshotting + restoring the
        // env. `unsafe` is required because env-var mutation
        // is process-wide; the test pre-condition + restore
        // ensures parallel tests don't see each other's
        // state.
        let saved_url = env::var_os(ID_TOKEN_REQUEST_URL_ENV);
        let saved_tok = env::var_os(ID_TOKEN_REQUEST_TOKEN_ENV);

        unsafe {
            env::remove_var(ID_TOKEN_REQUEST_URL_ENV);
            env::remove_var(ID_TOKEN_REQUEST_TOKEN_ENV);
        }
        assert!(!gha_runner_present(), "neither set");

        unsafe {
            env::set_var(ID_TOKEN_REQUEST_URL_ENV, "https://example.invalid/token");
        }
        assert!(!gha_runner_present(), "url only");

        unsafe {
            env::set_var(ID_TOKEN_REQUEST_TOKEN_ENV, "secret");
        }
        assert!(gha_runner_present(), "both set");

        unsafe {
            env::remove_var(ID_TOKEN_REQUEST_URL_ENV);
        }
        assert!(!gha_runner_present(), "token only");

        // Restore.
        unsafe {
            match saved_url {
                Some(v) => env::set_var(ID_TOKEN_REQUEST_URL_ENV, v),
                None => env::remove_var(ID_TOKEN_REQUEST_URL_ENV),
            }
            match saved_tok {
                Some(v) => env::set_var(ID_TOKEN_REQUEST_TOKEN_ENV, v),
                None => env::remove_var(ID_TOKEN_REQUEST_TOKEN_ENV),
            }
        }
    }
}
