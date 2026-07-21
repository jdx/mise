use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use eyre::{Report, Result, bail, ensure, eyre};
use regex::Regex;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{ClientBuilder, IntoUrl, Method, Response};
use std::sync::LazyLock as Lazy;
use tokio::io::AsyncWriteExt;
use tokio::sync::OnceCell;
use url::Url;

use crate::cli::version;
use crate::config::Settings;
use crate::file::display_path;
use crate::netrc;
use crate::ui::progress_report::SingleReport;
use crate::ui::time::format_duration;
use crate::{env, file};

pub static HTTP: Lazy<Client> =
    Lazy::new(|| Client::new(Settings::get().http_timeout(), ClientKind::Http).unwrap());

pub static HTTP_FETCH: Lazy<Client> = Lazy::new(|| {
    Client::new(
        Settings::get().configured_fetch_remote_versions_timeout(),
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
/// Origins that returned a hard connection failure during a prefer-offline
/// process. Keep the original error text so a short-circuited request remains
/// actionable rather than hiding the reason the circuit opened.
static UNAVAILABLE_HTTP_HOSTS: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
type RetryStateHandle = Arc<Mutex<RetryState>>;

#[derive(Debug)]
struct UnavailableHttpHost {
    origin: String,
    cause: String,
}

impl std::fmt::Display for UnavailableHttpHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HTTP host {} is unavailable after an earlier connection failure: {}",
            self.origin, self.cause
        )
    }
}

impl std::error::Error for UnavailableHttpHost {}

struct RetryState {
    headers: HeaderMap,
    use_netrc: bool,
}

#[derive(Clone)]
struct SendOnceOptions {
    use_netrc: bool,
    retry_github_oauth_401: bool,
    error_for_status: bool,
    retry_state: Option<RetryStateHandle>,
}

impl SendOnceOptions {
    fn new(retry_state: Option<RetryStateHandle>, use_netrc: bool) -> Self {
        Self {
            use_netrc,
            retry_github_oauth_401: true,
            error_for_status: true,
            retry_state,
        }
    }

    fn allow_error_status(mut self) -> Self {
        self.error_for_status = false;
        self
    }

    fn recursive_retry(&self) -> Self {
        Self {
            use_netrc: false,
            retry_github_oauth_401: false,
            error_for_status: self.error_for_status,
            retry_state: self.retry_state.clone(),
        }
    }
}

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

    fn request_timeout(&self) -> Duration {
        match self.kind {
            ClientKind::Fetch if Settings::get().prefer_offline() => {
                self.timeout.min(Duration::from_secs(3))
            }
            _ => self.timeout,
        }
    }

    pub async fn get_bytes<U: IntoUrl>(&self, url: U) -> Result<impl AsRef<[u8]>> {
        let url = url.into_url()?;
        let resp = self.get_async(url.clone()).await?;
        Ok(resp.bytes().await?)
    }

    pub async fn get_async<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let url = url.into_url()?;
        let headers = host_auth_headers(&url)?;
        self.get_async_with_headers(url, &headers).await
    }

    async fn get_async_with_headers<U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<Response> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url()?;
        let resp = self
            .send_with_https_fallback(Method::GET, url, headers, "GET")
            .await?;
        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub async fn get_async_with_headers_allow_error_status<U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<Response> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url()?;
        self.send_with_https_fallback_allow_error_status(Method::GET, url, headers, "GET")
            .await
    }

    pub async fn head<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let url = url.into_url()?;
        let headers = host_auth_headers(&url)?;
        self.head_async_with_headers(url, &headers).await
    }

    pub async fn head_async_with_headers<U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<Response> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url()?;
        let resp = self
            .send_with_https_fallback(Method::HEAD, url, headers, "HEAD")
            .await?;
        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub async fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        self.get_text_request(url).send().await
    }

    pub fn get_text_request<U: IntoUrl>(&self, url: U) -> TextRequest<'_> {
        // Defer surfacing an invalid URL to `send()` (which returns `Result`) so a
        // bad URL is reported as an error instead of panicking here. See #3547.
        TextRequest {
            client: self,
            url: url.into_url().map_err(|e| e.to_string()),
            extra_headers: HeaderMap::new(),
            retries: Settings::get().http_retries(),
        }
    }

    /// Like get_text but caches results in memory for the duration of the process.
    /// Useful when the same URL will be requested multiple times (e.g., SHASUMS256.txt
    /// when locking multiple platforms). Concurrent requests for the same URL will
    /// wait for the first fetch to complete.
    pub async fn get_text_cached<U: IntoUrl>(&self, url: U) -> Result<String> {
        let url = url.into_url()?;
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
        let url = url.into_url()?;
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
        let url = url.into_url()?;
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
        let url = url.into_url()?;
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
        let headers = host_auth_headers(&url)?;
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
        self.download_file_with_headers_timeout(
            url,
            path,
            headers,
            pr,
            Settings::get().http_download_timeout(),
        )
        .await
    }

    async fn download_file_with_headers_timeout<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        headers: &HeaderMap,
        pr: Option<&dyn SingleReport>,
        total_timeout: Duration,
    ) -> Result<()> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url()?;
        debug!("GET Downloading {} to {}", &url, display_path(path));
        let parent = path.parent().unwrap();
        file::create_dir_all(parent)?;
        let attempt = Arc::new(AtomicUsize::new(0));
        let bytes_received = Arc::new(AtomicU64::new(0));

        // Retry the whole download so a mid-stream chunk failure restarts from
        // byte 0 instead of failing the install. send_once_with_https_fallback
        // (not send_with_https_fallback) is used inside to avoid retry-on-retry.
        let download = retry_async("GET", &url, || {
            let attempt = attempt.clone();
            let bytes_received = bytes_received.clone();
            let request_url = url.clone();
            async move {
                attempt.fetch_add(1, Ordering::Relaxed);
                bytes_received.store(0, Ordering::Relaxed);
                let mut resp = self
                    .send_once_with_https_fallback(Method::GET, request_url, headers, "GET")
                    .await?;
                if let Some(pr) = pr {
                    if let Some(length) = resp.content_length() {
                        pr.set_length(length);
                    }
                    pr.set_position(0);
                }
                let (temp_file, file) = {
                    let path = path.to_path_buf();
                    let parent = parent.to_path_buf();
                    tokio::task::spawn_blocking(move || {
                        let temp_file = tempfile::NamedTempFile::with_prefix_in(path, parent)?;
                        let file = temp_file.reopen()?;
                        Ok::<_, std::io::Error>((temp_file, file))
                    })
                    .await??
                };
                let mut file = tokio::fs::File::from_std(file);
                while let Some(chunk) = resp.chunk().await? {
                    if crate::ui::ctrlc::is_cancelled() {
                        bail!("download cancelled by user");
                    }
                    file.write_all(&chunk).await?;
                    bytes_received.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    if let Some(pr) = pr {
                        pr.inc(chunk.len() as u64);
                    }
                }
                file.shutdown().await?;
                drop(file);
                Ok(temp_file)
            }
        });

        let temp_file = match tokio::time::timeout(total_timeout, download).await {
            Ok(result) => result?,
            Err(_) => bail!(
                "HTTP download timed out after {} for {} (attempt {}, {} bytes received; change with `http_download_timeout` or env `MISE_HTTP_DOWNLOAD_TIMEOUT`)",
                format_duration(total_timeout),
                url,
                attempt.load(Ordering::Relaxed),
                bytes_received.load(Ordering::Relaxed),
            ),
        };

        // Complete the atomic rename after the cancellable transfer budget. A
        // blocking task cannot be cancelled once it starts, so keeping it out
        // of `timeout` prevents us from returning an error while it can still
        // install the destination in the background.
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || temp_file.persist(path)).await??;
        Ok(())
    }

    async fn send_with_https_fallback(
        &self,
        method: Method,
        url: Url,
        headers: &HeaderMap,
        verb_label: &str,
    ) -> Result<Response> {
        self.send_with_https_fallback_with_retries(
            method,
            url,
            headers,
            verb_label,
            Settings::get().http_retries(),
            true,
        )
        .await
    }

    async fn send_with_https_fallback_allow_error_status(
        &self,
        method: Method,
        url: Url,
        headers: &HeaderMap,
        verb_label: &str,
    ) -> Result<Response> {
        self.send_with_https_fallback_with_retries(
            method,
            url,
            headers,
            verb_label,
            Settings::get().http_retries(),
            false,
        )
        .await
    }

    async fn send_with_https_fallback_with_retries(
        &self,
        method: Method,
        url: Url,
        headers: &HeaderMap,
        verb_label: &str,
        retries: i64,
        error_for_status: bool,
    ) -> Result<Response> {
        let retry_state = Arc::new(Mutex::new(RetryState {
            headers: headers.clone(),
            use_netrc: true,
        }));
        retry_async_with_retries(verb_label, &url, retries, || async {
            let (headers, use_netrc) = {
                let state = retry_state.lock().unwrap();
                (state.headers.clone(), state.use_netrc)
            };
            let options = SendOnceOptions::new(Some(retry_state.clone()), use_netrc);
            let options = if error_for_status {
                options
            } else {
                options.allow_error_status()
            };
            self.send_once_with_https_fallback_with_retry_headers(
                method.clone(),
                url.clone(),
                &headers,
                verb_label,
                options,
            )
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
        self.send_once_with_https_fallback_with_retry_headers(
            method,
            url,
            headers,
            verb_label,
            SendOnceOptions::new(None, true),
        )
        .await
    }

    async fn send_once_with_https_fallback_with_retry_headers(
        &self,
        method: Method,
        url: Url,
        headers: &HeaderMap,
        verb_label: &str,
        options: SendOnceOptions,
    ) -> Result<Response> {
        match self
            .send_once_with_retry_headers(
                method.clone(),
                url.clone(),
                headers,
                verb_label,
                options.clone(),
            )
            .await
        {
            Ok(resp) => Ok(resp),
            Err(err)
                if url.scheme() == "http"
                    && (is_connection_error(&err) || is_unavailable_http_host_error(&err)) =>
            {
                let mut url = url;
                url.set_scheme("https").unwrap();
                self.send_once_with_retry_headers(method, url, headers, verb_label, options)
                    .await
            }
            Err(err) => Err(err),
        }
    }

    async fn send_once_with_retry_headers(
        &self,
        method: Method,
        url: Url,
        headers: &HeaderMap,
        verb_label: &str,
        options: SendOnceOptions,
    ) -> Result<Response> {
        self.send_once_inner(method, url, headers, verb_label, options)
            .await
    }

    async fn send_once_inner(
        &self,
        method: Method,
        mut url: Url,
        headers: &HeaderMap,
        verb_label: &str,
        options: SendOnceOptions,
    ) -> Result<Response> {
        let original_url = url.clone();
        apply_url_replacements(&mut url);
        let host_key = http_host_key(&url);
        if Settings::get().prefer_offline()
            && let Some(host) = &host_key
            && let Some(cause) = UNAVAILABLE_HTTP_HOSTS.lock().unwrap().get(host).cloned()
        {
            return Err(UnavailableHttpHost {
                origin: host.clone(),
                cause,
            }
            .into());
        }
        debug!("{} {}", verb_label, &url);

        // Apply netrc credentials after URL replacement.
        //
        // netrc is treated as a *fallback*, mirroring curl's behavior: an
        // explicit Authorization header (e.g. the forge token resolved by
        // `host_auth_headers` from GITHUB_TOKEN/gh/github_tokens.toml) wins
        // over netrc. The one exception is when a URL replacement actually
        // redirected the request to a different URL — in that case the
        // pre-existing auth header was built for the *original* host and is
        // likely wrong for the replacement target, so netrc (scoped to the
        // new host) should override it. This preserves the #7164 use case
        // (replace a public URL with a private mirror authenticated via
        // netrc) without clobbering forge tokens on un-redirected requests.
        let mut final_headers = headers.clone();
        if options.use_netrc {
            final_headers =
                apply_netrc_credentials(final_headers, &original_url, &url, netrc_headers(&url));
        }

        let request_timeout = self.request_timeout();
        let mut req = self.reqwest.request(method.clone(), url.clone());
        if matches!(self.kind, ClientKind::Fetch) {
            req = req.timeout(request_timeout);
        }
        req = req.headers(final_headers.clone());
        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(err) => {
                let err = err.without_url();
                if Settings::get().prefer_offline()
                    && is_hard_connection_failure(&err)
                    && let Some(host) = host_key
                {
                    UNAVAILABLE_HTTP_HOSTS
                        .lock()
                        .unwrap()
                        .insert(host, err.to_string());
                }
                if err.is_timeout() {
                    let (setting, env_var) = match self.kind {
                        ClientKind::Http => ("http_timeout", "MISE_HTTP_TIMEOUT"),
                        ClientKind::Fetch => (
                            "fetch_remote_versions_timeout",
                            "MISE_FETCH_REMOTE_VERSIONS_TIMEOUT",
                        ),
                    };
                    let hint = format!(
                        "HTTP timed out after {} for {} (change with `{}` or env `{}`).",
                        format_duration(request_timeout),
                        url,
                        setting,
                        env_var
                    );
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
        if options.retry_github_oauth_401
            && let Some(stale_access_token) =
                stale_github_oauth_unauthorized_token(&original_url, &final_headers, &resp)
            && let Some(host) = original_url.host_str()
        {
            match crate::github::oauth::refresh_cached_token_for_host(host, &stale_access_token)
                .await
            {
                Ok(Some(token)) => {
                    let mut headers = headers.clone();
                    if let Ok(value) = HeaderValue::from_str(format!("Bearer {token}").as_str()) {
                        headers.insert(AUTHORIZATION, value);
                        if let Some(retry_state) = &options.retry_state {
                            *retry_state.lock().unwrap() = RetryState {
                                headers: headers.clone(),
                                use_netrc: false,
                            };
                        }
                        debug!(
                            "{} {} retrying with refreshed GitHub OAuth token after 401",
                            verb_label, &url
                        );
                        return Box::pin(self.send_once_inner(
                            method,
                            original_url,
                            &headers,
                            verb_label,
                            options.recursive_retry(),
                        ))
                        .await;
                    } else {
                        debug!(
                            "refreshed GitHub OAuth token contains invalid header bytes; skipping retry"
                        );
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    crate::github::oauth::log_refresh_error(&err);
                }
            }
        }
        if options.error_for_status && is_github_forbidden(&url, &resp) {
            let status = resp.status();
            let status_error = resp
                .error_for_status_ref()
                .expect_err("403 response should be an error");
            let used_github_token = final_headers.contains_key(AUTHORIZATION);
            let rate_limit = github_rate_limit_summary(&resp);
            let body = resp.text().await.unwrap_or_default();
            // Retry without auth when the response mentions IP allow lists: GitHub App
            // installation tokens (`ghs_*`) get 403 on public API resources for orgs with IP
            // allow lists; stripping auth avoids that path.
            // https://github.com/orgs/community/discussions/191185
            // https://github.com/jdx/mise/discussions/9119
            if used_github_token && body.contains("IP allow list") {
                let mut headers = final_headers;
                headers.remove(AUTHORIZATION);
                debug!(
                    "{} {} retrying without GitHub auth after {}",
                    verb_label, &url, status
                );
                return Box::pin(self.send_once_inner(
                    method,
                    original_url,
                    &headers,
                    verb_label,
                    options.recursive_retry(),
                ))
                .await;
            }
            return Err(github_forbidden_report(
                status_error,
                used_github_token,
                rate_limit,
                &body,
            ));
        }
        if options.error_for_status {
            resp.error_for_status_ref()?;
        }
        Ok(resp)
    }
}

pub struct TextRequest<'a> {
    client: &'a Client,
    // Parsed lazily by `get_text_request`; an invalid URL surfaces as an error in
    // `send()` rather than a panic. See #3547.
    url: Result<Url, String>,
    extra_headers: HeaderMap,
    retries: i64,
}

impl TextRequest<'_> {
    pub fn headers(mut self, headers: &HeaderMap) -> Self {
        self.extra_headers.extend(headers.clone());
        self
    }

    pub fn retries(mut self, retries: i64) -> Self {
        self.retries = retries;
        self
    }

    pub async fn send(mut self) -> Result<String> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let mut url = self.url.clone().map_err(|e| eyre!(e))?;
        // Merge GitHub headers with any extra headers provided
        let mut headers = host_auth_headers(&url)?;
        headers.extend(self.extra_headers.clone());
        let resp = self
            .client
            .send_with_https_fallback_with_retries(
                Method::GET,
                url.clone(),
                &headers,
                "GET",
                self.retries,
                true,
            )
            .await?;
        let text = resp.text().await?;
        if text.starts_with("<!DOCTYPE html>") {
            if url.scheme() == "http" {
                // try with https since http may be blocked
                url.set_scheme("https").unwrap();
                self.url = Ok(url);
                return Box::pin(self.send()).await;
            }
            bail!("Got HTML instead of text from {}", url);
        }
        Ok(text)
    }
}

fn is_github_forbidden(url: &Url, resp: &Response) -> bool {
    resp.status() == StatusCode::FORBIDDEN && url.host_str() == Some("api.github.com")
}

fn github_forbidden_report(
    status_error: reqwest::Error,
    used_github_token: bool,
    rate_limit: Option<String>,
    body: &str,
) -> Report {
    let token_status = if used_github_token { "yes" } else { "no" };
    let rate_limit = rate_limit
        .map(|summary| format!("\ngithub rate limit: {summary}"))
        .unwrap_or_default();
    let body = format_response_body(body);
    eyre!("{status_error}\ngithub auth: {token_status}{rate_limit}\ngithub response: {body}")
}

fn format_response_body(body: &str) -> String {
    const MAX_BODY_CHARS: usize = 4096;
    if body.trim().is_empty() {
        return "<empty>".to_string();
    }

    let mut chars = body.chars();
    let mut formatted: String = chars.by_ref().take(MAX_BODY_CHARS).collect();
    if chars.next().is_some() {
        formatted.push_str("\n<truncated>");
    }
    formatted
}

fn github_rate_limit_summary(resp: &Response) -> Option<String> {
    let headers = resp.headers();
    let limit = headers
        .get("x-ratelimit-limit")
        .and_then(|h| h.to_str().ok());
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|h| h.to_str().ok());
    let resource = headers
        .get("x-ratelimit-resource")
        .and_then(|h| h.to_str().ok());
    let reset = headers
        .get("x-ratelimit-reset")
        .and_then(|h| h.to_str().ok());

    if limit.is_none() && remaining.is_none() && resource.is_none() && reset.is_none() {
        return None;
    }

    Some(format!(
        "{}/{}{}{}",
        remaining.unwrap_or("?"),
        limit.unwrap_or("?"),
        resource
            .map(|resource| format!(" ({resource})"))
            .unwrap_or_default(),
        reset
            .map(|reset| format!(", resets at {reset}"))
            .unwrap_or_default()
    ))
}

fn stale_github_oauth_unauthorized_token(
    url: &Url,
    headers: &HeaderMap,
    resp: &Response,
) -> Option<String> {
    if resp.status() != StatusCode::UNAUTHORIZED || !crate::github::is_github_api_url(url) {
        return None;
    }
    let host = url.host_str()?;
    let token = crate::github::oauth::cached_access_token_for_host(host)?;
    let header_token = headers
        .get(AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))?;
    if header_token == token {
        Some(header_token.to_string())
    } else {
        None
    }
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

fn host_auth_headers(url: &Url) -> Result<HeaderMap> {
    if crate::github::is_github_api_url(url) {
        return crate::github::get_headers(url.as_str());
    }

    let Some(host) = url.host_str() else {
        return Ok(HeaderMap::new());
    };

    let is_gitlab = host == "gitlab.com" || crate::gitlab::is_gitlab_host(host);
    if is_gitlab {
        return Ok(crate::gitlab::get_headers(url.as_str()));
    }

    let is_forgejo = host == "codeberg.org" || crate::forgejo::is_forgejo_host(host);
    if is_forgejo {
        return Ok(crate::forgejo::get_headers(url.as_str()));
    }

    Ok(HeaderMap::new())
}

/// Decide whether netrc credentials should be applied to a request.
///
/// netrc is a *fallback*: an explicit Authorization header (e.g. a forge
/// token resolved from GITHUB_TOKEN/gh/github_tokens.toml) takes precedence
/// over netrc, matching curl's behavior. The exception is a URL replacement
/// that redirected the request to a *different host*: the existing auth
/// header was built for the original host and is likely wrong for the
/// replacement target, so netrc (which is itself scoped to the new host) is
/// allowed to override it. A same-host rewrite (e.g. a path-only replacement)
/// keeps the existing auth, since the forge token is still valid for that host.
fn netrc_should_apply(host_changed: bool, has_existing_auth: bool) -> bool {
    host_changed || !has_existing_auth
}

/// Merge `netrc` credentials into `final_headers`, honoring the fallback
/// policy in [`netrc_should_apply`]. `original_url` is the URL before any
/// `apply_url_replacements` rewrite and `url` is the (possibly rewritten)
/// URL actually being requested; a change of *host* means the request was
/// redirected to a different server, which lets netrc override an existing
/// auth header. Netrc values are `insert`ed (not `extend`ed) so they replace
/// a pre-existing Authorization rather than appending a duplicate one.
fn apply_netrc_credentials(
    mut final_headers: HeaderMap,
    original_url: &Url,
    url: &Url,
    netrc: HeaderMap,
) -> HeaderMap {
    // Compare host only: netrc lookup and forge-token selection are both
    // host-scoped, so a path/query-only rewrite on the same host must not
    // let netrc clobber a still-valid forge token.
    let host_changed = url.host() != original_url.host();
    let has_auth = final_headers.contains_key(AUTHORIZATION);
    if netrc_should_apply(host_changed, has_auth) {
        for (name, value) in netrc {
            if let Some(name) = name {
                final_headers.insert(name, value);
            }
        }
    }
    final_headers
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

pub(crate) fn default_backoff_strategy(retries: i64) -> impl Iterator<Item = Duration> {
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

fn http_host_key(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let port = url.port_or_known_default()?;
    Some(format!("{}://{host}:{port}", url.scheme()))
}

fn is_unavailable_http_host_error(err: &Report) -> bool {
    err.chain()
        .any(|err| err.downcast_ref::<UnavailableHttpHost>().is_some())
}

/// hyper-util exposes DNS failures in the error chain as a `dns error` source,
/// but reqwest intentionally erases the concrete connector type. Match that
/// stable connector error label rather than platform-specific getaddrinfo text.
fn is_dns_error(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(source) = current {
        if source.to_string() == "dns error" {
            return true;
        }
        current = source.source();
    }
    false
}

fn is_hard_connection_failure(err: &reqwest::Error) -> bool {
    is_dns_error(err) || (err.is_connect() && !err.is_timeout())
}

/// Classifies an error as transient (should retry) vs permanent.
/// Walks the error chain so wrapped errors (e.g. our timeout hint) still match.
pub(crate) fn is_transient(err: &Report) -> bool {
    if is_dns_error(err.as_ref()) {
        return false;
    }
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
pub(crate) async fn retry_async<F, Fut, T>(verb_label: &str, url: &Url, f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    retry_async_with_retries(verb_label, url, Settings::get().http_retries(), f).await
}

pub(crate) async fn retry_async_with_retries<F, Fut, T>(
    verb_label: &str,
    url: &Url,
    retries: i64,
    mut f: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut backoff = default_backoff_strategy(retries);
    let mut attempt: usize = 1;
    loop {
        let started_at = Instant::now();
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
                    "HTTP {} {} attempt {} failed after {} (transient): {}; retrying in {:?}",
                    verb_label,
                    url,
                    attempt,
                    format_duration(started_at.elapsed()),
                    err,
                    delay
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
    use std::path::PathBuf;
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
    async fn test_invalid_url_returns_error_not_panic() {
        // A relative/invalid URL must return an error rather than panicking
        // (previously `into_url().unwrap()` crashed the process). See #3547.
        let client = Client::new(Duration::from_secs(1), ClientKind::Http).unwrap();
        assert!(client.get_bytes("").await.is_err());
        assert!(client.head("").await.is_err());
        assert!(client.get_text("").await.is_err());
        assert!(client.get_text_request("").send().await.is_err());
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
    fn set_test_prefer_offline(http_retries: i64) -> SettingsGuard {
        let lock = TEST_SETTINGS_LOCK.lock().unwrap();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.prefer_offline = Some(true);
        settings.http_retries = Some(http_retries);
        crate::config::Settings::reset(Some(settings));
        SettingsGuard { _lock: lock }
    }
    fn set_test_offline() -> SettingsGuard {
        let lock = TEST_SETTINGS_LOCK.lock().unwrap();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.offline = Some(true);
        crate::config::Settings::reset(Some(settings));
        SettingsGuard { _lock: lock }
    }

    struct UnavailableHostsGuard {
        host_keys: Vec<String>,
    }
    impl UnavailableHostsGuard {
        fn new(host_keys: Vec<String>) -> Self {
            let mut unavailable = UNAVAILABLE_HTTP_HOSTS.lock().unwrap();
            for host_key in &host_keys {
                unavailable.remove(host_key);
            }
            drop(unavailable);
            Self { host_keys }
        }
    }
    impl Drop for UnavailableHostsGuard {
        fn drop(&mut self) {
            let mut unavailable = UNAVAILABLE_HTTP_HOSTS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for host_key in &self.host_keys {
                unavailable.remove(host_key);
            }
        }
    }

    struct GithubOauthSettingsGuard {
        _settings_lock: std::sync::MutexGuard<'static, ()>,
        _github_env_lock: std::sync::MutexGuard<'static, ()>,
        vars: Vec<(&'static str, Option<String>)>,
    }

    impl Drop for GithubOauthSettingsGuard {
        fn drop(&mut self) {
            for (key, value) in &self.vars {
                if let Some(value) = value {
                    crate::env::set_var(key, value);
                } else {
                    crate::env::remove_var(key);
                }
            }
            crate::github::oauth::test_support::clear_cache_path();
            crate::config::Settings::reset(None);
        }
    }

    fn set_test_github_oauth(server_url: &str, cache_path: PathBuf) -> GithubOauthSettingsGuard {
        let settings_lock = TEST_SETTINGS_LOCK.lock().unwrap();
        let github_env_lock = crate::github::TEST_ENV_LOCK.lock().unwrap();
        let vars = vec![
            ("MISE_EXPERIMENTAL", std::env::var("MISE_EXPERIMENTAL").ok()),
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
            ("MISE_GITHUB_TOKEN", std::env::var("MISE_GITHUB_TOKEN").ok()),
            ("GITHUB_API_TOKEN", std::env::var("GITHUB_API_TOKEN").ok()),
            ("GITHUB_TOKEN", std::env::var("GITHUB_TOKEN").ok()),
        ];

        crate::env::set_var("MISE_EXPERIMENTAL", "1");
        crate::env::set_var("MISE_GITHUB_OAUTH_CLIENT_ID", "Iv1.mock");
        crate::env::set_var("MISE_GITHUB_OAUTH_AUTH_URL", format!("{server_url}/login"));
        crate::env::set_var("MISE_GITHUB_OAUTH_API_URL", format!("{server_url}/api/v3"));
        crate::env::remove_var("MISE_GITHUB_OAUTH_SCOPES");
        crate::env::remove_var("MISE_GITHUB_TOKEN");
        crate::env::remove_var("GITHUB_API_TOKEN");
        crate::env::remove_var("GITHUB_TOKEN");
        crate::github::oauth::test_support::set_cache_path(cache_path);
        crate::config::Settings::reset(None);

        GithubOauthSettingsGuard {
            _settings_lock: settings_lock,
            _github_env_lock: github_env_lock,
            vars,
        }
    }

    // A tiny in-process HTTP/1.1 responder. Each accepted connection consumes
    // the next response from `responses` and writes it back. Returns the bound
    // port and an Arc counter of connections actually served.
    async fn spawn_canned_server(
        responses: Vec<&'static str>,
    ) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let (port, count, _) = spawn_recording_server(responses).await;
        (port, count)
    }

    async fn spawn_recording_server(
        responses: Vec<&'static str>,
    ) -> (
        u16,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let count = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let count_inner = count.clone();
        let requests_inner = requests.clone();
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
                requests_inner
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&total).to_string());
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (port, count, requests)
    }

    async fn spawn_trickling_server() -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await;
            if socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n",
                )
                .await
                .is_err()
            {
                return;
            }
            loop {
                if socket.write_all(b"x").await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        port
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
    fn unauthorized_response() -> &'static str {
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 15\r\nConnection: close\r\n\r\nBad credentials"
    }
    fn github_forbidden_response() -> &'static str {
        concat!(
            "HTTP/1.1 403 Forbidden\r\n",
            "Content-Type: application/json\r\n",
            "X-RateLimit-Limit: 5000\r\n",
            "X-RateLimit-Remaining: 42\r\n",
            "X-RateLimit-Resource: core\r\n",
            "X-RateLimit-Reset: 1781337353\r\n",
            "Content-Length: 47\r\n",
            "Connection: close\r\n",
            "\r\n",
            r#"{"message":"secondary rate limit","docs":"url"}"#
        )
    }
    fn github_oauth_token_response() -> &'static str {
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 51\r\nConnection: close\r\n\r\n{\"access_token\":\"ghu-refreshed\",\"expires_in\":28800}"
    }
    fn json_empty_array_response() -> &'static str {
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]"
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_github_oauth_401_refreshes_and_retries_once() {
        let (port, count, requests) = spawn_recording_server(vec![
            unauthorized_response(),
            github_oauth_token_response(),
            json_empty_array_response(),
        ])
        .await;
        let server_url = format!("http://127.0.0.1:{port}");
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("github-oauth-tokens.toml");
        let _guard = set_test_github_oauth(&server_url, cache_path.clone());
        let settings = crate::config::Settings::get();
        let cache_key = crate::github::oauth::test_support::cache_key(
            "127.0.0.1",
            "Iv1.mock",
            settings.github.oauth_scopes.trim(),
        );
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

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer ghu-stale"));
        let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
        let text = client
            .get_text_request(format!("{server_url}/api/v3/repos/owner/repo/releases"))
            .headers(&headers)
            .send()
            .await
            .unwrap_or_else(|err| {
                let requests = requests.lock().unwrap();
                panic!(
                    "request failed: {err:#}\nrequests:\n{}",
                    requests.join("\n---\n")
                );
            });

        assert_eq!(text, "[]");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
        let requests = requests.lock().unwrap();
        let first_request = requests[0].to_ascii_lowercase();
        let refresh_request = requests[1].to_ascii_lowercase();
        let retry_request = requests[2].to_ascii_lowercase();
        assert!(first_request.contains("get /api/v3/repos/owner/repo/releases"));
        assert!(first_request.contains("authorization: bearer ghu-stale"));
        assert!(refresh_request.contains("post /login/oauth/access_token"));
        assert!(retry_request.contains("get /api/v3/repos/owner/repo/releases"));
        assert!(retry_request.contains("authorization: bearer ghu-refreshed"));
        let cache = std::fs::read_to_string(cache_path).unwrap();
        assert!(cache.contains("ghu-refreshed"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_github_forbidden_report_includes_body_and_auth_state() {
        let (port, _count) = spawn_canned_server(vec![github_forbidden_response()]).await;
        let url = format!("http://127.0.0.1:{port}/repos/microsoft/edit/releases");
        let resp = reqwest::Client::new().get(url).send().await.unwrap();
        let rate_limit = github_rate_limit_summary(&resp);
        let status_error = resp
            .error_for_status_ref()
            .expect_err("403 response should be an error");
        let body = resp.text().await.unwrap();
        let err = github_forbidden_report(status_error, true, rate_limit, &body);
        let msg = format!("{err:?}");

        assert!(msg.contains("github auth: yes"));
        assert!(msg.contains("github rate limit: 42/5000 (core), resets at 1781337353"));
        assert!(msg.contains(r#"{"message":"secondary rate limit","docs":"url"}"#));
    }

    #[test]
    fn test_netrc_should_apply_treats_netrc_as_fallback() {
        // No existing auth → netrc fills in (normal fallback).
        assert!(netrc_should_apply(false, false));
        // Explicit auth (e.g. forge token) on a same-host request →
        // netrc must NOT clobber it. This is the regression guard for
        // private GitHub release-asset downloads where a netrc github
        // entry was overriding the resolved Bearer token.
        assert!(!netrc_should_apply(false, true));
        // Host changed via URL replacement → existing auth was built for the
        // original host, so netrc (scoped to the new host) wins.
        assert!(netrc_should_apply(true, true));
        assert!(netrc_should_apply(true, false));
    }

    fn basic_netrc_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, HeaderValue::from_static("Basic bmV0cmM="));
        h
    }

    fn auth_value(headers: &HeaderMap) -> Vec<String> {
        headers
            .get_all(AUTHORIZATION)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn test_apply_netrc_keeps_forge_token_on_un_redirected_url() {
        // Regression: a netrc entry for api.github.com must NOT override the
        // Bearer forge token when the URL was not rewritten. Previously this
        // clobbered the token and broke private release-asset downloads.
        let url: Url = "https://api.github.com/repos/o/r/releases/assets/1"
            .parse()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer forge-token"),
        );

        let out = apply_netrc_credentials(headers, &url, &url, basic_netrc_headers());
        // Exactly one Authorization header, still the forge token.
        assert_eq!(auth_value(&out), vec!["Bearer forge-token".to_string()]);
    }

    #[test]
    fn test_apply_netrc_fills_in_when_no_existing_auth() {
        let url: Url = "https://example.com/file".parse().unwrap();
        let out = apply_netrc_credentials(HeaderMap::new(), &url, &url, basic_netrc_headers());
        assert_eq!(auth_value(&out), vec!["Basic bmV0cmM=".to_string()]);
    }

    #[test]
    fn test_apply_netrc_overrides_existing_auth_when_url_redirected() {
        // #7164 use case: a URL replacement redirected the request to a
        // private mirror. The pre-existing auth header was built for the
        // original host, so netrc (scoped to the new host) must win — and
        // replace, not duplicate, the Authorization header.
        let original: Url = "https://public.example.com/file".parse().unwrap();
        let redirected: Url = "https://mirror.internal/file".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer stale"));

        let out = apply_netrc_credentials(headers, &original, &redirected, basic_netrc_headers());
        assert_eq!(auth_value(&out), vec!["Basic bmV0cmM=".to_string()]);
    }

    #[test]
    fn test_apply_netrc_keeps_forge_token_on_same_host_path_rewrite() {
        // A URL replacement that only rewrites the path/query on the SAME host
        // must not let netrc override the forge token: the token is still valid
        // for that host, and netrc is host-scoped anyway.
        let original: Url = "https://github.com/o/r/releases/download/v1/f.tar.gz"
            .parse()
            .unwrap();
        let rewritten: Url = "https://github.com/o/r/releases/download/v1/f-linux.tar.gz"
            .parse()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer forge-token"),
        );

        let out = apply_netrc_credentials(headers, &original, &rewritten, basic_netrc_headers());
        assert_eq!(auth_value(&out), vec!["Bearer forge-token".to_string()]);
    }

    #[test]
    fn test_format_response_body_handles_empty_and_truncates() {
        assert_eq!(format_response_body(" \n\t"), "<empty>");

        let body = "a".repeat(4097);
        let formatted = format_response_body(&body);
        assert_eq!(formatted.strip_suffix("\n<truncated>").unwrap().len(), 4096);
        assert!(formatted.ends_with("\n<truncated>"));
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
    async fn test_prefer_offline_disables_http_retries() {
        let _guard = set_test_prefer_offline(3);
        let (port, count) = spawn_canned_server(vec![bad_gateway_response(), ok_response()]).await;
        let url: Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let err = client.get_async(url).await.unwrap_err();

        assert!(format!("{err:?}").contains("502"));
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            Settings::get().fetch_remote_versions_timeout(),
            Duration::from_secs(3)
        );
    }

    #[test]
    fn test_fetch_client_applies_prefer_offline_timeout_at_request_time() {
        let client = Client::new(Duration::from_secs(30), ClientKind::Fetch).unwrap();
        let _guard = set_test_prefer_offline(3);

        assert_eq!(client.request_timeout(), Duration::from_secs(3));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_reqwest_dns_error_is_not_transient_and_opens_circuit() {
        let _settings_guard = set_test_prefer_offline(3);
        let timeout = Duration::from_secs(3);
        let client = Client {
            reqwest: Client::_new()
                .no_proxy()
                .read_timeout(timeout)
                .connect_timeout(timeout)
                .build()
                .unwrap(),
            timeout,
            kind: ClientKind::Fetch,
        };
        let url: Url = "https://mise-dns-regression.invalid/?token=secret"
            .parse()
            .unwrap();
        let host_key = http_host_key(&url).unwrap();
        let _hosts_guard = UnavailableHostsGuard::new(vec![host_key.clone()]);

        let err = client.get_async(url).await.unwrap_err();

        assert!(is_dns_error(err.as_ref()), "unexpected error: {err:#}");
        assert!(!is_transient(&err));
        assert!(
            UNAVAILABLE_HTTP_HOSTS
                .lock()
                .unwrap()
                .contains_key(&host_key)
        );
        assert!(
            !UNAVAILABLE_HTTP_HOSTS
                .lock()
                .unwrap()
                .get(&host_key)
                .unwrap()
                .contains("token=secret")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_circuit_broken_http_origin_falls_back_to_https() {
        let _settings_guard = set_test_prefer_offline(3);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let http_url: Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
        let https_url: Url = format!("https://127.0.0.1:{port}/").parse().unwrap();
        let http_origin = http_host_key(&http_url).unwrap();
        let https_origin = http_host_key(&https_url).unwrap();
        let _hosts_guard = UnavailableHostsGuard::new(vec![http_origin.clone(), https_origin]);
        UNAVAILABLE_HTTP_HOSTS
            .lock()
            .unwrap()
            .insert(http_origin, "connection refused".to_string());

        let accepted = Arc::new(AtomicUsize::new(0));
        let accepted_inner = accepted.clone();
        let server = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                accepted_inner.fetch_add(1, Ordering::SeqCst);
                let _ = socket.shutdown().await;
            }
        });

        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let err = client.get_async(http_url).await.unwrap_err();
        server.await.unwrap();

        assert_eq!(accepted.load(Ordering::SeqCst), 1);
        assert!(!is_unavailable_http_host_error(&err));
    }

    #[test]
    fn test_unavailable_host_error_preserves_original_cause() {
        let err: Report = UnavailableHttpHost {
            origin: "https://example.com:443".to_string(),
            cause: "connection refused".to_string(),
        }
        .into();

        assert!(is_unavailable_http_host_error(&err));
        assert!(err.to_string().contains("connection refused"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_circuit_breaker_is_disabled_without_prefer_offline() {
        let _settings_guard = set_test_http_retries(0);
        let (port, count) = spawn_canned_server(vec![ok_response()]).await;
        let url: Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
        let host_key = http_host_key(&url).unwrap();
        let _hosts_guard = UnavailableHostsGuard::new(vec![host_key.clone()]);
        UNAVAILABLE_HTTP_HOSTS
            .lock()
            .unwrap()
            .insert(host_key, "connection refused".to_string());

        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let resp = client.get_async(url).await.unwrap();

        assert!(resp.status().is_success());
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_total_timeout_bounds_trickling_response() {
        let port = spawn_trickling_server().await;
        let url = format!("http://127.0.0.1:{port}/artifact.tar.gz");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact.tar.gz");
        // The server sends a byte every 10ms, so the 100ms idle read timeout
        // never fires. The separate total budget must still end the download.
        let client = Client::new(Duration::from_millis(100), ClientKind::Http).unwrap();
        let started_at = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            client.download_file_with_headers_timeout(
                &url,
                &path,
                &HeaderMap::new(),
                None,
                Duration::from_millis(500),
            ),
        )
        .await
        .expect("download timeout regression test exceeded its independent deadline");
        let err = result.unwrap_err();
        let message = err.to_string();

        assert!(started_at.elapsed() < Duration::from_secs(5));
        assert!(message.contains("HTTP download timed out after 500.0ms"));
        assert!(message.contains(&url));
        assert!(message.contains("attempt 1"));
        assert!(message.contains("bytes received"));
        assert!(!message.contains("attempt 1, 0 bytes received"));
        assert!(message.contains("http_download_timeout"));
        assert!(message.contains("MISE_HTTP_DOWNLOAD_TIMEOUT"));
        assert!(!path.exists());
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

    #[tokio::test(flavor = "current_thread")]
    async fn test_text_request_can_override_retry_count() {
        let _guard = set_test_http_retries(3);
        let (port, count) = spawn_canned_server(vec![
            bad_gateway_response(),
            bad_gateway_response(),
            ok_response(),
        ])
        .await;
        let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let err = client
            .get_text_request(url)
            .retries(1)
            .send()
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("502"));
        // Should stop after the initial request plus the single overridden retry.
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_text_request_respects_offline_mode() {
        let _guard = set_test_offline();
        let (port, count) = spawn_canned_server(vec![ok_response()]).await;
        let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let err = client.get_text_request(url).send().await.unwrap_err();
        assert_eq!(err.to_string(), "offline mode is enabled");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
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
