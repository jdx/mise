use crate::config::Settings;
use crate::env;
use crate::env_diff::EnvMap;
use eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

const GRANT_DEVICE_CODE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_TOKEN_SECS: i64 = 8 * 60 * 60;
const REUSE_BUFFER_SECS: i64 = 300;

static REFRESH_TOKEN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone)]
pub struct TokenRequest {
    pub host: String,
    /// Whether the device-code authorization flow may be triggered when no
    /// reusable cached or refreshed token is available. When false, an
    /// uncached host bails instead of prompting the user.
    pub allow_device_flow: bool,
    /// Mint a fresh token even when the cached one is still time-valid: try
    /// the refresh-token grant first, then fall back to the device flow (if
    /// allowed). GitHub App user tokens are scoped to the installations that
    /// existed when they were minted, so a cached token silently misses
    /// permissions granted afterwards (e.g. the app was installed on a repo
    /// after authorizing) until it expires hours later.
    pub force_refresh: bool,
}

impl Default for TokenRequest {
    fn default() -> Self {
        Self {
            host: "github.com".to_string(),
            allow_device_flow: false,
            force_refresh: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_poll_interval")]
    interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
    refresh_token_expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedToken {
    access_token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TokenCache {
    #[serde(default)]
    tokens: HashMap<String, CachedToken>,
}

#[cfg(test)]
static TEST_CACHE_PATH: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

pub fn resolve_token(host: &str) -> Option<String> {
    let settings = Settings::get();
    if settings.github.oauth_client_id.trim().is_empty()
        || !host_matches_settings(host, &settings.github.oauth_api_url)
    {
        return None;
    }

    token(TokenRequest {
        host: host.to_string(),
        allow_device_flow: false,
        force_refresh: false,
    })
    .ok()
}

pub fn cached_access_token_for_host(host: &str) -> Option<String> {
    let settings = Settings::get();
    let client_id = settings.github.oauth_client_id.trim();
    if client_id.is_empty() || !host_matches_settings(host, &settings.github.oauth_api_url) {
        return None;
    }
    let canonical_host =
        api_host(&settings.github.oauth_api_url).unwrap_or_else(|| host.to_string());
    let cache_key = cache_key(
        &canonical_host,
        client_id,
        settings.github.oauth_scopes.trim(),
    );
    read_cache()
        .tokens
        .get(&cache_key)
        .map(|cached| cached.access_token.clone())
}

/// If OAuth is configured and a cached or refreshable token is available,
/// inject it into the env map under the configured variable name. Never
/// triggers the device-code flow, so this is safe to call from shell hook
/// paths like `mise hook-env`, `mise env`, and `mise exec`.
pub fn inject_token_env(env: &mut EnvMap) {
    let settings = Settings::get();
    let var_name = settings.github.oauth_export_env.trim();
    if var_name.is_empty() || settings.github.oauth_client_id.trim().is_empty() {
        return;
    }
    let Some(host) = api_host(&settings.github.oauth_api_url) else {
        return;
    };
    if env.contains_key(var_name) {
        return;
    }
    if let Some(token) = resolve_token(&host) {
        env.insert(var_name.to_string(), token);
    }
}

pub fn token(req: TokenRequest) -> Result<String> {
    block_on(token_async(req))
}

pub async fn refresh_cached_token_for_host(
    host: &str,
    stale_access_token: &str,
) -> Result<Option<String>> {
    let settings = Settings::get();
    if settings.github.oauth_client_id.trim().is_empty()
        || !host_matches_settings(host, &settings.github.oauth_api_url)
    {
        return Ok(None);
    }

    let client_id = settings.github.oauth_client_id.trim();
    let scopes = settings.github.oauth_scopes.trim();
    let canonical_host =
        api_host(&settings.github.oauth_api_url).unwrap_or_else(|| host.to_string());
    let cache_key = cache_key(&canonical_host, client_id, scopes);
    refresh_cached_token(&cache_key, Some(stale_access_token)).await
}

async fn refresh_cached_token(
    cache_key: &str,
    stale_access_token: Option<&str>,
) -> Result<Option<String>> {
    let _lock = REFRESH_TOKEN_LOCK.lock().await;
    let mut cache = read_cache();
    let Some(mut cached) = cache.tokens.get(cache_key).cloned() else {
        return Ok(None);
    };

    let invalidate_on_none = if let Some(stale_access_token) = stale_access_token {
        if cached.access_token != stale_access_token {
            return Ok(Some(cached.access_token));
        }
        true
    } else if reusable(&cached) {
        return Ok(Some(cached.access_token));
    } else {
        false
    };

    let Some(refreshed) = refresh_token(&cached).await? else {
        if invalidate_on_none {
            cached.expires_at = chrono::Utc::now();
            cache.tokens.insert(cache_key.to_string(), cached);
            if let Err(err) = write_cache(&cache) {
                warn!("failed to invalidate GitHub OAuth token cache: {err:#}");
            }
        }
        return Ok(None);
    };
    let access_token = refreshed.access_token.clone();
    cache.tokens.insert(cache_key.to_string(), refreshed);
    if let Err(err) = write_cache(&cache) {
        warn!("failed to cache refreshed GitHub OAuth token: {err:#}");
    }
    Ok(Some(access_token))
}

async fn token_async(req: TokenRequest) -> Result<String> {
    let settings = Settings::get();
    let client_id = settings.github.oauth_client_id.trim();
    let scopes = settings.github.oauth_scopes.trim();
    if client_id.is_empty() {
        bail!("GitHub OAuth is not configured. Set github.oauth_client_id first.");
    }
    if !host_matches_settings(&req.host, &settings.github.oauth_api_url) {
        bail!(
            "GitHub OAuth is configured for {}, not {}",
            api_host(&settings.github.oauth_api_url).unwrap_or_else(|| "unknown host".to_string()),
            req.host
        );
    }

    let canonical_host =
        api_host(&settings.github.oauth_api_url).unwrap_or_else(|| req.host.clone());
    let cache_key = cache_key(&canonical_host, client_id, scopes);
    let mut cache = read_cache();
    if !req.force_refresh
        && let Some(cached) = cache.tokens.get(&cache_key)
        && reusable(cached)
    {
        return Ok(cached.access_token.clone());
    }
    if let Some(cached) = cache.tokens.get(&cache_key).cloned() {
        // Passing the cached token as "stale" forces the refresh-token grant
        // even though the token is still time-valid.
        let stale_access_token = req.force_refresh.then_some(cached.access_token.as_str());
        match refresh_cached_token(&cache_key, stale_access_token).await {
            Ok(Some(token)) => return Ok(token),
            Ok(None) => {}
            Err(err) => {
                debug!("failed to refresh GitHub OAuth token: {err:#}");
            }
        }
        if !req.force_refresh && cached.expires_at > chrono::Utc::now() {
            return Ok(cached.access_token);
        }
    }
    if !req.allow_device_flow {
        bail!("GitHub OAuth token is not cached. Run `mise token github --oauth` to authorize.");
    }

    let device = create_device_code().await?;
    print_device_instructions(&device);
    let token = poll_access_token(&device).await?;
    let cached = token_response_to_cache(token)?;
    let access_token = cached.access_token.clone();
    cache.tokens.insert(cache_key, cached);
    if let Err(err) = write_cache(&cache) {
        warn!("failed to cache GitHub OAuth token: {err:#}");
    }
    Ok(access_token)
}

async fn create_device_code() -> Result<DeviceCodeResponse> {
    let settings = Settings::get();
    let url = format!(
        "{}/device/code",
        settings.github.oauth_auth_url.trim_end_matches('/')
    );
    let client_id = settings.github.oauth_client_id.trim();
    let scopes = settings.github.oauth_scopes.trim();
    let mut form = vec![("client_id", client_id)];
    if !scopes.is_empty() {
        form.push(("scope", scopes));
    }
    Ok(crate::http::HTTP
        .reqwest()
        .post(url)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await?
        .error_for_status()?
        .json::<DeviceCodeResponse>()
        .await?)
}

async fn poll_access_token(device: &DeviceCodeResponse) -> Result<TokenResponse> {
    let settings = Settings::get();
    let deadline = chrono::Utc::now() + chrono::Duration::seconds(device.expires_in as i64);
    let mut interval = device.interval.max(1);
    let url = format!(
        "{}/oauth/access_token",
        settings.github.oauth_auth_url.trim_end_matches('/')
    );
    let client_id = settings.github.oauth_client_id.trim();
    let client = crate::http::HTTP.reqwest();

    loop {
        if chrono::Utc::now() >= deadline {
            bail!("GitHub device authorization expired");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;

        let response = match client
            .post(&url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", client_id),
                ("device_code", device.device_code.as_str()),
                ("grant_type", GRANT_DEVICE_CODE),
            ])
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            Ok(resp) => match resp.json::<TokenResponse>().await {
                Ok(body) => body,
                Err(err) => {
                    debug!("transient error polling GitHub OAuth token: {err:#}");
                    continue;
                }
            },
            Err(err) => {
                debug!("transient error polling GitHub OAuth token: {err:#}");
                continue;
            }
        };

        match response.error.as_deref() {
            None => return Ok(response),
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += 5;
                continue;
            }
            Some("expired_token") => bail!("GitHub device authorization expired"),
            Some("access_denied") => bail!("GitHub device authorization was denied"),
            Some(error) => {
                let details = response
                    .error_description
                    .unwrap_or_else(|| error.to_string());
                bail!("{details}");
            }
        }
    }
}

async fn refresh_token(cached: &CachedToken) -> Result<Option<CachedToken>> {
    let Some(refresh_token) = cached.refresh_token.as_deref() else {
        return Ok(None);
    };
    if cached
        .refresh_expires_at
        .is_some_and(|exp| exp <= chrono::Utc::now())
    {
        return Ok(None);
    }

    let settings = Settings::get();
    let url = format!(
        "{}/oauth/access_token",
        settings.github.oauth_auth_url.trim_end_matches('/')
    );
    let response = crate::http::HTTP
        .reqwest()
        .post(url)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", settings.github.oauth_client_id.trim()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<TokenResponse>()
        .await?;

    if response.error.is_some() {
        return Ok(None);
    }
    let mut refreshed = token_response_to_cache(response)?;
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = cached.refresh_token.clone();
        refreshed.refresh_expires_at = cached.refresh_expires_at;
    }
    Ok(Some(refreshed))
}

fn token_response_to_cache(response: TokenResponse) -> Result<CachedToken> {
    let access_token = response
        .access_token
        .ok_or_else(|| eyre!("GitHub token response did not include access_token"))?;
    let now = chrono::Utc::now();
    Ok(CachedToken {
        access_token,
        expires_at: now
            + chrono::Duration::seconds(response.expires_in.unwrap_or(DEFAULT_TOKEN_SECS)),
        refresh_token: response.refresh_token,
        refresh_expires_at: response
            .refresh_token_expires_in
            .map(|secs| now + chrono::Duration::seconds(secs)),
    })
}

fn print_device_instructions(device: &DeviceCodeResponse) {
    eprintln!(
        "Open {} and enter code {} to authorize GitHub access.",
        device.verification_uri, device.user_code
    );
    if Settings::get().github.oauth_open_browser {
        let _ = open_browser(&device.verification_uri);
    }
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).status()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(url).status()?;
    }
    Ok(())
}

fn reusable(token: &CachedToken) -> bool {
    token.expires_at - chrono::Duration::seconds(REUSE_BUFFER_SECS) > chrono::Utc::now()
}

fn cache_key(host: &str, client_id: &str, scopes: &str) -> String {
    let hash = blake3::hash(format!("{host}|{client_id}|{scopes}").as_bytes());
    hash.to_hex()[..16].to_string()
}

fn cache_path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_CACHE_PATH.read().unwrap().clone() {
        return path;
    }
    env::MISE_STATE_DIR.join("github-oauth-tokens.toml")
}

fn read_cache() -> TokenCache {
    let path = cache_path();
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_cache(cache: &TokenCache) -> Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(cache)?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)?;
    }
    Ok(())
}

fn host_matches_settings(host: &str, oauth_api_url: &str) -> bool {
    let Some(api_host) = api_host(oauth_api_url) else {
        return false;
    };
    host == api_host || format!("api.{host}") == api_host
}

fn api_host(oauth_api_url: &str) -> Option<String> {
    url::Url::parse(oauth_api_url)
        .ok()?
        .host_str()
        .map(|h| h.to_string())
}

fn default_poll_interval() -> u64 {
    5
}

fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime")
                .block_on(future)
        })
        .join()
        .expect("tokio runtime thread panicked")
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
            .block_on(future)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) fn cache_key(host: &str, client_id: &str, scopes: &str) -> String {
        super::cache_key(host, client_id, scopes)
    }

    pub(crate) fn set_cache_path(path: PathBuf) {
        *TEST_CACHE_PATH.write().unwrap() = Some(path);
    }

    pub(crate) fn clear_cache_path() {
        *TEST_CACHE_PATH.write().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OAuthEnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        vars: Vec<(&'static str, Option<String>)>,
    }

    impl OAuthEnvGuard {
        fn new(auth_url: String, cache_path: PathBuf) -> Self {
            let lock = crate::github::TEST_ENV_LOCK.lock().unwrap();
            let vars = vec![
                (
                    "MISE_GITHUB_OAUTH_CLIENT_ID",
                    std::env::var("MISE_GITHUB_OAUTH_CLIENT_ID").ok(),
                ),
                (
                    "MISE_GITHUB_OAUTH_AUTH_URL",
                    std::env::var("MISE_GITHUB_OAUTH_AUTH_URL").ok(),
                ),
                (
                    "MISE_GITHUB_OAUTH_API_URL",
                    std::env::var("MISE_GITHUB_OAUTH_API_URL").ok(),
                ),
                (
                    "MISE_GITHUB_OAUTH_SCOPES",
                    std::env::var("MISE_GITHUB_OAUTH_SCOPES").ok(),
                ),
                ("MISE_EXPERIMENTAL", std::env::var("MISE_EXPERIMENTAL").ok()),
            ];
            crate::env::set_var("MISE_GITHUB_OAUTH_CLIENT_ID", "Iv1.mock");
            crate::env::set_var("MISE_GITHUB_OAUTH_AUTH_URL", auth_url);
            crate::env::set_var("MISE_GITHUB_OAUTH_API_URL", "https://api.github.com");
            crate::env::remove_var("MISE_GITHUB_OAUTH_SCOPES");
            crate::env::set_var("MISE_EXPERIMENTAL", "1");
            test_support::set_cache_path(cache_path);
            Settings::reset(None);
            Self { _lock: lock, vars }
        }
    }

    impl Drop for OAuthEnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.vars {
                if let Some(value) = value {
                    crate::env::set_var(key, value);
                } else {
                    crate::env::remove_var(key);
                }
            }
            test_support::clear_cache_path();
            Settings::reset(None);
        }
    }

    #[tokio::test]
    async fn refresh_error_preserves_cached_token() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((sock, _)) = listener.accept().await {
                drop(sock);
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("github-oauth-tokens.toml");
        let _guard =
            OAuthEnvGuard::new(format!("http://127.0.0.1:{port}/login"), cache_path.clone());

        let expires_at = chrono::DateTime::parse_from_rfc3339("2099-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let cache_key = cache_key("api.github.com", "Iv1.mock", "");
        std::fs::write(
            &cache_path,
            format!(
                r#"[tokens.{cache_key}]
access_token = "ghu-stale"
expires_at = "2099-01-01T00:00:00Z"
refresh_token = "ghr-refresh"
refresh_expires_at = "2099-01-01T00:00:00Z"
"#
            ),
        )
        .unwrap();

        let result = refresh_cached_token(&cache_key, Some("ghu-stale")).await;

        assert!(result.is_err());
        let cache = read_cache();
        let cached = cache.tokens.get(&cache_key).unwrap();
        assert_eq!(cached.access_token, "ghu-stale");
        assert_eq!(cached.expires_at, expires_at);
    }

    #[tokio::test]
    async fn force_refresh_mints_new_token_despite_valid_cache() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let body = r#"{"access_token":"ghu-new","expires_in":28800,"refresh_token":"ghr-new","refresh_token_expires_in":15897600}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("github-oauth-tokens.toml");
        let _guard =
            OAuthEnvGuard::new(format!("http://127.0.0.1:{port}/login"), cache_path.clone());

        let cache_key = cache_key("api.github.com", "Iv1.mock", "");
        std::fs::write(
            &cache_path,
            format!(
                r#"[tokens.{cache_key}]
access_token = "ghu-current"
expires_at = "2099-01-01T00:00:00Z"
refresh_token = "ghr-refresh"
refresh_expires_at = "2099-01-01T00:00:00Z"
"#
            ),
        )
        .unwrap();

        // Without force_refresh the time-valid cached token is reused.
        let token = token_async(TokenRequest {
            host: "github.com".to_string(),
            allow_device_flow: false,
            force_refresh: false,
        })
        .await
        .unwrap();
        assert_eq!(token, "ghu-current");

        // With force_refresh the refresh-token grant mints a new token even
        // though the cached one has not expired.
        let token = token_async(TokenRequest {
            host: "github.com".to_string(),
            allow_device_flow: false,
            force_refresh: true,
        })
        .await
        .unwrap();
        assert_eq!(token, "ghu-new");

        let cache = read_cache();
        let cached = cache.tokens.get(&cache_key).unwrap();
        assert_eq!(cached.access_token, "ghu-new");
        assert_eq!(cached.refresh_token.as_deref(), Some("ghr-new"));
    }
}
