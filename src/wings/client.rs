//! Typed HTTP calls against the wings proxy's dev-auth surface.
//! Three endpoints, each a thin wrapper around the shared
//! `crate::http::HTTP` client with the right body shape and
//! error mapping.
//!
//! The proxy's response shapes are documented in
//! `proxy/src/routes/auth_dev.rs` (companion repo). Field names
//! mirror that side one-for-one — `serde(deny_unknown_fields)` is
//! deliberately *not* used here so a future field addition
//! (e.g. `seats_used`) doesn't break older mise binaries in the
//! field.

use eyre::Context;
use serde::{Deserialize, Serialize};

use crate::config::Settings;
use crate::result::Result;
use crate::wings::credentials::Credentials;

/// Build the apex API URL `https://api.<wings.host>/<path>`.
/// Centralized so a `wings.host` setting change re-routes every
/// call without per-callsite host stitching.
fn api_url(path: &str) -> String {
    let host = &Settings::get().wings.host;
    format!("https://api.{host}{path}")
}

/// Request body for `POST /auth/dev/refresh`. Single field —
/// the refresh token plaintext — sent over HTTPS.
#[derive(Serialize)]
struct RefreshRequest<'a> {
    refresh_token: &'a str,
}

/// Response shape for `POST /auth/dev` and `POST /auth/dev/refresh`.
/// Identical between the two endpoints by design — the client
/// flow stamps the same fields into [`Credentials`] either way.
#[derive(Deserialize)]
struct DevAuthResponse {
    /// Wings session JWT.
    token: String,
    /// Seconds until the access token's `exp`.
    expires_in: u64,
    /// Always `"Bearer"` from the proxy; we don't parse or
    /// validate, just stamp into the `Authorization` header.
    #[allow(dead_code)]
    token_type: String,
    /// Long-lived refresh token (rotated on every call).
    refresh_token: String,
    /// Seconds until the refresh token expires.
    refresh_expires_in: u64,
    /// Echoed identity. Not part of the proxy's `DevAuthResponse`
    /// today — we read it from the Clerk session JWT we sent in,
    /// not from the wings response. Left here as `Option` so a
    /// future proxy change that *does* echo identity can populate
    /// it without breaking older mise binaries.
    #[serde(default)]
    user_id: Option<String>,
    /// Same — optional today; will become the canonical identity
    /// echo when the proxy's response shape grows it.
    #[serde(default)]
    org: Option<String>,
}

/// Exchange a Clerk frontend session JWT for a wings session.
/// The Clerk JWT comes from the user's browser-side sign-in;
/// this is the only point on the CLI that talks to Clerk-
/// originated credentials.
///
/// ## Identity extraction
///
/// `user_id` and `org` aren't in the proxy's response yet — we
/// pull them from the Clerk JWT body (`sub` claim, and from the
/// proxy's stamped `org` claim by re-decoding the *returned*
/// wings JWT). The wings JWT body has `org` and `user_id`
/// fields; cheaper to read those than parse Clerk's claims a
/// second time. JWT bodies are unverified locally — the proxy
/// is the trust boundary; we just need the values for `whoami`.
pub async fn exchange_clerk_session(clerk_session_jwt: &str) -> Result<Credentials> {
    let url = api_url("/auth/dev");
    let mut headers = reqwest::header::HeaderMap::new();
    let auth = format!("Bearer {clerk_session_jwt}");
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&auth)
            .wrap_err("clerk session token contains invalid header characters")?,
    );

    let resp: DevAuthResponse = send_post_for(&url, &serde_json::json!({}), &headers).await?;

    let (user_id, org) = resp
        .user_id
        .clone()
        .zip(resp.org.clone())
        .unwrap_or_else(|| extract_identity_from_wings_jwt(&resp.token));

    Credentials::from_dev_auth(
        Settings::get().wings.host.clone(),
        resp.token,
        resp.expires_in,
        resp.refresh_token,
        resp.refresh_expires_in,
        user_id,
        org,
    )
}

/// Rotate the refresh token. Called by the auto-refresh path
/// when the access token's `exp` is inside the leeway window.
/// On 401 (refresh token revoked / expired / replayed), the
/// caller's responsibility is to clear local credentials and
/// surface "re-login required".
pub async fn refresh(creds: &Credentials) -> Result<Credentials> {
    let url = api_url("/auth/dev/refresh");
    let body = RefreshRequest {
        refresh_token: &creds.refresh_token,
    };
    let resp: DevAuthResponse =
        send_post_for::<_, DevAuthResponse>(&url, &body, &reqwest::header::HeaderMap::new())
            .await
            .wrap_err("wings refresh failed")?;

    let mut next = creds.clone();
    next.apply_refresh(
        resp.token,
        resp.expires_in,
        resp.refresh_token,
        resp.refresh_expires_in,
    );
    Ok(next)
}

/// Revoke every active wings session for the calling user.
/// `clerk_session_jwt` re-authenticates the call (the route
/// uses Clerk session, not the wings session, so that a
/// compromised wings session can't revoke itself in a way
/// that locks out the legitimate user). On HTTP error, the
/// caller still deletes local credentials — better to be
/// "logged out locally with possibly-still-active server
/// state" than "stuck logged in because the server was
/// briefly unreachable".
pub async fn revoke(clerk_session_jwt: &str) -> Result<()> {
    let url = api_url("/auth/dev/revoke");
    let mut headers = reqwest::header::HeaderMap::new();
    let auth = format!("Bearer {clerk_session_jwt}");
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&auth)
            .wrap_err("clerk session token contains invalid header characters")?,
    );
    let client = reqwest::Client::builder()
        .timeout(Settings::get().http_timeout())
        .user_agent(format!("mise/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .wrap_err("building HTTP client for wings revoke")?;
    let resp = client
        .post(&url)
        .headers(headers)
        .json(&serde_json::json!({}))
        .send()
        .await
        .wrap_err_with(|| format!("POST {url}"))?;
    // Proxy returns 204 No Content on success. Treat any 2xx
    // as a clean revoke; a 401 here typically means the Clerk
    // session has already lapsed (the user is effectively
    // logged out as far as Clerk is concerned), and the local
    // credentials still need to be cleared by the caller —
    // surface as an error so the caller logs it but proceeds.
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eyre::bail!("wings revoke returned {status}: {body}");
    }
    Ok(())
}

/// Decode the wings session JWT and pull the (`user_id`,
/// `org`) pair out of its body. Best-effort — a JWT shape we
/// can't parse falls back to placeholder strings; the user
/// sees them in `whoami` and knows something's off, but the
/// access path still works (the proxy verifies the JWT
/// authoritatively).
fn extract_identity_from_wings_jwt(jwt: &str) -> (String, String) {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let body_b64 = jwt.split('.').nth(1).unwrap_or("");
    let Ok(body) = URL_SAFE_NO_PAD.decode(body_b64) else {
        return ("(unknown)".into(), "(unknown)".into());
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return ("(unknown)".into(), "(unknown)".into());
    };
    let user_id = v
        .get("user_id")
        .and_then(|x| x.as_str())
        .unwrap_or("(unknown)")
        .to_string();
    let org = v
        .get("org")
        .and_then(|x| x.as_str())
        .unwrap_or("(unknown)")
        .to_string();
    (user_id, org)
}

/// POST a JSON body and decode the response body as `R`.
/// Wraps reqwest directly because mise's existing
/// `HTTP.post_json_with_headers` returns a `Result<bool>` —
/// which is fine for "fire and forget" but doesn't expose
/// the response body we need for the auth flows.
async fn send_post_for<B: Serialize, R: for<'de> Deserialize<'de>>(
    url: &str,
    body: &B,
    headers: &reqwest::header::HeaderMap,
) -> Result<R> {
    // Build a fresh reqwest::Client tuned with the same
    // timeout settings the shared HTTP uses, then drive the
    // round-trip ourselves so we can decode the response body
    // as a typed value. The shared `crate::http::HTTP.reqwest`
    // is private; falling back to a per-call client is
    // acceptable because dev-auth endpoints fire ≤ a handful
    // of times per CLI invocation, never on the cache hot
    // path.
    let client = reqwest::Client::builder()
        .timeout(Settings::get().http_timeout())
        .user_agent(format!("mise/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .wrap_err("building HTTP client for wings auth")?;
    let resp = client
        .post(url)
        .headers(headers.clone())
        .json(body)
        .send()
        .await
        .wrap_err_with(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eyre::bail!("wings {url} returned {status}: {body}");
    }
    let parsed: R = resp
        .json()
        .await
        .wrap_err_with(|| format!("decoding {url} response body"))?;
    Ok(parsed)
}
