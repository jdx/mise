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

use std::sync::OnceLock;

use eyre::Context;
use serde::{Deserialize, Serialize};

use crate::result::Result;
use crate::wings::{
    credentials::{Credentials, DevAuthCredentials},
    device::DeviceKey,
};

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Build the apex API URL `https://api.<wings-host>/<path>`.
/// Centralized so the `wings.staging` toggle re-routes every
/// call without per-callsite host stitching.
fn api_url(path: &str) -> String {
    let host = crate::wings::host();
    format!("https://api.{host}{path}")
}

/// Request body for `POST /auth/dev/refresh`. Single field —
/// the refresh token plaintext — sent over HTTPS.
#[derive(Serialize)]
struct RefreshRequest<'a> {
    refresh_token: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    challenge: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
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
    #[serde(default)]
    device_id: Option<String>,
}

#[derive(Serialize)]
struct DeviceStartRequest<'a> {
    public_key: &'a str,
    key_kind: &'a str,
    hardware_backed: bool,
    device_label: String,
    os: String,
    arch: String,
}

#[derive(Deserialize)]
pub struct DeviceStartResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Serialize)]
struct DevicePollRequest<'a> {
    device_code: &'a str,
}

#[derive(Serialize)]
struct DeviceChallengeRequest<'a> {
    device_id: &'a str,
}

#[derive(Deserialize)]
struct DeviceChallengeResponse {
    challenge: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
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

    let resp: DevAuthResponse = post_json(&url, &serde_json::json!({}), &headers).await?;

    let (user_id, org) = resp
        .user_id
        .clone()
        .zip(resp.org.clone())
        .unwrap_or_else(|| extract_identity_from_wings_jwt(&resp.token));

    Credentials::from_dev_auth(DevAuthCredentials {
        host: crate::wings::host().to_string(),
        access_token: resp.token,
        expires_in: resp.expires_in,
        refresh_token: resp.refresh_token,
        refresh_expires_in: resp.refresh_expires_in,
        user_id,
        org,
        device_id: resp.device_id,
    })
}

pub async fn start_device_login(key: &DeviceKey) -> Result<DeviceStartResponse> {
    let url = api_url("/auth/dev/device/start");
    let public_key = key.public_key_base64()?;
    let body = DeviceStartRequest {
        public_key: &public_key,
        key_kind: key.key_kind(),
        hardware_backed: key.hardware_backed(),
        device_label: hostname_label(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    };
    post_json(&url, &body, &reqwest::header::HeaderMap::new()).await
}

pub async fn poll_device_login(device_code: &str) -> Result<Option<Credentials>> {
    let url = api_url("/auth/dev/device/token");
    let body = DevicePollRequest { device_code };
    let resp = http_client()?
        .post(&url)
        .json(&body)
        .send()
        .await
        .wrap_err_with(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if serde_json::from_str::<ErrorResponse>(&body)
            .map(|e| e.error == "authorization_pending")
            .unwrap_or(false)
        {
            return Ok(None);
        }
        eyre::bail!("wings {url} returned {status}: {body}");
    }
    let resp: DevAuthResponse = resp
        .json()
        .await
        .wrap_err_with(|| format!("decoding {url} response body"))?;
    let (user_id, org) = resp
        .user_id
        .clone()
        .zip(resp.org.clone())
        .unwrap_or_else(|| extract_identity_from_wings_jwt(&resp.token));
    Ok(Some(Credentials::from_dev_auth(DevAuthCredentials {
        host: crate::wings::host().to_string(),
        access_token: resp.token,
        expires_in: resp.expires_in,
        refresh_token: resp.refresh_token,
        refresh_expires_in: resp.refresh_expires_in,
        user_id,
        org,
        device_id: resp.device_id,
    })?))
}

/// Rotate the refresh token. Called by the auto-refresh path
/// when the access token's `exp` is inside the leeway window.
/// On 401 (refresh token revoked / expired / replayed), the
/// caller's responsibility is to clear local credentials and
/// surface "re-login required".
pub async fn refresh(creds: &Credentials) -> Result<Credentials> {
    let url = api_url("/auth/dev/refresh");
    let mut challenge = None;
    let mut signature = None;
    if let Some(device_id) = creds.device_id.as_deref() {
        let key = DeviceKey::load_for_current_host()?
            .ok_or_else(|| eyre::eyre!("wings device key missing; run `mise wings login`"))?;
        let c = device_challenge(device_id).await?;
        signature = Some(key.sign_challenge(device_id, &c)?);
        challenge = Some(c);
    }
    let body = RefreshRequest {
        refresh_token: &creds.refresh_token,
        device_id: creds.device_id.as_deref(),
        challenge: challenge.as_deref(),
        signature,
    };
    let resp: DevAuthResponse =
        post_json::<_, DevAuthResponse>(&url, &body, &reqwest::header::HeaderMap::new())
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

async fn device_challenge(device_id: &str) -> Result<String> {
    let url = api_url("/auth/dev/device/challenge");
    let body = DeviceChallengeRequest { device_id };
    let resp: DeviceChallengeResponse =
        post_json(&url, &body, &reqwest::header::HeaderMap::new()).await?;
    Ok(resp.challenge)
}

fn hostname_label() -> String {
    sys_info::hostname().unwrap_or_else(|_| "unknown".into())
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
    let resp = http_client()?
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
pub(crate) fn http_client() -> Result<reqwest::Client> {
    if let Some(client) = HTTP_CLIENT.get() {
        return Ok(client.clone());
    }

    let client = reqwest::Client::builder()
        .timeout(crate::config::Settings::get().http_timeout())
        .user_agent(format!("mise/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .wrap_err("building HTTP client for wings")?;
    let _ = HTTP_CLIENT.set(client.clone());
    Ok(client)
}

/// POST a JSON body and decode the response body as `R`.
pub(crate) async fn post_json<B: Serialize, R: for<'de> Deserialize<'de>>(
    url: &str,
    body: &B,
    headers: &reqwest::header::HeaderMap,
) -> Result<R> {
    let resp = http_client()?
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
