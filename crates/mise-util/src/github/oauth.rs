use crate::env;
use crate::env_diff::EnvMap;
use eyre::{Result, bail, eyre};
use mise_settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const GRANT_DEVICE_CODE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_TOKEN_SECS: i64 = 8 * 60 * 60;
const REUSE_BUFFER_SECS: i64 = 300;
const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(2);
const ENCLAVE_NAME: &str = "the Secure Enclave helper";
const STORE_TIMEOUT: Duration = Duration::from_secs(60);

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
    /// Report refresh failures when credentials are needed, rather than merely exported.
    pub warn_on_refresh_failure: bool,
}

impl Default for TokenRequest {
    fn default() -> Self {
        Self {
            host: "github.com".to_string(),
            allow_device_flow: false,
            force_refresh: false,
            warn_on_refresh_failure: true,
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

#[derive(Debug, thiserror::Error)]
#[error(
    "GitHub OAuth refresh was rejected ({code}): {description}. Reauthorization is required; run `mise token github --oauth --refresh`"
)]
struct RefreshRejected {
    code: String,
    description: String,
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

#[doc(hidden)]
pub static TEST_CACHE_PATH: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

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
        ..Default::default()
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
        .ok()?
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
    if let Ok(token) = token(TokenRequest {
        host,
        warn_on_refresh_failure: false,
        ..Default::default()
    }) {
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
    let path = cache_path();
    // GitHub refresh tokens are single-use. Serialize the read-refresh-write
    // transaction across mise processes, then re-read after acquiring the
    // lock so a waiter observes any token minted by the previous holder.
    let (cache_lock, mut cache) = lock_and_read_cache(path).await?;
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

    if cached.refresh_token.is_none() && store().is_some() {
        // The refresh token lives in the external store. Read it only now that
        // a refresh is needed, and if it cannot be read stop here: refreshing
        // without it would look like "no refresh token" and invalidate a token
        // we merely could not unlock.
        cached.refresh_token = tokio::task::spawn_blocking({
            let key = cache_key.to_string();
            move || store_read().map(|mut tokens| tokens.remove(&key))
        })
        .await??;
    }

    let refreshed = match refresh_token(&cached).await {
        Ok(refreshed) => refreshed,
        Err(err) => {
            if invalidate_on_none && err.downcast_ref::<RefreshRejected>().is_some() {
                invalidate_cached_token(cache_key, cached, &mut cache);
                if let Err(cache_err) = write_cache_locked_async(cache_lock, cache).await {
                    warn!("failed to invalidate GitHub OAuth token cache: {cache_err:#}");
                }
            }
            return Err(err);
        }
    };
    let Some(refreshed) = refreshed else {
        if invalidate_on_none {
            invalidate_cached_token(cache_key, cached, &mut cache);
            if let Err(err) = write_cache_locked_async(cache_lock, cache).await {
                warn!("failed to invalidate GitHub OAuth token cache: {err:#}");
            }
        }
        return Ok(None);
    };
    let access_token = refreshed.access_token.clone();
    cache.tokens.insert(cache_key.to_string(), refreshed);
    if let Err(err) = write_cache_locked_async(cache_lock, cache).await {
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
    let cache = read_cache_async().await?;
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
                if req.warn_on_refresh_failure {
                    log_refresh_error(&err);
                } else {
                    debug!("failed to refresh GitHub OAuth token for environment export: {err:#}");
                }
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
    if let Err(err) = cache_token(cache_key, cached).await {
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
        .reqwest()?
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
    let client = crate::http::HTTP.reqwest()?;

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
        .reqwest()?
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

    if let Some(code) = response.error.as_deref() {
        let description = response
            .error_description
            .clone()
            .unwrap_or_else(|| code.to_string());
        return Err(RefreshRejected {
            code: code.to_string(),
            description,
        }
        .into());
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
    if should_copy_code() {
        match copy_to_clipboard(&device.user_code) {
            Ok(()) => eprintln!("Copied the code to your clipboard."),
            Err(e) => debug!("could not copy the device code to the clipboard: {e}"),
        }
    }
    if Settings::get().github.oauth_open_browser {
        let _ = open_browser(&device.verification_uri);
    }
}

/// An explicit `oauth_copy_code` wins; when unset, ask if a person can answer.
fn should_copy_code() -> bool {
    use std::io::IsTerminal;

    if let Some(copy) = Settings::get().github.oauth_copy_code {
        return copy;
    }
    // --yes, MISE_YES and CI mean nobody is there to answer, and copying to the
    // clipboard is a side effect we should not assume consent for.
    if Settings::get().yes || !(console::user_attended_stderr() && std::io::stdin().is_terminal()) {
        return false;
    }
    demand::Confirm::new("Copy the code to your clipboard?")
        .affirmative("Yes")
        .negative("No")
        .run()
        .unwrap_or(false)
}

/// Pipe `text` to the first clipboard tool that works on this platform.
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(windows) {
        &[("clip.exe", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
            ("clip.exe", &[]),
        ]
    };
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, "no clipboard tool found");
    let mut timed_out = false;
    for (cmd, args) in candidates {
        // A hung tool means the display is likely stale for the other display
        // tools too, so skip them. clip.exe (WSL) does not use the display, so
        // it still gets a bounded try; at most two timeouts are spent in total.
        if timed_out && *cmd != "clip.exe" {
            continue;
        }
        match pipe_to(cmd, args, text) {
            Ok(()) => return Ok(()),
            Err(e) => {
                timed_out |= e.kind() == std::io::ErrorKind::TimedOut;
                last = e;
            }
        }
    }
    Err(last)
}

fn pipe_to(cmd: &str, args: &[&str], text: &str) -> std::io::Result<()> {
    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let result = feed_and_wait(&mut child, cmd, text);
    if result.is_err() {
        // Every failure path (write error, try_wait error, timeout) must not
        // leave the tool running or unreaped.
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn feed_and_wait(child: &mut std::process::Child, cmd: &str, text: &str) -> std::io::Result<()> {
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("no stdin"))?
        .write_all(text.as_bytes())?;
    // A hung clipboard tool (stale DISPLAY, broken WSLg) must not hold up the
    // device flow until the code expires, so bound the wait.
    let deadline = std::time::Instant::now() + CLIPBOARD_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(format!("{cmd} failed")))
            };
        }
        if std::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("{cmd} timed out"),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
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
    if crate::testing::in_tests()
        && let Some(path) = TEST_CACHE_PATH.read().unwrap().clone()
    {
        return path;
    }
    env::MISE_STATE_DIR.join("github-oauth-tokens.toml")
}

fn read_cache() -> Result<TokenCache> {
    let path = cache_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => match toml::from_str(&content) {
            Ok(cache) => Ok(cache),
            Err(_) => {
                warn!(
                    "failed to parse GitHub OAuth token cache at {}; ignoring it",
                    path.display()
                );
                Ok(TokenCache::default())
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(TokenCache::default()),
        Err(err) => Err(eyre!(
            "failed to read GitHub OAuth token cache at {}: {err}",
            path.display()
        )),
    }
}

async fn read_cache_async() -> Result<TokenCache> {
    tokio::task::spawn_blocking(read_cache).await?
}

fn write_cache(cache: &TokenCache) -> Result<()> {
    let protected = protect_refresh_tokens(cache);
    let cache = protected.as_ref().unwrap_or(cache);
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(cache)?;
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("GitHub OAuth cache path has no parent"))?;
    let mut tmp = tempfile::NamedTempFile::with_prefix_in(".github-oauth-tokens.", parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(content.as_bytes())?;
    tmp.as_file_mut().sync_all()?;
    persist_cache_tmp(tmp, &path)?;
    Ok(())
}

/// Where the refresh tokens live when they are kept out of the cache file.
enum Store {
    /// User-supplied `oauth_cache_get` / `oauth_cache_set` commands.
    Commands { get: String, set: String },
    /// A file sealed by the macOS Secure Enclave helper; unsealing needs Touch ID.
    Enclave { helper: PathBuf },
}

type EnclaveHelper = fn() -> Result<PathBuf>;

/// The refresh tokens as last unsealed or sealed by this process, with the sealed
/// bytes they match. A refresh reads the store to refresh and again to merge the
/// rotated token, and each unseal is a Touch ID prompt, so the second read must
/// not unseal again. The copy is used only while the sealed file still holds
/// those exact bytes, so a token rotated by another mise process is never
/// replaced with a stale one.
static ENCLAVE_TOKENS: std::sync::Mutex<Option<(String, HashMap<String, String>)>> =
    std::sync::Mutex::new(None);

static ENCLAVE_HELPER: std::sync::OnceLock<EnclaveHelper> = std::sync::OnceLock::new();

/// Register how to find the Secure Enclave helper. The helper is embedded in the
/// mise binary, so mise supplies it. Without one, `oauth_cache_secure_enclave`
/// warns and the refresh token stays in the cache file.
pub fn set_enclave_helper(f: EnclaveHelper) {
    let _ = ENCLAVE_HELPER.set(f);
}

/// The configured store, if any. Setting only one command is a mistake that
/// would silently leave the refresh token in the file.
fn store() -> Option<Store> {
    let settings = Settings::get();
    let get = settings.github.oauth_cache_get.trim();
    let set = settings.github.oauth_cache_set.trim();
    if settings.github.oauth_cache_secure_enclave {
        if !get.is_empty() || !set.is_empty() {
            warn_once!(
                "github.oauth_cache_secure_enclave overrides github.oauth_cache_get and \
                 github.oauth_cache_set"
            );
        }
        let helper = ENCLAVE_HELPER
            .get()
            .ok_or_else(|| eyre!("the Secure Enclave is only available on macOS"))
            .and_then(|f| f());
        match helper {
            Ok(helper) => return Some(Store::Enclave { helper }),
            Err(err) => {
                warn_once!(
                    "github.oauth_cache_secure_enclave is unavailable ({err:#}); \
                     keeping the GitHub OAuth refresh token in the cache file"
                );
                return None;
            }
        }
    }
    match (get.is_empty(), set.is_empty()) {
        (false, false) => Some(Store::Commands {
            get: get.to_string(),
            set: set.to_string(),
        }),
        (true, true) => None,
        _ => {
            warn_once!(
                "github.oauth_cache_get and github.oauth_cache_set must both be set; \
                 keeping the GitHub OAuth refresh token in the cache file"
            );
            None
        }
    }
}

fn sealed_path() -> PathBuf {
    let mut name = cache_path()
        .file_stem()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(".sealed");
    cache_path().with_file_name(name)
}

fn store_command(cmd: &str) -> Result<std::process::Command> {
    // The store holds every host's refresh tokens in one value, so the command
    // must not see a host to pick an entry by.
    let mut command = crate::tokens::credential_command_process("github", cmd, "")
        .ok_or_else(|| eyre!("the default inline shell is empty"))?;
    command.env_remove("MISE_CREDENTIAL_HOST");
    Ok(command)
}

/// Run a store command to completion and return its raw stdout. The command runs
/// while mise holds the cache lock, so it is stopped and reaped when it fails
/// or takes longer than `STORE_TIMEOUT`.
fn run_store_command(name: &str, cmd: &str, input: Option<&str>) -> Result<Vec<u8>> {
    run_store_process(name, store_command(cmd)?, input)
}

fn run_store_process(
    name: &str,
    mut command: std::process::Command,
    input: Option<&str>,
) -> Result<Vec<u8>> {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| eyre!("failed to run {name}: {err}"))?;
    // The readers are joined only on success. After a kill, a grandchild of the
    // shell can still hold the pipes open, so joining on the failure paths could
    // hang the call the timeout is there to bound. The threads end when the
    // pipes close and own nothing but their buffers.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    if let Some(input) = input {
        let mut stdin = child.stdin.take().expect("stdin is piped");
        if let Err(e) = stdin.write_all(input.as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("failed to write to {name}: {e}");
        }
        // Close stdin so a command that reads until EOF can finish.
        drop(stdin);
    }
    let deadline = std::time::Instant::now() + STORE_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{name} did not finish within {}s", STORE_TIMEOUT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if !status.success() {
        bail!(
            "{name} failed ({status}): {}",
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    Ok(stdout)
}

/// Read the refresh tokens (cache key to token) from the external store.
fn store_read() -> Result<HashMap<String, String>> {
    let mut sealed_input = None;
    let (stdout, name) = match store() {
        None => return Ok(HashMap::new()),
        Some(Store::Commands { get, .. }) => (
            run_store_command("github.oauth_cache_get", &get, None)?,
            "github.oauth_cache_get",
        ),
        Some(Store::Enclave { helper }) => {
            let sealed = match std::fs::read_to_string(sealed_path()) {
                Ok(sealed) => sealed,
                // A missing file is an error, like a failing `oauth_cache_get`:
                // callers treat it as an empty store only while every refresh
                // token is still in the cache file, so tokens that were moved
                // out are never replaced by a store that lost them.
                Err(err) => bail!("failed to read {}: {err}", sealed_path().display()),
            };
            if let Some((bytes, tokens)) = &*ENCLAVE_TOKENS.lock().unwrap()
                && *bytes == sealed
            {
                return Ok(tokens.clone());
            }
            let mut command = std::process::Command::new(helper);
            command.arg("unseal");
            let out = run_store_process(ENCLAVE_NAME, command, Some(&sealed))?;
            sealed_input = Some(sealed);
            (out, ENCLAVE_NAME)
        }
    };
    let sealed = sealed_input;
    let stdout = String::from_utf8(stdout).map_err(|_| eyre!("{name} did not print UTF-8"))?;
    let tokens: HashMap<String, String> = if stdout.trim().is_empty() {
        HashMap::new()
    } else {
        toml::from_str(&stdout)
            .map_err(|_| eyre!("{name} did not print what the refresh token store was given"))?
    };
    if let Some(sealed) = sealed {
        *ENCLAVE_TOKENS.lock().unwrap() = Some((sealed, tokens.clone()));
    }
    Ok(tokens)
}

/// Hand the refresh tokens to the external store on its stdin.
fn store_write(tokens: &HashMap<String, String>) -> Result<()> {
    let Some(store) = store() else {
        return Ok(());
    };
    let ordered: std::collections::BTreeMap<_, _> = tokens.iter().collect();
    let value = toml::to_string(&ordered)?;
    match store {
        Store::Commands { set, .. } => {
            run_store_command("github.oauth_cache_set", &set, Some(&value))?;
        }
        Store::Enclave { helper } => {
            let mut command = std::process::Command::new(helper);
            command.arg("seal");
            let sealed = run_store_process(ENCLAVE_NAME, command, Some(&value))?;
            write_private_file(&sealed_path(), &sealed)?;
            *ENCLAVE_TOKENS.lock().unwrap() = Some((
                String::from_utf8_lossy(&sealed).into_owned(),
                tokens.clone(),
            ));
        }
    }
    Ok(())
}

/// Write `content` to `path` through a temp file, readable only by the user.
fn write_private_file(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::with_prefix_in(".github-oauth-sealed.", parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(content)?;
    tmp.as_file_mut().sync_all()?;
    persist_cache_tmp(tmp, path)?;
    Ok(())
}

/// When an external store is configured, move the refresh tokens in `cache`
/// into it and return a copy without them for the file. Returns `None` to write
/// `cache` unchanged: no store, nothing to protect, or the store failed.
///
/// A failure keeps the refresh token in the file: GitHub has already rotated
/// it, so dropping it would end the session.
fn protect_refresh_tokens(cache: &TokenCache) -> Option<TokenCache> {
    store()?;
    if cache.tokens.values().all(|t| t.refresh_token.is_none()) {
        return None;
    }
    // A store with nothing in it yet usually makes the read command fail
    // (`pass show` on a missing entry), which is only safe to treat as empty
    // while every refresh token is still in the file. If some were moved out
    // earlier, an unreadable store may hold them, and replacing it would lose
    // them.
    let nothing_stored_yet = cache.tokens.values().all(|t| t.refresh_token.is_some());
    let mut tokens = match store_read() {
        Ok(tokens) => tokens,
        Err(err) if nothing_stored_yet => {
            debug!("treating the GitHub OAuth refresh token store as empty: {err:#}");
            HashMap::new()
        }
        Err(err) => {
            warn!(
                "could not read the GitHub OAuth refresh token store; \
                 keeping the new refresh token in the cache file: {err:#}"
            );
            return None;
        }
    };
    for (key, cached) in &cache.tokens {
        if let Some(refresh) = &cached.refresh_token {
            tokens.insert(key.clone(), refresh.clone());
        }
    }
    let result = store_write(&tokens);
    if let Err(err) = result {
        warn!(
            "could not store the GitHub OAuth refresh token externally; \
             keeping it in the cache file: {err:#}"
        );
        return None;
    }
    let mut stripped = cache.clone();
    for cached in stripped.tokens.values_mut() {
        cached.refresh_token = None;
    }
    Some(stripped)
}

async fn cache_token(cache_key: String, cached: CachedToken) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let path = cache_path();
        let _cache_lock = lock_cache(&path)?;
        let mut cache = read_cache()?;
        cache.tokens.insert(cache_key, cached);
        write_cache(&cache)
    })
    .await?
}

fn invalidate_cached_token(cache_key: &str, mut cached: CachedToken, cache: &mut TokenCache) {
    cached.expires_at = chrono::Utc::now();
    cache.tokens.insert(cache_key.to_string(), cached);
}

async fn lock_and_read_cache(path: PathBuf) -> Result<(fslock::LockFile, TokenCache)> {
    tokio::task::spawn_blocking(move || {
        let cache_lock = lock_cache(&path)?;
        let cache = read_cache()?;
        Ok((cache_lock, cache))
    })
    .await?
}

async fn write_cache_locked_async(cache_lock: fslock::LockFile, cache: TokenCache) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _cache_lock = cache_lock;
        write_cache(&cache)
    })
    .await?
}

fn lock_cache(path: &Path) -> Result<fslock::LockFile> {
    crate::lock_file::LockFile::new(path)
        .with_callback(|_| debug!("waiting for GitHub OAuth token cache lock"))
        .lock()
}

pub fn log_refresh_error(err: &eyre::Report) {
    if err.downcast_ref::<RefreshRejected>().is_some() {
        warn_once!("{err}");
    } else {
        debug!("failed to refresh GitHub OAuth token: {err:#}");
    }
}

fn persist_cache_tmp(mut tmp: tempfile::NamedTempFile, target: &Path) -> Result<()> {
    const RETRIES: u32 = 20;

    for attempt in 0..=RETRIES {
        match tmp.persist(target) {
            Ok(_) => return Ok(()),
            Err(err) if should_retry_cache_persist(&err.error) && attempt < RETRIES => {
                tmp = err.file;
                std::thread::sleep(Duration::from_millis(5 * u64::from(attempt + 1)));
            }
            Err(err) => return Err(err.error.into()),
        }
    }

    unreachable!("OAuth cache persist retry loop should always return");
}

#[cfg(windows)]
fn should_retry_cache_persist(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(not(windows))]
fn should_retry_cache_persist(_err: &std::io::Error) -> bool {
    false
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

#[doc(hidden)]
pub mod test_support {
    use super::*;

    pub fn cache_key(host: &str, client_id: &str, scopes: &str) -> String {
        super::cache_key(host, client_id, scopes)
    }

    pub fn set_cache_path(path: PathBuf) {
        *TEST_CACHE_PATH.write().unwrap() = Some(path);
    }

    pub fn clear_cache_path() {
        *TEST_CACHE_PATH.write().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OAuthEnvGuard {
        _settings_lock: std::sync::MutexGuard<'static, ()>,
        _lock: std::sync::MutexGuard<'static, ()>,
        vars: Vec<(&'static str, Option<String>)>,
    }

    impl OAuthEnvGuard {
        fn new(auth_url: String, cache_path: PathBuf) -> Self {
            // The settings are reloaded below, so hold still any test that overrides them.
            let settings_lock =
                crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
            let lock = crate::testing::lock_ignoring_poison(&crate::github::TEST_ENV_LOCK);
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
            crate::testing::reset_settings(None);
            Self {
                _settings_lock: settings_lock,
                _lock: lock,
                vars,
            }
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
            crate::testing::reset_settings(None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn enclave_store_unseals_once_per_refresh_and_survives_a_cancelled_prompt() {
        use std::os::unix::fs::PermissionsExt;

        static HELPER_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("github-oauth-tokens.toml");
        let _guard = OAuthEnvGuard::new("http://127.0.0.1:1/login".into(), cache_path.clone());
        crate::env::set_var("MISE_GITHUB_OAUTH_CACHE_SECURE_ENCLAVE", "1");
        crate::testing::reset_settings(None);

        // Stands in for the Touch ID helper: "sealing" is base64, each unseal is
        // counted, and an unseal fails while `cancel` exists.
        let helper = dir.path().join("mise-enclave");
        std::fs::write(
            &helper,
            "#!/bin/sh\nd=$(dirname \"$0\")\ncase $1 in\nseal) base64;;\nunseal)\n  echo x >> \"$d/unseals\"\n  [ -e \"$d/cancel\" ] && { echo cancelled >&2; exit 1; }\n  base64 -d;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = HELPER_PATH.set(helper.clone());
        set_enclave_helper(|| Ok(HELPER_PATH.get().unwrap().clone()));
        *ENCLAVE_TOKENS.lock().unwrap() = None;
        let unseals = || {
            std::fs::read_to_string(dir.path().join("unseals"))
                .map(|s| s.lines().count())
                .unwrap_or(0)
        };

        let token = |refresh: &str| CachedToken {
            access_token: "ghu".into(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(8),
            refresh_token: Some(refresh.into()),
            refresh_expires_at: None,
        };
        let mut cache = TokenCache::default();
        cache.tokens.insert("a".into(), token("ghr-a"));
        cache.tokens.insert("b".into(), token("ghr-b"));

        // Storing a fresh authorization seals both tokens and keeps them out of the file.
        write_cache(&cache).unwrap();
        assert_eq!(unseals(), 0);
        let content = std::fs::read_to_string(&cache_path).unwrap();
        assert!(!content.contains("ghr-"), "{content}");
        assert!(sealed_path().exists());

        // A refresh in a new process: one unseal to read, none to save the rotated token.
        *ENCLAVE_TOKENS.lock().unwrap() = None;
        let mut tokens = store_read().unwrap();
        assert_eq!(tokens.remove("a").as_deref(), Some("ghr-a"));
        let mut cache = read_cache().unwrap();
        cache.tokens.get_mut("a").unwrap().refresh_token = Some("ghr-a2".into());
        write_cache(&cache).unwrap();
        assert_eq!(unseals(), 1);

        // The other entry's token survives, and the rotated one replaced the old.
        *ENCLAVE_TOKENS.lock().unwrap() = None;
        let tokens = store_read().unwrap();
        assert_eq!(tokens.get("a").map(String::as_str), Some("ghr-a2"));
        assert_eq!(tokens.get("b").map(String::as_str), Some("ghr-b"));

        // Another mise process rotating the sealed file invalidates this process's copy.
        let before = unseals();
        let mut other = HashMap::new();
        other.insert("a".to_string(), "ghr-a-rotated-elsewhere".to_string());
        let mut command = std::process::Command::new(&helper);
        command.arg("seal");
        let sealed =
            run_store_process("fake", command, Some(&toml::to_string(&other).unwrap())).unwrap();
        write_private_file(&sealed_path(), &sealed).unwrap();
        let tokens = store_read().unwrap();
        assert_eq!(
            tokens.get("a").map(String::as_str),
            Some("ghr-a-rotated-elsewhere")
        );
        assert_eq!(unseals(), before + 1);

        // A lost sealed file must not be replaced by a store holding only the new token.
        let backup = dir.path().join("sealed.bak");
        std::fs::rename(sealed_path(), &backup).unwrap();
        *ENCLAVE_TOKENS.lock().unwrap() = None;
        assert!(store_read().is_err());
        let mut cache = read_cache().unwrap();
        cache.tokens.get_mut("a").unwrap().refresh_token = Some("ghr-a4".into());
        write_cache(&cache).unwrap();
        assert!(!sealed_path().exists());
        assert!(
            std::fs::read_to_string(&cache_path)
                .unwrap()
                .contains("ghr-a4")
        );
        std::fs::rename(&backup, sealed_path()).unwrap();

        // A cancelled prompt is an error, never an empty store, so nothing is dropped.
        std::fs::write(dir.path().join("cancel"), "").unwrap();
        *ENCLAVE_TOKENS.lock().unwrap() = None;
        assert!(store_read().is_err());
        let sealed_before = std::fs::read(sealed_path()).unwrap();
        let mut cache = read_cache().unwrap();
        cache.tokens.get_mut("a").unwrap().refresh_token = Some("ghr-a3".into());
        cache.tokens.get_mut("b").unwrap().refresh_token = None;
        write_cache(&cache).unwrap();
        assert_eq!(std::fs::read(sealed_path()).unwrap(), sealed_before);

        crate::env::remove_var("MISE_GITHUB_OAUTH_CACHE_SECURE_ENCLAVE");
        *ENCLAVE_TOKENS.lock().unwrap() = None;
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
        let cache = read_cache().unwrap();
        let cached = cache.tokens.get(&cache_key).unwrap();
        assert_eq!(cached.access_token, "ghu-stale");
        assert_eq!(cached.expires_at, expires_at);
    }

    #[tokio::test]
    async fn refresh_rejection_surfaces_sanitized_error() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let body = r#"{"error":"bad_refresh_token","error_description":"The refresh token is invalid."}"#;
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
access_token = "ghu-stale"
expires_at = "2020-01-01T00:00:00Z"
refresh_token = "ghr-secret"
refresh_expires_at = "2099-01-01T00:00:00Z"
"#
            ),
        )
        .unwrap();

        let err = refresh_cached_token(&cache_key, Some("ghu-stale"))
            .await
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("bad_refresh_token"));
        assert!(message.contains("Reauthorization is required"));
        assert!(!message.contains("ghr-secret"));
        let cache = read_cache().unwrap();
        let cached = cache.tokens.get(&cache_key).unwrap();
        assert!(cached.expires_at <= chrono::Utc::now());
    }

    #[test]
    fn corrupt_cache_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("github-oauth-tokens.toml");
        let _guard = OAuthEnvGuard::new("http://127.0.0.1:9/login".to_string(), cache_path.clone());
        std::fs::write(&cache_path, "[tokens.invalid").unwrap();

        let cache = read_cache().unwrap();

        assert!(cache.tokens.is_empty());
    }

    #[test]
    fn refresh_rereads_cache_after_filesystem_lock() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("github-oauth-tokens.toml");
        let _guard = OAuthEnvGuard::new("http://127.0.0.1:9/login".to_string(), cache_path.clone());
        let cache_key = cache_key("api.github.com", "Iv1.mock", "");
        std::fs::write(
            &cache_path,
            format!(
                r#"[tokens.{cache_key}]
access_token = "ghu-stale"
expires_at = "2020-01-01T00:00:00Z"
refresh_token = "ghr-stale"
refresh_expires_at = "2099-01-01T00:00:00Z"
"#
            ),
        )
        .unwrap();

        let cache_lock = crate::lock_file::LockFile::new(&cache_path).lock().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let refresh_key = cache_key.clone();
        let refresh = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            block_on(async move { refresh_cached_token(&refresh_key, Some("ghu-stale")).await })
        });
        started_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(50));

        let mut cache = read_cache().unwrap();
        cache.tokens.insert(
            cache_key,
            CachedToken {
                access_token: "ghu-fresh".to_string(),
                expires_at: chrono::DateTime::parse_from_rfc3339("2099-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                refresh_token: Some("ghr-fresh".to_string()),
                refresh_expires_at: None,
            },
        );
        write_cache(&cache).unwrap();
        drop(cache_lock);

        let token = refresh.join().unwrap().unwrap();
        assert_eq!(token.as_deref(), Some("ghu-fresh"));
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
            ..Default::default()
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
            ..Default::default()
        })
        .await
        .unwrap();
        assert_eq!(token, "ghu-new");

        let cache = read_cache().unwrap();
        let cached = cache.tokens.get(&cache_key).unwrap();
        assert_eq!(cached.access_token, "ghu-new");
        assert_eq!(cached.refresh_token.as_deref(), Some("ghr-new"));
    }
}
