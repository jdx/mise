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
//! process and cache the result in memory
//! for the rest of the process's lifetime.
//!
//! For long-running `mise` processes (the daemon a future
//! `mise hook` mode might spawn), a periodic re-mint will be
//! needed; the cache here is fine for the typical
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
//! in-memory cache here keeps the two paths out of each
//! other's way.

use std::env;

use eyre::{Context, Report, eyre};
use reqwest::{Response, StatusCode};
use serde::Deserialize;

/// GHA exposes two env vars to fetch the runner's OIDC
/// token: a one-time URL to GET, and a Bearer secret. Both
/// must be present; if either is missing, this is not an
/// OIDC-enabled run (the workflow forgot
/// `permissions: id-token: write`, or this isn't GHA at all).
const ID_TOKEN_REQUEST_URL_ENV: &str = "ACTIONS_ID_TOKEN_REQUEST_URL";
const ID_TOKEN_REQUEST_TOKEN_ENV: &str = "ACTIONS_ID_TOKEN_REQUEST_TOKEN";

/// Lazy-initialized cache for the CI session JWT.
///
/// `Token` if the OIDC → `/auth` exchange succeeded.
/// `Unavailable` if it failed permanently (for example,
/// missing subscription or unauthorized audience). Permanent
/// negative caching keeps non-subscribed repositories from
/// retrying the exchange for every install.
///
/// Transient network/rate-limit/server failures are not cached,
/// so a later install in the same process can recover.
static CI_TOKEN: tokio::sync::Mutex<Option<CiTokenCache>> = tokio::sync::Mutex::const_new(None);

#[derive(Clone)]
enum CiTokenCache {
    Token(String),
    Unavailable,
}

#[derive(Debug)]
struct CiAuthError {
    report: Report,
    transient: bool,
}

impl CiAuthError {
    fn permanent(report: impl Into<Report>) -> Self {
        Self {
            report: report.into(),
            transient: false,
        }
    }

    fn transient(report: impl Into<Report>) -> Self {
        Self {
            report: report.into(),
            transient: true,
        }
    }
}

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
/// `None` covers unavailable credentials. Permanent failures
/// are cached; transient failures are logged but left
/// retryable for later tools in the same process.
/// The HTTP hook treats `None` the same as "no credentials"
/// and passes the request through unchanged.
pub async fn cached_ci_token() -> Option<String> {
    let mut cache = CI_TOKEN.lock().await;
    match cache.as_ref() {
        Some(CiTokenCache::Token(token)) => return Some(token.clone()),
        Some(CiTokenCache::Unavailable) => return None,
        None => {}
    }

    match exchange_runner_oidc().await {
        Ok(token) => {
            *cache = Some(CiTokenCache::Token(token.clone()));
            Some(token)
        }
        Err(e) if e.transient => {
            log::warn!(
                "wings: transient CI OIDC exchange failed (will retry if needed): {:#}",
                e.report
            );
            None
        }
        Err(e) => {
            log::warn!(
                "wings: CI OIDC exchange failed (proceeding without wings): {:#}",
                e.report
            );
            *cache = Some(CiTokenCache::Unavailable);
            None
        }
    }
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
async fn exchange_runner_oidc() -> std::result::Result<String, CiAuthError> {
    let request_url = env::var(ID_TOKEN_REQUEST_URL_ENV)
        .wrap_err_with(|| format!("env var {ID_TOKEN_REQUEST_URL_ENV} not set"))
        .map_err(CiAuthError::permanent)?;
    let request_token = env::var(ID_TOKEN_REQUEST_TOKEN_ENV)
        .wrap_err_with(|| format!("env var {ID_TOKEN_REQUEST_TOKEN_ENV} not set"))
        .map_err(CiAuthError::permanent)?;
    let host = crate::wings::host();

    let client = crate::wings::client::http_client().map_err(CiAuthError::permanent)?;

    // Step 1: fetch the runner's OIDC token, scoped to the
    // wings audience. GHA returns `{value: "<jwt>"}`.
    let mut oidc_url = url::Url::parse(&request_url)
        .wrap_err("parsing GHA OIDC request URL")
        .map_err(CiAuthError::permanent)?;
    oidc_url.query_pairs_mut().append_pair("audience", host);
    #[derive(Deserialize)]
    struct OidcEnvelope {
        value: String,
    }
    let oidc_resp = client
        .get(oidc_url)
        .bearer_auth(&request_token)
        .send()
        .await
        .map_err(|e| classify_request_error(e, "fetching GHA OIDC token"))?;
    let oidc: OidcEnvelope = checked_response(oidc_resp, "GHA OIDC issuer")
        .await?
        .json()
        .await
        .wrap_err("decoding GHA OIDC response")
        .map_err(CiAuthError::permanent)?;

    // Step 2: exchange at the wings proxy.
    #[derive(Deserialize)]
    struct AuthResponse {
        token: String,
        // Other fields (expires_in, token_type) intentionally
        // ignored — the CI cache is process-scoped, so the
        // token's exp doesn't drive any local refresh
        // decision. The proxy will 401 a stale token; callers
        // surface that as a wings request failure.
    }
    let exchange_url = format!("https://api.{host}/auth");
    let auth_resp = client
        .post(&exchange_url)
        .bearer_auth(&oidc.value)
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|e| classify_request_error(e, format!("POST {exchange_url}")))?;
    let auth: AuthResponse = checked_response(auth_resp, format!("wings {exchange_url}"))
        .await?
        .json()
        .await
        .wrap_err("decoding wings /auth response")
        .map_err(CiAuthError::permanent)?;

    log::debug!(
        "wings: minted CI session via GHA OIDC ({} chars)",
        auth.token.len()
    );
    Ok(auth.token)
}

async fn checked_response(
    resp: Response,
    label: impl AsRef<str>,
) -> std::result::Result<Response, CiAuthError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let report = eyre!("{} returned {status}: {body}", label.as_ref());
    if is_transient_auth_status(status) {
        Err(CiAuthError::transient(report))
    } else {
        Err(CiAuthError::permanent(report))
    }
}

fn classify_request_error(err: reqwest::Error, label: impl AsRef<str>) -> CiAuthError {
    let transient = is_transient_request_error(&err);
    let report = Report::new(err).wrap_err(label.as_ref().to_string());
    if transient {
        CiAuthError::transient(report)
    } else {
        CiAuthError::permanent(report)
    }
}

fn is_transient_request_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect()
}

fn is_transient_auth_status(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
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

    #[test]
    fn ci_auth_transient_statuses_are_not_negative_cached() {
        assert!(is_transient_auth_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_transient_auth_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!is_transient_auth_status(StatusCode::PAYMENT_REQUIRED));
        assert!(!is_transient_auth_status(StatusCode::UNAUTHORIZED));
    }
}
