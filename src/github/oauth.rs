use crate::config::Settings;
use crate::env;
use eyre::{Result, bail, eyre};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Duration;

const GRANT_DEVICE_CODE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_TOKEN_SECS: i64 = 8 * 60 * 60;
const REUSE_BUFFER_SECS: i64 = 300;

#[derive(Debug, Clone)]
pub struct TokenRequest {
    pub host: String,
    pub force_device_flow: bool,
}

impl Default for TokenRequest {
    fn default() -> Self {
        Self {
            host: "github.com".to_string(),
            force_device_flow: false,
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

pub fn configured() -> bool {
    !Settings::get().github.oauth_client_id.trim().is_empty()
}

pub fn resolve_token(host: &str) -> Option<String> {
    if !configured() || !host_matches_settings(host) {
        return None;
    }

    token(TokenRequest {
        host: host.to_string(),
        force_device_flow: false,
    })
    .ok()
}

pub fn token(req: TokenRequest) -> Result<String> {
    block_on(token_async(req))
}

async fn token_async(req: TokenRequest) -> Result<String> {
    let settings = Settings::get();
    let client_id = settings.github.oauth_client_id.trim();
    if client_id.is_empty() {
        bail!("GitHub OAuth is not configured. Set github.oauth_client_id first.");
    }
    if !host_matches_settings(&req.host) {
        bail!(
            "GitHub OAuth is configured for {}, not {}",
            api_host().unwrap_or_else(|| "unknown host".to_string()),
            req.host
        );
    }

    let cache_key = cache_key(&req.host, client_id);
    let mut cache = read_cache();
    if let Some(cached) = cache.tokens.get(&cache_key)
        && reusable(cached)
    {
        return Ok(cached.access_token.clone());
    }
    if let Some(cached) = cache.tokens.get(&cache_key).cloned()
        && let Some(refreshed) = refresh_token(&cached).await?
    {
        let token = refreshed.access_token.clone();
        cache.tokens.insert(cache_key, refreshed);
        write_cache(&cache)?;
        return Ok(token);
    }

    if !req.force_device_flow && !std::io::stderr().is_terminal() {
        return Err(eyre!(
            "GitHub OAuth token is not cached. Run `mise github login` to authorize."
        ));
    }

    let device = create_device_code().await?;
    print_device_instructions(&device);
    let token = poll_access_token(&device).await?;
    let cached = token_response_to_cache(token)?;
    let access_token = cached.access_token.clone();
    cache.tokens.insert(cache_key, cached);
    write_cache(&cache)?;
    Ok(access_token)
}

async fn create_device_code() -> Result<DeviceCodeResponse> {
    let settings = Settings::get();
    let url = format!(
        "{}/device/code",
        settings.github.oauth_auth_url.trim_end_matches('/')
    );
    let mut form = vec![("client_id", settings.github.oauth_client_id.as_str())];
    if !settings.github.oauth_scopes.trim().is_empty() {
        form.push(("scope", settings.github.oauth_scopes.as_str()));
    }
    Ok(reqwest::Client::new()
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
        "{}/access_token",
        settings.github.oauth_auth_url.trim_end_matches('/')
    );

    loop {
        if chrono::Utc::now() >= deadline {
            bail!("GitHub device authorization expired");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;

        let response = reqwest::Client::new()
            .post(&url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", settings.github.oauth_client_id.as_str()),
                ("device_code", device.device_code.as_str()),
                ("grant_type", GRANT_DEVICE_CODE),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<TokenResponse>()
            .await?;

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
        "{}/access_token",
        settings.github.oauth_auth_url.trim_end_matches('/')
    );
    let response = reqwest::Client::new()
        .post(url)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", settings.github.oauth_client_id.as_str()),
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
    token_response_to_cache(response).map(Some)
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

fn cache_key(host: &str, client_id: &str) -> String {
    let hash = blake3::hash(
        format!(
            "{}|{}|{}",
            host,
            client_id,
            Settings::get().github.oauth_scopes
        )
        .as_bytes(),
    );
    hash.to_hex()[..16].to_string()
}

fn cache_path() -> PathBuf {
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

fn host_matches_settings(host: &str) -> bool {
    let Some(api_host) = api_host() else {
        return false;
    };
    host == api_host || (host == "github.com" && api_host == "api.github.com")
}

fn api_host() -> Option<String> {
    url::Url::parse(&Settings::get().github.oauth_api_url)
        .ok()?
        .host_str()
        .map(|h| h.to_string())
}

fn default_poll_interval() -> u64 {
    5
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
            .block_on(future)
    }
}
