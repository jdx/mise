use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use eyre::{Report, Result, bail, ensure};
use regex::Regex;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{ClientBuilder, IntoUrl, Method, Response};
use std::sync::LazyLock as Lazy;
use tokio::sync::OnceCell;
use url::Url;

use crate::cli::version;
use crate::config::Settings;
use crate::file::display_path;
use crate::netrc;
use crate::ui::progress_report::SingleReport;
use crate::ui::time::format_duration;
use crate::{env, file};

#[cfg(not(test))]
pub static HTTP_VERSION_CHECK: Lazy<Client> =
    Lazy::new(|| Client::new(Duration::from_secs(3), ClientKind::VersionCheck).unwrap());

pub static HTTP: Lazy<Client> =
    Lazy::new(|| Client::new(Settings::get().http_timeout(), ClientKind::Http).unwrap());

pub static HTTP_FETCH: Lazy<Client> = Lazy::new(|| {
    Client::new(
        Settings::get().fetch_remote_versions_timeout(),
        ClientKind::Fetch,
    )
    .unwrap()
});

/// In-memory cache for HTTP text responses, useful for requests that are repeated
/// during a single operation (e.g., fetching SHASUMS256.txt for multiple platforms).
/// Each URL gets its own OnceCell to ensure concurrent requests for the same URL
/// wait for the first fetch to complete rather than all fetching simultaneously.
type CachedResult = Arc<OnceCell<Result<String, String>>>;
static HTTP_CACHE: Lazy<Mutex<HashMap<String, CachedResult>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug)]
pub struct Client {
    reqwest: reqwest::Client,
    timeout: Duration,
    kind: ClientKind,
}

#[derive(Debug, Clone, Copy)]
enum ClientKind {
    Http,
    Fetch,
    #[allow(dead_code)]
    VersionCheck,
}

impl Client {
    fn new(timeout: Duration, kind: ClientKind) -> Result<Self> {
        Ok(Self {
            reqwest: Self::_new()
                .read_timeout(timeout)
                .connect_timeout(timeout)
                .build()?,
            timeout,
            kind,
        })
    }

    /// Underlying reqwest client. Use sparingly — most callers should reach for
    /// the higher-level `get_*`/`json_*`/`post_json_*` helpers instead. This
    /// exists for callers that need request shapes those helpers don't cover
    /// (e.g. form-encoded POST in the GitHub OAuth flow) but still want the
    /// shared timeouts, gzip, and user-agent.
    pub fn reqwest(&self) -> &reqwest::Client {
        &self.reqwest
    }

    fn _new() -> ClientBuilder {
        let v = &*version::VERSION;
        let shell = env::MISE_SHELL.map(|s| s.to_string()).unwrap_or_default();
        ClientBuilder::new()
            .user_agent(format!("mise/{v} {shell}").trim())
            .gzip(true)
            .zstd(true)
    }

    pub async fn get_bytes<U: IntoUrl>(&self, url: U) -> Result<impl AsRef<[u8]>> {
        let url = url.into_url().unwrap();
        let resp = self.get_async(url.clone()).await?;
        Ok(resp.bytes().await?)
    }

    /// Like `get_bytes`, but lets the caller supply the exact headers used
    /// for the request. Does NOT merge `host_auth_headers` — this mirrors
    /// `json_headers_with_headers` so callers get consistent behavior
    /// between manifest JSON fetches and blob byte fetches to the same host
    /// (e.g. `ghcr.io`, where the OCI Bearer token must not be mixed with
    /// the GitHub token that host_auth_headers would inject).
    pub async fn get_bytes_with_headers<U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<impl AsRef<[u8]>> {
        let url = url.into_url().unwrap();
        let resp = self.get_async_with_headers(url, headers).await?;
        Ok(resp.bytes().await?)
    }

    pub async fn get_async<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let url = url.into_url().unwrap();
        let headers = host_auth_headers(&url);
        self.get_async_with_headers(url, &headers).await
    }

    async fn get_async_with_headers<U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<Response> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url().unwrap();
        let resp = self
            .send_with_https_fallback(Method::GET, url, headers, "GET")
            .await?;
        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub async fn head<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let url = url.into_url().unwrap();
        let headers = host_auth_headers(&url);
        self.head_async_with_headers(url, &headers).await
    }

    pub async fn head_async_with_headers<U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<Response> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url().unwrap();
        let resp = self
            .send_with_https_fallback(Method::HEAD, url, headers, "HEAD")
            .await?;
        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub async fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        self.get_text_with_headers(url, &HeaderMap::new()).await
    }

    pub async fn get_text_with_headers<U: IntoUrl>(
        &self,
        url: U,
        extra_headers: &HeaderMap,
    ) -> Result<String> {
        let mut url = url.into_url().unwrap();
        // Merge GitHub headers with any extra headers provided
        let mut headers = host_auth_headers(&url);
        headers.extend(extra_headers.clone());
        let resp = self.get_async_with_headers(url.clone(), &headers).await?;
        let text = resp.text().await?;
        if text.starts_with("<!DOCTYPE html>") {
            if url.scheme() == "http" {
                // try with https since http may be blocked
                url.set_scheme("https").unwrap();
                return Box::pin(self.get_text_with_headers(url, extra_headers)).await;
            }
            bail!("Got HTML instead of text from {}", url);
        }
        Ok(text)
    }

    /// Like get_text but caches results in memory for the duration of the process.
    /// Useful when the same URL will be requested multiple times (e.g., SHASUMS256.txt
    /// when locking multiple platforms). Concurrent requests for the same URL will
    /// wait for the first fetch to complete.
    pub async fn get_text_cached<U: IntoUrl>(&self, url: U) -> Result<String> {
        let url = url.into_url().unwrap();
        let key = url.to_string();

        // Get or create the OnceCell for this URL
        let cell = {
            let mut cache = HTTP_CACHE.lock().unwrap();
            cache.entry(key).or_default().clone()
        };

        // Initialize the cell if needed - concurrent callers will wait
        let result = cell
            .get_or_init(|| {
                let url = url.clone();
                async move {
                    match self.get_text(url).await {
                        Ok(text) => Ok(text),
                        Err(err) => Err(err.to_string()),
                    }
                }
            })
            .await;

        match result {
            Ok(text) => Ok(text.clone()),
            Err(err) => bail!("{}", err),
        }
    }

    pub async fn get_html<U: IntoUrl>(&self, url: U) -> Result<String> {
        let url = url.into_url().unwrap();
        let resp = self.get_async(url.clone()).await?;
        let is_html = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|content_type| content_type.to_str().ok())
            .is_some_and(|content_type| {
                content_type
                    .split_once(';')
                    .map_or(content_type, |(media_type, _)| media_type)
                    .trim()
                    .eq_ignore_ascii_case("text/html")
            });
        if !is_html {
            bail!("Got non-HTML text from {}", url);
        }
        let html = resp.text().await?;
        Ok(html)
    }

    pub async fn json_headers<T, U: IntoUrl>(&self, url: U) -> Result<(T, HeaderMap)>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let resp = self.get_async(url).await?;
        let headers = resp.headers().clone();
        let json = resp.json().await?;
        Ok((json, headers))
    }

    pub async fn json_headers_with_headers<T, U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<(T, HeaderMap)>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let resp = self.get_async_with_headers(url, headers).await?;
        let headers = resp.headers().clone();
        let json = resp.json().await?;
        Ok((json, headers))
    }

    pub async fn json<T, U: IntoUrl>(&self, url: U) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.json_headers(url).await.map(|(json, _)| json)
    }

    /// Like json but caches raw JSON text in memory for the duration of the process.
    /// Useful when the same URL will be requested multiple times (e.g., zig index.json
    /// when locking multiple platforms). Concurrent requests for the same URL will
    /// wait for the first fetch to complete.
    pub async fn json_cached<T, U: IntoUrl>(&self, url: U) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let text = self.get_text_cached(url).await?;
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn json_with_headers<T, U: IntoUrl>(&self, url: U, headers: &HeaderMap) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.json_headers_with_headers(url, headers)
            .await
            .map(|(json, _)| json)
    }

    /// POST JSON data to a URL. Returns Ok(true) on success, Ok(false) on non-success status.
    /// Errors only on network/connection failures.
    #[allow(dead_code)]
    pub async fn post_json<U: IntoUrl, T: serde::Serialize>(
        &self,
        url: U,
        body: &T,
    ) -> Result<bool> {
        self.post_json_with_headers(url, body, &HeaderMap::new())
            .await
    }

    /// POST JSON data to a URL with custom headers.
    pub async fn post_json_with_headers<U: IntoUrl, T: serde::Serialize>(
        &self,
        url: U,
        body: &T,
        headers: &HeaderMap,
    ) -> Result<bool> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url()?;
        debug!("POST {}", &url);
        let resp = self
            .reqwest
            .post(url)
            .header("Content-Type", "application/json")
            .headers(headers.clone())
            .json(body)
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    pub async fn download_file<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        let url = url.into_url()?;
        let headers = host_auth_headers(&url);
        self.download_file_with_headers(url, path, &headers, pr)
            .await
    }

    pub async fn download_file_with_headers<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        headers: &HeaderMap,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url()?;
        debug!("GET Downloading {} to {}", &url, display_path(path));
        let parent = path.parent().unwrap();
        file::create_dir_all(parent)?;

        // Retry the whole download so a mid-stream chunk failure restarts from
        // byte 0 instead of failing the install. send_once_with_https_fallback
        // (not send_with_https_fallback) is used inside to avoid retry-on-retry.
        retry_async("GET", &url, || async {
            let mut resp = self
                .send_once_with_https_fallback(Method::GET, url.clone(), headers, "GET")
                .await?;
            if let Some(length) = resp.content_length()
                && let Some(pr) = pr
            {
                // Reset progress on each attempt
                pr.set_length(length);
                pr.set_position(0);
            }
            let mut file = tempfile::NamedTempFile::with_prefix_in(path, parent)?;
            while let Some(chunk) = resp.chunk().await? {
                if crate::ui::ctrlc::is_cancelled() {
                    bail!("download cancelled by user");
                }
                file.write_all(&chunk)?;
                if let Some(pr) = pr {
                    pr.inc(chunk.len() as u64);
                }
            }
            file.persist(path)?;
            Ok(())
        })
        .await
    }

    async fn send_with_https_fallback(
        &self,
        method: Method,
        url: Url,
        headers: &HeaderMap,
        verb_label: &str,
    ) -> Result<Response> {
        retry_async(verb_label, &url, || async {
            self.send_once_with_https_fallback(method.clone(), url.clone(), headers, verb_label)
                .await
        })
        .await
    }

    /// One attempt with http→https fallback, no retry. Used as the inner step
    /// for both `send_with_https_fallback` (which adds retry) and
    /// `download_file_with_headers` (which has its own outer retry covering the
    /// chunk stream). Splitting this out avoids retry × retry blowup.
    /// The fallback only fires on connection-level errors (corporate proxy
    /// blocking plain http), not on HTTP status errors — falling back to https
    /// after the server already returned a 4xx/5xx makes no sense.
    async fn send_once_with_https_fallback(
        &self,
        method: Method,
        url: Url,
        headers: &HeaderMap,
        verb_label: &str,
    ) -> Result<Response> {
        match self
            .send_once(method.clone(), url.clone(), headers, verb_label)
            .await
        {
            Ok(resp) => Ok(resp),
            Err(err) if url.scheme() == "http" && is_connection_error(&err) => {
                let mut url = url;
                url.set_scheme("https").unwrap();
                self.send_once(method, url, headers, verb_label).await
            }
            Err(err) => Err(err),
        }
    }

    async fn send_once(
        &self,
        method: Method,
        url: Url,
        headers: &HeaderMap,
        verb_label: &str,
    ) -> Result<Response> {
        self.send_once_inner(method, url, headers, verb_label, true)
            .await
    }

    async fn send_once_inner(
        &self,
        method: Method,
        mut url: Url,
        headers: &HeaderMap,
        verb_label: &str,
        use_netrc: bool,
    ) -> Result<Response> {
        apply_url_replacements(&mut url);
        debug!("{} {}", verb_label, &url);

        // Apply netrc credentials after URL replacement
        let mut final_headers = headers.clone();
        if use_netrc {
            final_headers.extend(netrc_headers(&url));
        }

        let mut req = self.reqwest.request(method.clone(), url.clone());
        req = req.headers(final_headers.clone());
        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(err) => {
                if err.is_timeout() {
                    let (setting, env_var) = match self.kind {
                        ClientKind::Http => ("http_timeout", "MISE_HTTP_TIMEOUT"),
                        ClientKind::Fetch => (
                            "fetch_remote_versions_timeout",
                            "MISE_FETCH_REMOTE_VERSIONS_TIMEOUT",
                        ),
                        ClientKind::VersionCheck => ("version_check_timeout", ""),
                    };
                    let hint = if env_var.is_empty() {
                        format!(
                            "HTTP timed out after {} for {}.",
                            format_duration(self.timeout),
                            url
                        )
                    } else {
                        format!(
                            "HTTP timed out after {} for {} (change with `{}` or env `{}`).",
                            format_duration(self.timeout),
                            url,
                            setting,
                            env_var
                        )
                    };
                    // wrap_err preserves the underlying reqwest::Error in the chain so
                    // is_transient() can still classify this as a retryable timeout.
                    return Err(Report::new(err).wrap_err(hint));
                }
                return Err(err.into());
            }
        };
        if *env::MISE_LOG_HTTP {
            eprintln!("{} {url} {}", verb_label, resp.status());
        }
        debug!("{} {url} {}", verb_label, resp.status());
        display_github_rate_limit(&resp);
        if is_authenticated_github_forbidden(&url, &final_headers, &resp) {
            let status = resp.status();
            let status_error = resp
                .error_for_status_ref()
                .expect_err("403 response should be an error");
            let body = resp.text().await.unwrap_or_default();
            // Retry without auth when the response mentions IP allow lists: GitHub App
            // installation tokens (`ghs_*`) get 403 on public API resources for orgs with IP
            // allow lists; stripping auth avoids that path.
            // https://github.com/orgs/community/discussions/191185
            // https://github.com/jdx/mise/discussions/9119
            if body.contains("IP allow list") {
                let mut headers = final_headers;
                headers.remove(AUTHORIZATION);
                debug!(
                    "{} {} retrying without GitHub auth after {}",
                    verb_label, &url, status
                );
                return Box::pin(self.send_once_inner(method, url, &headers, verb_label, false))
                    .await;
            }
            return Err(status_error.into());
        }
        resp.error_for_status_ref()?;
        Ok(resp)
    }
}

fn is_authenticated_github_forbidden(url: &Url, headers: &HeaderMap, resp: &Response) -> bool {
    resp.status() == StatusCode::FORBIDDEN
        && url.host_str() == Some("api.github.com")
        && headers.contains_key(AUTHORIZATION)
}

pub fn error_code(e: &Report) -> Option<u16> {
    if e.to_string().contains("404") {
        // TODO: not this when I can figure out how to use eyre properly
        return Some(404);
    }
    if let Some(err) = e.downcast_ref::<reqwest::Error>() {
        err.status().map(|s| s.as_u16())
    } else {
        None
    }
}

fn host_auth_headers(url: &Url) -> HeaderMap {
    if crate::github::is_github_api_url(url) {
        return crate::github::get_headers(url.as_str());
    }

    let Some(host) = url.host_str() else {
        return HeaderMap::new();
    };

    let is_gitlab = host == "gitlab.com" || crate::gitlab::is_gitlab_host(host);
    if is_gitlab {
        return crate::gitlab::get_headers(url.as_str());
    }

    let is_forgejo = host == "codeberg.org" || crate::forgejo::is_forgejo_host(host);
    if is_forgejo {
        return crate::forgejo::get_headers(url.as_str());
    }

    HeaderMap::new()
}

/// Get HTTP Basic authentication headers from netrc file for the given URL
fn netrc_headers(url: &Url) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(host) = url.host_str()
        && let Some((login, password)) = netrc::get_credentials(host)
    {
        let credentials = BASE64_STANDARD.encode(format!("{login}:{password}"));
        if let Ok(value) = HeaderValue::from_str(&format!("Basic {credentials}")) {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
    }
    headers
}

/// Apply URL replacements based on settings configuration
/// Supports both simple string replacement and regex patterns (prefixed with "regex:")
pub fn apply_url_replacements(url: &mut Url) {
    let settings = Settings::get();
    if let Some(replacements) = &settings.url_replacements {
        let url_string = url.to_string();

        for (pattern, replacement) in replacements {
            if let Some(pattern_without_prefix) = pattern.strip_prefix("regex:") {
                // Regex replacement
                if let Ok(regex) = Regex::new(pattern_without_prefix) {
                    let new_url_string = regex.replace(&url_string, replacement.as_str());
                    // Only proceed if the URL actually changed
                    if new_url_string != url_string
                        && let Ok(new_url) = new_url_string.parse()
                    {
                        *url = new_url;
                        trace!(
                            "Replaced URL using regex '{}': {} -> {}",
                            pattern_without_prefix,
                            url_string,
                            url.as_str()
                        );
                        return; // Apply only the first matching replacement
                    }
                } else {
                    warn!(
                        "Invalid regex pattern in URL replacement: {}",
                        pattern_without_prefix
                    );
                }
            } else {
                // Simple string replacement
                if url_string.contains(pattern) {
                    let new_url_string = url_string.replace(pattern, replacement);
                    // Only proceed if the URL actually changed
                    if new_url_string != url_string
                        && let Ok(new_url) = new_url_string.parse()
                    {
                        *url = new_url;
                        trace!(
                            "Replaced URL using string replacement '{}': {} -> {}",
                            pattern,
                            url_string,
                            url.as_str()
                        );
                        return; // Apply only the first matching replacement
                    }
                }
            }
        }
    }
}

fn display_github_rate_limit(resp: &Response) {
    let status = resp.status().as_u16();
    if status == 403 || status == 429 {
        let remaining = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|r| r.to_str().ok());
        if remaining.is_some_and(|r| r == "0") {
            if let Some(reset_time) = resp
                .headers()
                .get("x-ratelimit-reset")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            {
                warn!(
                    "GitHub rate limit exceeded. Resets at {}",
                    reset_time.with_timezone(&chrono::Local)
                );
            }
            return;
        }
        // retry-after header is processed only if x-ratelimit-remaining is not 0 or is missing
        if let Some(retry_after) = resp
            .headers()
            .get("retry-after")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
        {
            warn!(
                "GitHub rate limit exceeded. Retry after {} seconds",
                retry_after
            );
        }
    }
}

fn default_backoff_strategy(retries: i64) -> impl Iterator<Item = Duration> {
    // Hand-rolled schedule (with jitter): ~200ms / ~1s / ~4s / ~15s, then 15s
    // for every retry beyond the schedule. The trailing repeat matters because
    // `MISE_HTTP_RETRIES` can be set arbitrarily high — a fixed-length array
    // would silently cap retries at its length. tokio_retry's ExponentialBackoff
    // ::from_millis is geometric in the base (base, base*base, …) so picking a
    // base that gives nice human-scale delays is awkward; explicit is clearer.
    [200u64, 1_000, 4_000, 15_000]
        .into_iter()
        .chain(std::iter::repeat(15_000))
        .map(Duration::from_millis)
        .map(equal_jitter)
        .take(retries.max(0) as usize)
}

/// Jitter the duration to a random value in `[d/2, d)` — "equal jitter" per
/// AWS's backoff guidance. Avoids tokio_retry's `jitter` which can return
/// near-zero (its range is `[0, d)`), defeating the point of backoff.
fn equal_jitter(d: Duration) -> Duration {
    let factor = 0.5 + rand::random::<f64>() * 0.5;
    Duration::from_secs_f64(d.as_secs_f64() * factor)
}

/// True if the error is a network-layer connection problem (no status received).
/// Used to decide when http→https fallback makes sense: only when the http
/// attempt never reached the server, not when the server returned a status.
fn is_connection_error(err: &Report) -> bool {
    err.chain().any(|e| {
        let Some(reqwest_err) = e.downcast_ref::<reqwest::Error>() else {
            return false;
        };
        (reqwest_err.is_connect() || reqwest_err.is_timeout()) && reqwest_err.status().is_none()
    })
}

/// Classifies an error as transient (should retry) vs permanent.
/// Walks the error chain so wrapped errors (e.g. our timeout hint) still match.
pub(crate) fn is_transient(err: &Report) -> bool {
    err.chain().any(|e| {
        let Some(reqwest_err) = e.downcast_ref::<reqwest::Error>() else {
            return false;
        };
        // Network-layer failures: connect refused, timeout, mid-stream body drop.
        if reqwest_err.is_timeout() || reqwest_err.is_connect() || reqwest_err.is_body() {
            return true;
        }
        // Status errors: 5xx server errors plus 408 (Request Timeout) and
        // 429 (Too Many Requests). Other 4xx are deterministic — don't retry.
        if let Some(status) = reqwest_err.status() {
            let code = status.as_u16();
            return code == 408 || code == 429 || (500..600).contains(&code);
        }
        false
    })
}

/// Retry an async operation on transient errors using `default_backoff_strategy`.
/// Emits a warn! immediately on each transient failure so the user sees flaky
/// infrastructure as it's happening, instead of waiting through the backoff
/// schedule. Successful rescues and final exhaustion don't get extra warnings
/// — the caller surfaces the outcome.
pub(crate) async fn retry_async<F, Fut, T>(verb_label: &str, url: &Url, mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut backoff = default_backoff_strategy(Settings::get().http_retries);
    let mut attempt: usize = 1;
    loop {
        match f().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if !is_transient(&err) {
                    return Err(err);
                }
                let Some(delay) = backoff.next() else {
                    return Err(err);
                };
                warn!(
                    "HTTP {} {} attempt {} failed (transient): {}; retrying in {:?}",
                    verb_label, url, attempt, err, delay
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confique::Layer;
    use indexmap::IndexMap;
    use url::Url;

    // Mutex to ensure tests don't interfere with each other when modifying global settings
    static TEST_SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // Helper to create test settings with specific URL replacements
    fn with_test_settings<F, R>(replacements: IndexMap<String, String>, test_fn: F) -> R
    where
        F: FnOnce() -> R,
    {
        // Lock to prevent parallel tests from interfering with global settings
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();

        // Create settings with custom URL replacements
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.url_replacements = Some(replacements);

        // Set settings for this test
        crate::config::Settings::reset(Some(settings));

        // Run test
        let result = test_fn();

        // Clean up after test
        crate::config::Settings::reset(None);

        result
    }

    #[tokio::test]
    async fn test_get_html_accepts_text_html_without_doctype() {
        let mut server = mockito::Server::new_async().await;
        let expected_body = "<html><body>package index</body></html>";
        let mock = server
            .mock("GET", "/simple")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(expected_body)
            .expect(1)
            .create_async()
            .await;

        let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
        let html = client
            .get_html(format!("{}/simple", server.url()))
            .await
            .unwrap();

        assert_eq!(html, expected_body);
        mock.assert();
    }

    #[tokio::test]
    async fn test_get_html_rejects_non_html_content_type() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/plain")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("<!DOCTYPE html><html></html>")
            .expect(1)
            .create_async()
            .await;

        let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
        let err = client
            .get_html(format!("{}/plain", server.url()))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("Got non-HTML text from"));
        mock.assert();
    }

    // RAII guard that holds the global test lock and resets settings on drop.
    // Use this in async tests so the mutex stays held across .await points
    // without sync/async closure shenanigans.
    struct SettingsGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for SettingsGuard {
        fn drop(&mut self) {
            crate::config::Settings::reset(None);
        }
    }
    fn set_test_http_retries(retries: i64) -> SettingsGuard {
        let lock = TEST_SETTINGS_LOCK.lock().unwrap();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.http_retries = Some(retries);
        crate::config::Settings::reset(Some(settings));
        SettingsGuard { _lock: lock }
    }

    // A tiny in-process HTTP/1.1 responder. Each accepted connection consumes
    // the next response from `responses` and writes it back. Returns the bound
    // port and an Arc counter of connections actually served.
    async fn spawn_canned_server(
        responses: Vec<&'static str>,
    ) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let count = Arc::new(AtomicUsize::new(0));
        let count_inner = count.clone();
        tokio::spawn(async move {
            for resp in responses {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                count_inner.fetch_add(1, Ordering::SeqCst);
                // Drain request headers (read until \r\n\r\n or EOF).
                let mut buf = [0u8; 4096];
                let mut total = Vec::new();
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            total.extend_from_slice(&buf[..n]);
                            if total.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (port, count)
    }

    fn ok_response() -> &'static str {
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK"
    }
    fn bad_gateway_response() -> &'static str {
        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    }
    fn not_found_response() -> &'static str {
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    }
    fn server_error_response() -> &'static str {
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_retry_succeeds_after_two_502s() {
        // 2 retries is enough to verify the rescue path (2 failures + 1 success)
        // without paying the third backoff (~12.5s).
        let _guard = set_test_http_retries(2);
        let (port, count) = spawn_canned_server(vec![
            bad_gateway_response(),
            bad_gateway_response(),
            ok_response(),
        ])
        .await;
        let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let resp = client.get_async(url).await.unwrap();
        assert!(resp.status().is_success());
        // Should have served 3 connections: two 502s + one 200.
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_no_retry_on_404() {
        let _guard = set_test_http_retries(3);
        let (port, count) = spawn_canned_server(vec![not_found_response()]).await;
        let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let err = client.get_async(url).await.unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("404"), "expected 404 in error: {msg}");
        // Should not have retried — only one connection.
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_retry_exhausted_on_persistent_500() {
        // Use 1 retry so the test doesn't pay the full backoff schedule;
        // the behavior under test (exhaustion → final error) is the same.
        let _guard = set_test_http_retries(1);
        // 2 connections: initial + 1 retry.
        let (port, count) =
            spawn_canned_server(vec![server_error_response(), server_error_response()]).await;
        let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let err = client.get_async(url).await.unwrap_err();
        assert!(format!("{err:?}").contains("500"));
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn test_backoff_strategy_yields_requested_count_beyond_schedule() {
        // Regression: a fixed-length schedule used to silently cap retries at 4.
        // Now extra retries should fall back to the longest delay.
        let delays: Vec<_> = default_backoff_strategy(7).collect();
        assert_eq!(delays.len(), 7);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_retries_disabled_fails_immediately() {
        let _guard = set_test_http_retries(0);
        let (port, count) = spawn_canned_server(vec![bad_gateway_response()]).await;
        let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let err = client.get_async(url).await.unwrap_err();
        assert!(format!("{err:?}").contains("502"));
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_simple_string_replacement() {
        let mut replacements = IndexMap::new();
        replacements.insert("github.com".to_string(), "my-proxy.com".to_string());

        with_test_settings(replacements, || {
            let mut url = Url::parse("https://github.com/owner/repo").unwrap();
            apply_url_replacements(&mut url);
            assert_eq!(url.as_str(), "https://my-proxy.com/owner/repo");
        });
    }

    #[test]
    fn test_full_url_string_replacement() {
        let mut replacements = IndexMap::new();
        replacements.insert(
            "https://github.com".to_string(),
            "https://my-proxy.com/artifactory/github-remote".to_string(),
        );

        with_test_settings(replacements, || {
            let mut url = Url::parse("https://github.com/owner/repo").unwrap();
            apply_url_replacements(&mut url);
            assert_eq!(
                url.as_str(),
                "https://my-proxy.com/artifactory/github-remote/owner/repo"
            );
        });
    }

    #[test]
    fn test_protocol_specific_replacement() {
        let mut replacements = IndexMap::new();
        replacements.insert(
            "https://github.com".to_string(),
            "https://secure-proxy.com".to_string(),
        );

        with_test_settings(replacements.clone(), || {
            // HTTPS gets replaced
            let mut url1 = Url::parse("https://github.com/owner/repo").unwrap();
            apply_url_replacements(&mut url1);
            assert_eq!(url1.as_str(), "https://secure-proxy.com/owner/repo");
        });

        with_test_settings(replacements, || {
            // HTTP does not get replaced (no match)
            let mut url2 = Url::parse("http://github.com/owner/repo").unwrap();
            apply_url_replacements(&mut url2);
            assert_eq!(url2.as_str(), "http://github.com/owner/repo");
        });
    }

    #[test]
    fn test_regex_replacement() {
        let mut replacements = IndexMap::new();
        replacements.insert(
            r"regex:https://github\.com".to_string(),
            "https://my-proxy.com".to_string(),
        );

        with_test_settings(replacements, || {
            let mut url = Url::parse("https://github.com/owner/repo").unwrap();
            apply_url_replacements(&mut url);
            assert_eq!(url.as_str(), "https://my-proxy.com/owner/repo");
        });
    }

    #[test]
    fn test_regex_with_capture_groups() {
        let mut replacements = IndexMap::new();
        replacements.insert(
            r"regex:https://github\.com/([^/]+)/([^/]+)".to_string(),
            "https://my-proxy.com/mirror/$1/$2".to_string(),
        );

        with_test_settings(replacements, || {
            let mut url = Url::parse("https://github.com/owner/repo/releases").unwrap();
            apply_url_replacements(&mut url);
            assert_eq!(
                url.as_str(),
                "https://my-proxy.com/mirror/owner/repo/releases"
            );
        });
    }

    #[test]
    fn test_regex_invalid_replacement_url() {
        let mut replacements = IndexMap::new();
        replacements.insert(
            r"regex:https://github\.com/([^/]+)".to_string(),
            "not-a-valid-url".to_string(),
        );

        with_test_settings(replacements, || {
            // Invalid result URL should be ignored, original URL unchanged
            let mut url = Url::parse("https://github.com/owner/repo").unwrap();
            let original = url.clone();
            apply_url_replacements(&mut url);
            assert_eq!(url.as_str(), original.as_str());
        });
    }

    #[test]
    fn test_multiple_replacements_first_match_wins() {
        let mut replacements = IndexMap::new();
        replacements.insert("github.com".to_string(), "first-proxy.com".to_string());
        replacements.insert("github".to_string(), "second-proxy.com".to_string());

        with_test_settings(replacements, || {
            let mut url = Url::parse("https://github.com/owner/repo").unwrap();
            apply_url_replacements(&mut url);
            // First replacement should win
            assert_eq!(url.as_str(), "https://first-proxy.com/owner/repo");
        });
    }

    #[test]
    fn test_no_replacements_configured() {
        let replacements = IndexMap::new(); // Empty

        with_test_settings(replacements, || {
            let mut url = Url::parse("https://github.com/owner/repo").unwrap();
            let original = url.clone();
            apply_url_replacements(&mut url);
            assert_eq!(url.as_str(), original.as_str());
        });
    }

    #[test]
    fn test_regex_complex_patterns() {
        let mut replacements = IndexMap::new();
        // Convert GitHub releases to JFrog Artifactory
        replacements.insert(
            r"regex:https://github\.com/([^/]+)/([^/]+)/releases/download/([^/]+)/(.+)".to_string(),
            "https://artifactory.company.com/artifactory/github-releases/$1/$2/$3/$4".to_string(),
        );

        with_test_settings(replacements, || {
            let mut url =
                Url::parse("https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz")
                    .unwrap();
            apply_url_replacements(&mut url);
            assert_eq!(
                url.as_str(),
                "https://artifactory.company.com/artifactory/github-releases/owner/repo/v1.0.0/file.tar.gz"
            );
        });
    }

    #[test]
    fn test_no_settings_configured() {
        // Test the real apply_url_replacements function with no settings override
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();
        crate::config::Settings::reset(None);

        let mut url = Url::parse("https://github.com/owner/repo").unwrap();
        let original = url.clone();

        // This should not crash and should leave URL unchanged
        apply_url_replacements(&mut url);
        assert_eq!(url.as_str(), original.as_str());
    }

    #[test]
    fn test_replacement_affects_full_url_not_just_hostname() {
        // Test that replacement works on the full URL string, not just hostname
        let mut replacements = IndexMap::new();
        replacements.insert(
            "github.com/owner".to_string(),
            "proxy.com/mirror".to_string(),
        );

        with_test_settings(replacements, || {
            let mut url = Url::parse("https://github.com/owner/repo").unwrap();
            apply_url_replacements(&mut url);
            // This demonstrates that replacement happens on full URL, not just hostname
            assert_eq!(url.as_str(), "https://proxy.com/mirror/repo");
        });
    }

    #[test]
    fn test_path_replacement_example() {
        // Test replacing part of the path, proving it's not hostname-only
        let mut replacements = IndexMap::new();
        replacements.insert("/releases/download/".to_string(), "/artifacts/".to_string());

        with_test_settings(replacements, || {
            let mut url =
                Url::parse("https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz")
                    .unwrap();
            apply_url_replacements(&mut url);
            // Path component was replaced, proving it's full URL replacement
            assert_eq!(
                url.as_str(),
                "https://github.com/owner/repo/artifacts/v1.0.0/file.tar.gz"
            );
        });
    }

    #[test]
    fn test_documentation_examples() {
        // Test the examples from the documentation to ensure they work correctly

        // Example 1: Simple hostname replacement
        let mut replacements = IndexMap::new();
        replacements.insert("github.com".to_string(), "myregistry.net".to_string());

        with_test_settings(replacements, || {
            let mut url = Url::parse("https://github.com/user/repo").unwrap();
            apply_url_replacements(&mut url);
            assert_eq!(url.as_str(), "https://myregistry.net/user/repo");
        });

        // Example 2: Protocol + hostname replacement
        let mut replacements2 = IndexMap::new();
        replacements2.insert(
            "https://github.com".to_string(),
            "https://proxy.corp.com/github-mirror".to_string(),
        );

        with_test_settings(replacements2, || {
            let mut url = Url::parse("https://github.com/user/repo").unwrap();
            apply_url_replacements(&mut url);
            assert_eq!(
                url.as_str(),
                "https://proxy.corp.com/github-mirror/user/repo"
            );
        });

        // Example 3: Domain + path replacement
        let mut replacements3 = IndexMap::new();
        replacements3.insert(
            "github.com/releases/download/".to_string(),
            "cdn.example.com/artifacts/".to_string(),
        );

        with_test_settings(replacements3, || {
            let mut url =
                Url::parse("https://github.com/releases/download/v1.0.0/file.tar.gz").unwrap();
            apply_url_replacements(&mut url);
            assert_eq!(
                url.as_str(),
                "https://cdn.example.com/artifacts/v1.0.0/file.tar.gz"
            );
        });
    }
}
