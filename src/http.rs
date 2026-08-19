use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use eyre::{Report, Result, WrapErr, bail, ensure, eyre};
use regex::Regex;
use reqwest::StatusCode;
use reqwest::header::{
    ACCEPT_ENCODING, AUTHORIZATION, CONTENT_RANGE, CONTENT_TYPE, DATE, ETAG, HeaderMap,
    HeaderValue, IF_RANGE, LAST_MODIFIED, RANGE,
};
use reqwest::{ClientBuilder, IntoUrl, Method, Response};
use serde::{Deserialize, Serialize};
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
    Lazy::new(|| Client::new_shared(Settings::get().http_timeout(), ClientKind::Http));

pub static HTTP_FETCH: Lazy<Client> = Lazy::new(|| {
    Client::new_shared(
        Settings::get().configured_fetch_remote_versions_timeout(),
        ClientKind::Fetch,
    )
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
    allow_range_not_satisfiable: bool,
    retry_state: Option<RetryStateHandle>,
}

impl SendOnceOptions {
    fn new(retry_state: Option<RetryStateHandle>, use_netrc: bool) -> Self {
        Self {
            use_netrc,
            retry_github_oauth_401: true,
            error_for_status: true,
            allow_range_not_satisfiable: false,
            retry_state,
        }
    }

    fn allow_error_status(mut self) -> Self {
        self.error_for_status = false;
        self
    }

    fn allow_range_not_satisfiable(mut self) -> Self {
        self.allow_range_not_satisfiable = true;
        self
    }

    fn recursive_retry(&self) -> Self {
        Self {
            use_netrc: false,
            retry_github_oauth_401: false,
            error_for_status: self.error_for_status,
            allow_range_not_satisfiable: self.allow_range_not_satisfiable,
            retry_state: self.retry_state.clone(),
        }
    }
}

const PARTIAL_DOWNLOAD_STATE_VERSION: u8 = 3;
const PARTIAL_DOWNLOAD_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
enum DownloadValidator {
    Etag(String),
    LastModified {
        value: String,
        response_date: String,
    },
}

impl DownloadValidator {
    fn as_header_value(&self) -> &str {
        match self {
            Self::Etag(value) | Self::LastModified { value, .. } => value,
        }
    }

    fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Etag(a), Self::Etag(b)) => a == b,
            (Self::LastModified { value: a, .. }, Self::LastModified { value: b, .. }) => a == b,
            _ => false,
        }
    }

    fn is_valid(&self) -> bool {
        match self {
            Self::Etag(value) => !value.is_empty() && !value.starts_with("W/"),
            Self::LastModified {
                value,
                response_date,
            } => is_strong_last_modified(value, response_date),
        }
    }
}

#[derive(Debug)]
struct DownloadSizeMismatch {
    expected: u64,
    actual: u64,
}

impl std::fmt::Display for DownloadSizeMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "downloaded file size mismatch: expected {} bytes, got {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for DownloadSizeMismatch {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PartialDownloadState {
    version: u8,
    request_hash: String,
    validator: DownloadValidator,
    total_size: Option<u64>,
    effective_filename: Option<String>,
}

/// Safe response metadata exposed to download callers.
///
/// This intentionally contains only the final URL's decoded path basename.
/// Query strings, fragments, credentials, and the complete URL are never
/// persisted in resumable download state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DownloadFileMetadata {
    pub(crate) effective_filename: Option<String>,
}

fn download_filename_hint(url: &Url) -> Option<String> {
    let segment = url.path_segments()?.next_back()?;
    let filename = urlencoding::decode(segment).ok()?.into_owned();
    if filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename.ends_with([' ', '.'])
        || filename.chars().any(|c| {
            c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
    {
        None
    } else {
        Some(filename)
    }
}

#[derive(Debug, Clone)]
struct PartialDownload {
    path: PathBuf,
    state_path: PathBuf,
    request_hash: String,
}

impl PartialDownload {
    fn new(destination: &Path, request_hash: String) -> Result<Self> {
        let parent = destination.parent().ok_or_else(|| {
            eyre!(
                "download destination has no parent: {}",
                destination.display()
            )
        })?;
        let filename = destination
            .file_name()
            .ok_or_else(|| {
                eyre!(
                    "download destination has no filename: {}",
                    destination.display()
                )
            })?
            .to_string_lossy();
        let partial_name = format!(".{filename}.mise-part");
        Ok(Self {
            path: parent.join(&partial_name),
            state_path: parent.join(format!("{partial_name}.json")),
            request_hash,
        })
    }

    fn load(&self) -> Result<Option<(PartialDownloadState, u64)>> {
        let state_bytes = match std::fs::read(&self.state_path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.remove_partial_if_exists()?;
                return Ok(None);
            }
            Err(err) => return Err(err.into()),
        };
        let state: PartialDownloadState = match serde_json::from_slice(&state_bytes) {
            Ok(state) => state,
            Err(_) => {
                self.clear()?;
                return Ok(None);
            }
        };
        let partial_size = match std::fs::metadata(&self.path) {
            Ok(metadata) => metadata.len(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.remove_state_if_exists()?;
                return Ok(None);
            }
            Err(err) => return Err(err.into()),
        };
        if state.version != PARTIAL_DOWNLOAD_STATE_VERSION
            || state.request_hash != self.request_hash
            || !state.validator.is_valid()
            || partial_size == 0
            || state.total_size.is_some_and(|total| partial_size > total)
        {
            self.clear()?;
            return Ok(None);
        }
        Ok(Some((state, partial_size)))
    }

    fn write_state(&self, state: &PartialDownloadState) -> Result<()> {
        let parent = self.state_path.parent().unwrap();
        let mut temp = tempfile::NamedTempFile::with_prefix_in(".mise-download-state.", parent)?;
        serde_json::to_writer(&mut temp, state)?;
        temp.as_file_mut().sync_all()?;
        temp.persist(&self.state_path).map_err(|err| err.error)?;
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        self.remove_partial_if_exists()?;
        self.remove_state_if_exists()?;
        Ok(())
    }

    fn remove_partial_if_exists(&self) -> Result<()> {
        remove_file_if_exists(&self.path)
    }

    fn remove_state_if_exists(&self) -> Result<()> {
        remove_file_if_exists(&self.state_path)
    }

    fn persist(&self, destination: &Path) -> Result<()> {
        let temp_path = tempfile::TempPath::try_from_path(&self.path)?;
        if let Err(err) = temp_path.persist(destination) {
            let error = err.error;
            let _ = err.path.keep();
            return Err(error.into());
        }
        self.remove_state_if_exists()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedContentRange {
    Bytes { start: u64, end: u64, total: u64 },
    Unsatisfied { total: u64 },
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Removes completed download artifacts while retaining partial download pairs
/// for validation by a later invocation. Explicit backend purges still remove
/// the entire downloads directory.
pub(crate) fn cleanup_download_dir(path: &Path) -> Result<()> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let pair_path = name
            .strip_suffix(".mise-part.json")
            .map(|stem| path.join(format!("{stem}.mise-part")))
            .or_else(|| {
                name.strip_suffix(".mise-part")
                    .map(|_| path.join(format!("{name}.json")))
            });
        if name.starts_with('.')
            && pair_path.is_some_and(|pair_path| {
                pair_path.is_file()
                    && partial_file_is_recent(&entry.path())
                    && partial_file_is_recent(&pair_path)
            })
        {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            file::remove_all(&path)?;
        } else {
            remove_file_if_exists(&path)?;
        }
    }
    if std::fs::read_dir(path)?.next().is_none() {
        std::fs::remove_dir(path)?;
    }
    Ok(())
}

fn partial_file_is_recent(path: &Path) -> bool {
    let Ok(modified) = std::fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .map_or(true, |age| age <= PARTIAL_DOWNLOAD_MAX_AGE)
}

fn update_download_hash(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn download_request_hash(url: &Url, headers: &HeaderMap) -> String {
    let mut hasher = blake3::Hasher::new();
    update_download_hash(&mut hasher, url.as_str().as_bytes());

    let mut header_values = headers
        .keys()
        .flat_map(|name| {
            headers
                .get_all(name)
                .iter()
                .map(move |value| (name.as_str().as_bytes(), value.as_bytes()))
        })
        .collect::<Vec<_>>();
    header_values.sort_unstable();
    for (name, value) in header_values {
        update_download_hash(&mut hasher, name);
        update_download_hash(&mut hasher, value);
    }

    if let Some(replacements) = &Settings::get().url_replacements {
        for (pattern, replacement) in replacements {
            update_download_hash(&mut hasher, pattern.as_bytes());
            update_download_hash(&mut hasher, replacement.as_bytes());
        }
    }
    hasher.finalize().to_hex().to_string()
}

fn response_validator(headers: &HeaderMap) -> Option<DownloadValidator> {
    headers
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && !value.starts_with("W/"))
        .map(|value| DownloadValidator::Etag(value.to_string()))
        .or_else(|| {
            let value = headers
                .get(LAST_MODIFIED)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())?;
            let response_date = headers.get(DATE)?.to_str().ok()?;
            is_strong_last_modified(value, response_date).then(|| DownloadValidator::LastModified {
                value: value.to_string(),
                response_date: response_date.to_string(),
            })
        })
}

fn is_strong_last_modified(value: &str, response_date: &str) -> bool {
    let Ok(last_modified) = chrono::DateTime::parse_from_rfc2822(value) else {
        return false;
    };
    let Ok(response_date) = chrono::DateTime::parse_from_rfc2822(response_date) else {
        return false;
    };
    response_date.signed_duration_since(last_modified) >= chrono::Duration::seconds(60)
}

fn parse_content_range(value: &str) -> Option<ParsedContentRange> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let total = total.parse::<u64>().ok()?;
    if range == "*" {
        return Some(ParsedContentRange::Unsatisfied { total });
    }
    let (start, end) = range.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    let end = end.parse::<u64>().ok()?;
    if start > end || end >= total {
        return None;
    }
    Some(ParsedContentRange::Bytes { start, end, total })
}

#[derive(Debug)]
pub struct Client {
    reqwest: Result<reqwest::Client, String>,
    timeout: Duration,
    kind: ClientKind,
}

#[derive(Debug, Clone, Copy)]
enum ClientKind {
    Http,
    Fetch,
}

impl Client {
    #[cfg(test)]
    fn new(timeout: Duration, kind: ClientKind) -> Result<Self> {
        Ok(Self {
            reqwest: Ok(Self::build(timeout)?),
            timeout,
            kind,
        })
    }

    fn new_shared(timeout: Duration, kind: ClientKind) -> Self {
        Self {
            reqwest: Self::build(timeout).map_err(|err| format!("{err:#}")),
            timeout,
            kind,
        }
    }

    fn build(timeout: Duration) -> Result<reqwest::Client> {
        Ok(Self::_new()
            .read_timeout(timeout)
            .connect_timeout(timeout)
            .build()?)
    }

    #[cfg(test)]
    pub(crate) fn with_init_error(error: impl Into<String>) -> Self {
        Self {
            reqwest: Err(error.into()),
            timeout: Duration::from_secs(1),
            kind: ClientKind::Http,
        }
    }

    /// Underlying reqwest client. Use sparingly — most callers should reach for
    /// the higher-level `get_*`/`json_*`/`post_json_*` helpers instead. This
    /// exists for callers that need request shapes those helpers don't cover
    /// (e.g. form-encoded POST in the GitHub OAuth flow) but still want the
    /// shared timeouts, gzip, and user-agent.
    pub fn reqwest(&self) -> Result<&reqwest::Client> {
        self.reqwest
            .as_ref()
            .map_err(|err| eyre!("Could not initialize the HTTP client: {err}"))
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
            ClientKind::Fetch if Settings::get().bound_remote_version_lookups() => {
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
            .reqwest()?
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
        self.download_file_with_metadata(url, path, pr)
            .await
            .map(|_| ())
    }

    pub(crate) async fn download_file_with_metadata<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        pr: Option<&dyn SingleReport>,
    ) -> Result<DownloadFileMetadata> {
        let url = url.into_url()?;
        let headers = host_auth_headers(&url)?;
        self.download_file_with_headers_metadata(url, path, &headers, pr)
            .await
    }

    pub async fn download_file_with_headers<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        headers: &HeaderMap,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        self.download_file_with_headers_metadata(url, path, headers, pr)
            .await
            .map(|_| ())
    }

    async fn download_file_with_headers_metadata<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        headers: &HeaderMap,
        pr: Option<&dyn SingleReport>,
    ) -> Result<DownloadFileMetadata> {
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
    ) -> Result<DownloadFileMetadata> {
        ensure!(!Settings::get().offline(), "offline mode is enabled");
        let url = url.into_url()?;
        debug!("GET Downloading {} to {}", &url, display_path(path));
        let parent = path.parent().unwrap();
        file::create_dir_all(parent)?;
        let partial = PartialDownload::new(path, download_request_hash(&url, headers))?;
        // Backends may already hold a lock for the destination while they
        // download it (for example rustup-init). Lock the downloader-owned
        // partial path instead so concurrent transfers are serialized without
        // recursively acquiring the caller's destination lock.
        let lock_path = partial.path.clone();
        let _download_lock =
            tokio::task::spawn_blocking(move || crate::lock_file::LockFile::new(&lock_path).lock())
                .await??;
        let attempt = Arc::new(AtomicUsize::new(0));
        let bytes_received = Arc::new(AtomicU64::new(0));

        // Retry the whole transfer, resuming a validated partial response when
        // possible. send_once_with_https_fallback_allow_416 (not
        // send_with_https_fallback) is used inside to avoid retry-on-retry.
        let download = retry_async("GET", &url, || {
            let attempt = attempt.clone();
            let bytes_received = bytes_received.clone();
            let request_url = url.clone();
            let partial = partial.clone();
            async move {
                attempt.fetch_add(1, Ordering::Relaxed);
                bytes_received.store(0, Ordering::Relaxed);
                self.download_file_attempt(request_url, headers, &partial, pr, &bytes_received)
                    .await
            }
        });

        let metadata = match tokio::time::timeout(total_timeout, download).await {
            Ok(result) => result?,
            Err(_) => {
                // A timeout cancels the transfer future before its normal cleanup
                // runs. Loading the sidecar removes an unvalidated partial while
                // preserving a resumable one.
                if let Err(err) = partial.load() {
                    debug!("failed to validate partial download after timeout: {err:#}");
                }
                bail!(
                    "HTTP download timed out after {} for {} (attempt {}, {} bytes received; change with `http_download_timeout` or env `MISE_HTTP_DOWNLOAD_TIMEOUT`)",
                    format_duration(total_timeout),
                    url,
                    attempt.load(Ordering::Relaxed),
                    bytes_received.load(Ordering::Relaxed),
                )
            }
        };

        // Complete the atomic rename after the cancellable transfer budget. A
        // blocking task cannot be cancelled once it starts, so keeping it out
        // of `timeout` prevents us from returning an error while it can still
        // install the destination in the background.
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || partial.persist(&path)).await??;
        Ok(metadata)
    }

    async fn download_file_attempt(
        &self,
        url: Url,
        headers: &HeaderMap,
        partial: &PartialDownload,
        pr: Option<&dyn SingleReport>,
        bytes_received: &AtomicU64,
    ) -> Result<DownloadFileMetadata> {
        let mut restarted_without_resume = false;
        loop {
            let resume = if restarted_without_resume {
                None
            } else {
                partial.load()?
            };
            if let Some((state, partial_size)) = &resume
                && state.total_size == Some(*partial_size)
            {
                if let Some(pr) = pr {
                    pr.set_length(*partial_size);
                    pr.set_position(*partial_size);
                }
                return Ok(DownloadFileMetadata {
                    effective_filename: state.effective_filename.clone(),
                });
            }

            let offset = resume.as_ref().map(|(_, size)| *size).unwrap_or(0);
            let mut request_headers = headers.clone();
            request_headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
            if let Some((state, _)) = &resume {
                request_headers.insert(RANGE, HeaderValue::from_str(&format!("bytes={offset}-"))?);
                request_headers.insert(
                    IF_RANGE,
                    HeaderValue::from_str(state.validator.as_header_value())?,
                );
            }

            let mut resp = self
                .send_once_with_https_fallback_allow_416(
                    Method::GET,
                    url.clone(),
                    &request_headers,
                    "GET",
                )
                .await?;
            let response_filename = download_filename_hint(resp.url());

            if resp.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                if let Some(ParsedContentRange::Unsatisfied { total }) = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_content_range)
                {
                    debug!("range request at offset {offset} was unsatisfied for {total} bytes");
                }
                partial.clear()?;
                if offset > 0 && !restarted_without_resume {
                    restarted_without_resume = true;
                    continue;
                }
                resp.error_for_status_ref()?;
            }

            let (write_offset, total_size, validator, resumable, effective_filename) =
                if resp.status() == StatusCode::PARTIAL_CONTENT {
                    let Some((state, _)) = resume else {
                        partial.clear()?;
                        if !restarted_without_resume {
                            restarted_without_resume = true;
                            continue;
                        }
                        bail!("server returned partial content without a resumable request");
                    };
                    let content_range = resp
                        .headers()
                        .get(CONTENT_RANGE)
                        .and_then(|value| value.to_str().ok())
                        .and_then(parse_content_range);
                    let Some(ParsedContentRange::Bytes { start, end, total }) = content_range
                    else {
                        partial.clear()?;
                        if restarted_without_resume {
                            bail!("server returned an invalid Content-Range response");
                        }
                        restarted_without_resume = true;
                        continue;
                    };
                    let validator = response_validator(resp.headers());
                    let response_length_matches = resp
                        .content_length()
                        .is_none_or(|length| length == end - start + 1);
                    if start != offset
                        || end + 1 != total
                        || !response_length_matches
                        || state.total_size.is_some_and(|expected| expected != total)
                        || validator
                            .as_ref()
                            .is_some_and(|value| !value.matches(&state.validator))
                    {
                        partial.clear()?;
                        if restarted_without_resume {
                            bail!("server returned inconsistent partial content");
                        }
                        restarted_without_resume = true;
                        continue;
                    }
                    // Keep the filename associated with the bytes already on disk.
                    // A redirect target may change between requests even when the
                    // server accepts the validator and Range header. Replacing the
                    // stored hint could make the completed bytes use a different
                    // archive format than the response that started the partial.
                    let effective_filename = state.effective_filename;
                    let validator = validator.unwrap_or(state.validator);
                    (
                        offset,
                        Some(total),
                        Some(validator),
                        true,
                        effective_filename,
                    )
                } else {
                    partial.clear()?;
                    let total_size = resp.content_length();
                    let validator = response_validator(resp.headers());
                    let resumable = total_size.is_some() && validator.is_some();
                    (
                        0,
                        total_size,
                        validator.filter(|_| resumable),
                        resumable,
                        response_filename,
                    )
                };

            let state = validator.map(|validator| PartialDownloadState {
                version: PARTIAL_DOWNLOAD_STATE_VERSION,
                request_hash: partial.request_hash.clone(),
                validator,
                total_size,
                effective_filename: effective_filename.clone(),
            });
            if let Some(state) = &state {
                partial.write_state(state)?;
            } else {
                partial.remove_state_if_exists()?;
            }

            if let Some(pr) = pr {
                if let Some(total_size) = total_size {
                    pr.set_length(total_size);
                }
                pr.set_position(write_offset);
            }
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(write_offset > 0)
                .truncate(write_offset == 0)
                .open(&partial.path)
                .await?;
            let transfer = async {
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
                file.sync_all().await?;
                Ok::<_, Report>(())
            }
            .await;
            if transfer.is_err()
                && !resumable
                && let Err(err) = partial.clear()
            {
                debug!("failed to remove unvalidated partial download: {err:#}");
            }
            transfer?;

            if let Some(total_size) = total_size {
                let actual_size = tokio::fs::metadata(&partial.path).await?.len();
                if actual_size != total_size {
                    return Err(DownloadSizeMismatch {
                        expected: total_size,
                        actual: actual_size,
                    }
                    .into());
                }
            }
            return Ok(DownloadFileMetadata { effective_filename });
        }
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
    async fn send_once_with_https_fallback_allow_416(
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
            SendOnceOptions::new(None, true).allow_range_not_satisfiable(),
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
        let mut req = self.reqwest()?.request(method.clone(), url.clone());
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
                        crate::github::remember_token_source(
                            host,
                            &token,
                            crate::github::TokenSource::GithubOauth,
                        );
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
        if options.error_for_status && is_github_unauthorized(&url, &resp) {
            // A static invalid/expired token (env var, gh CLI, ...) produces a 401
            // that the OAuth-refresh path above cannot recover. Surface a clear
            // error naming the token source instead of a bare status error. See #7218.
            let status_error = resp
                .error_for_status_ref()
                .expect_err("401 response should be an error");
            let used_github_token = final_headers.contains_key(AUTHORIZATION);
            // Use the source captured when this exact token was added to the request.
            // A netrc/caller-provided header must not be blamed on an unrelated token.
            let token_source = final_headers
                .get(AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .zip(original_url.host_str())
                .and_then(|(token, host)| crate::github::token_source_for_token(host, token));
            let body = read_bounded_error_body(resp, self.timeout).await;
            return Err(github_unauthorized_report(
                status_error,
                used_github_token,
                token_source.as_ref(),
                &body,
            ));
        }
        if options.error_for_status && is_github_forbidden(&url, &resp) {
            let status = resp.status();
            let status_error = resp
                .error_for_status_ref()
                .expect_err("403 response should be an error");
            let used_github_token = final_headers.contains_key(AUTHORIZATION);
            let rate_limit = github_rate_limit_summary(&resp);
            let body = read_bounded_error_body(resp, self.timeout).await;
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
        if options.error_for_status
            && !(options.allow_range_not_satisfiable
                && resp.status() == StatusCode::RANGE_NOT_SATISFIABLE)
        {
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

fn is_github_unauthorized(url: &Url, resp: &Response) -> bool {
    resp.status() == StatusCode::UNAUTHORIZED && crate::github::is_github_api_url(url)
}

/// Maximum body bytes buffered when building a GitHub error report, so an
/// oversized or slow-trickling error response can't exhaust memory. The overall
/// request timeout bounds the time; this bounds the memory.
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Reads at most [`MAX_ERROR_BODY_BYTES`] of the response body for use in an
/// error message, streaming chunk-by-chunk instead of buffering the whole body,
/// and abandoning the read after `deadline` so a slowly-trickling response can't
/// block indefinitely (the `Http` client has no overall request timeout, only an
/// idle `read_timeout`). On timeout the partial body is dropped and "" returned.
async fn read_bounded_error_body(resp: Response, deadline: Duration) -> String {
    let read = async move {
        let mut resp = resp;
        let mut bytes = Vec::new();
        while let Ok(Some(chunk)) = resp.chunk().await {
            let remaining = MAX_ERROR_BODY_BYTES.saturating_sub(bytes.len());
            if remaining == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
        String::from_utf8_lossy(&bytes).to_string()
    };
    tokio::time::timeout(deadline, read)
        .await
        .unwrap_or_default()
}

fn github_unauthorized_report(
    status_error: reqwest::Error,
    used_github_token: bool,
    token_source: Option<&crate::github::TokenSource>,
    body: &str,
) -> Report {
    // Only report a token when one was actually sent: the process may have a
    // GitHub token env var set that wasn't applied to this request.
    let auth = if !used_github_token {
        "no".to_string()
    } else {
        token_source
            .map(|source| format!("yes (token from {source})"))
            .unwrap_or_else(|| "yes".to_string())
    };
    let body = format_response_body(body);
    let hint = if used_github_token {
        let source = match token_source {
            Some(crate::github::TokenSource::EnvVar(var)) => format!("token in `{var}`"),
            Some(source) => format!("token from {source}"),
            None => "configured GitHub token".to_string(),
        };
        format!(
            "\nhint: the {source} was rejected by GitHub (401 Unauthorized). Verify it is a \
             valid, non-expired token for this host with the required scopes — see \
             https://mise.jdx.dev/dev-tools/github-tokens.html"
        )
    } else {
        String::new()
    };
    eyre!("{status_error}\ngithub auth: {auth}\ngithub response: {body}{hint}")
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
pub(crate) fn netrc_headers(url: &Url) -> HeaderMap {
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

/// Resolve the `rel="next"` target of a `Link` header against the URL it came from.
///
/// Forge APIs are inconsistent about this: an absolute URL is the common case, but a
/// root-relative or relative target is legal and appears from instances behind a proxy.
/// Shared by [`crate::github`] and [`crate::gitlab`] so their pagination loops resolve
/// the next page the same way — the two drifted apart once already (#6318).
pub(crate) fn resolve_pagination_url(current: &str, next: &str) -> Result<String> {
    if next.starts_with("http://") || next.starts_with("https://") {
        return Ok(next.to_string());
    }
    let base = url::Url::parse(current)
        .wrap_err_with(|| format!("invalid pagination base URL: {current}"))?;
    if next.starts_with('/') {
        return Ok(format!("{}{next}", base.origin().ascii_serialization()));
    }
    base.join(next)
        .map(|u| u.to_string())
        .wrap_err_with(|| format!("invalid pagination URL: {next}"))
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
        if e.downcast_ref::<DownloadSizeMismatch>().is_some() {
            return true;
        }
        let Some(reqwest_err) = e.downcast_ref::<reqwest::Error>() else {
            return false;
        };
        // Network-layer failures: connect refused, timeout, mid-stream body drop.
        if reqwest_err.is_timeout() || reqwest_err.is_connect() || reqwest_err.is_body() {
            return true;
        }
        // Send failures that never produced a response: the connection was
        // established but the request did not complete, so no application logic
        // ran and a retry is safe. This covers HTTP/2 stream errors such as
        // REFUSED_STREAM (which RFC 9113 §8.7 defines as "the request was not
        // processed", i.e. safely retryable) and connections closed before the
        // response started. These are not is_connect(), because connecting
        // succeeded, and they carry no status, so without this they fall through
        // and fail on the first attempt regardless of `http_retries`.
        if reqwest_err.is_request() && reqwest_err.status().is_none() {
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
        // `SettingsGuard` holds the lock and calls `Settings::reset(None)` in `Drop`, which runs
        // while unwinding. Resetting after `test_fn` instead would leave the replacements behind
        // for the next test whenever this one panics -- previously the lock's poison flag hid
        // that by failing every later test outright.
        let _guard = SettingsGuard {
            _lock: crate::test::lock_ignoring_poison(&TEST_SETTINGS_LOCK),
        };

        // Create settings with custom URL replacements
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.url_replacements = Some(replacements);

        // Set settings for this test
        crate::config::Settings::reset(Some(settings));

        test_fn()
    }

    #[test]
    fn test_resolve_pagination_url() {
        let base = "https://api.github.com/repos/jdx/aube/releases?per_page=100";
        assert_eq!(
            resolve_pagination_url(base, "/repos/jdx/aube/releases?page=2").unwrap(),
            "https://api.github.com/repos/jdx/aube/releases?page=2"
        );
        assert_eq!(
            resolve_pagination_url(
                base,
                "https://api.github.com/repos/jdx/aube/releases?page=2"
            )
            .unwrap(),
            "https://api.github.com/repos/jdx/aube/releases?page=2"
        );
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
    async fn test_client_initialization_error_is_returned_not_panicked() {
        let client = Client::with_init_error("builder error: OpenSSL error");

        let err = client.get_text("https://example.com").await.unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("Could not initialize the HTTP client"));
        assert!(message.contains("builder error: OpenSSL error"));
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
    async fn test_download_metadata_uses_redirected_filename() {
        let mut server = mockito::Server::new_async().await;
        let location = format!("{}/releases/tool.tar.gz", server.url());
        let redirect = server
            .mock("GET", "/download")
            .with_status(302)
            .with_header("location", &location)
            .expect(1)
            .create_async()
            .await;
        let artifact = server
            .mock("GET", "/releases/tool.tar.gz")
            .with_status(200)
            .with_header("content-length", "2")
            .with_body("OK")
            .expect(1)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("download");
        let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();

        let metadata = client
            .download_file_with_metadata(format!("{}/download", server.url()), &destination, None)
            .await
            .unwrap();

        assert_eq!(metadata.effective_filename.as_deref(), Some("tool.tar.gz"));
        assert_eq!(std::fs::read(&destination).unwrap(), b"OK");
        redirect.assert();
        artifact.assert();
    }

    #[test]
    fn test_download_filename_hint_excludes_unsafe_or_private_url_parts() {
        let url: Url =
            "https://user:pass@example.com/releases/tool%20name.tar.gz?token=secret#fragment"
                .parse()
                .unwrap();
        assert_eq!(
            download_filename_hint(&url).as_deref(),
            Some("tool name.tar.gz")
        );

        let unsafe_url: Url = "https://example.com/releases/%2E%2E%2Fsecret.tar.gz"
            .parse()
            .unwrap();
        assert_eq!(download_filename_hint(&unsafe_url), None);

        for encoded_name in [
            "tool%3Aname.tar.gz",
            "tool%2Aname.tar.gz",
            "tool%00name.tar.gz",
        ] {
            let url: Url = format!("https://example.com/releases/{encoded_name}")
                .parse()
                .unwrap();
            assert_eq!(download_filename_hint(&url), None, "{encoded_name}");
        }
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
        let lock = crate::test::lock_ignoring_poison(&TEST_SETTINGS_LOCK);
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.http_retries = Some(retries);
        crate::config::Settings::reset(Some(settings));
        SettingsGuard { _lock: lock }
    }
    fn set_test_prefer_offline(http_retries: i64) -> SettingsGuard {
        let lock = crate::test::lock_ignoring_poison(&TEST_SETTINGS_LOCK);
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.prefer_offline = Some(true);
        settings.http_retries = Some(http_retries);
        crate::config::Settings::reset(Some(settings));
        SettingsGuard { _lock: lock }
    }
    fn set_test_offline() -> SettingsGuard {
        let lock = crate::test::lock_ignoring_poison(&TEST_SETTINGS_LOCK);
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.offline = Some(true);
        crate::config::Settings::reset(Some(settings));
        SettingsGuard { _lock: lock }
    }

    struct AtomicBoolGuard {
        value: &'static std::sync::atomic::AtomicBool,
        previous: bool,
    }
    impl AtomicBoolGuard {
        fn set(value: &'static std::sync::atomic::AtomicBool, enabled: bool) -> Self {
            let previous = value.swap(enabled, Ordering::SeqCst);
            Self { value, previous }
        }
    }
    impl Drop for AtomicBoolGuard {
        fn drop(&mut self) {
            self.value.store(self.previous, Ordering::SeqCst);
        }
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
        let settings_lock = crate::test::lock_ignoring_poison(&TEST_SETTINGS_LOCK);
        let github_env_lock = crate::test::lock_ignoring_poison(&crate::github::TEST_ENV_LOCK);
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
    fn truncated_download_response() -> &'static str {
        concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Length: 10\r\n",
            "ETag: \"artifact-v1\"\r\n",
            "Connection: close\r\n",
            "\r\n",
            "hello"
        )
    }
    fn redirect_to_tar_gz_response() -> &'static str {
        "HTTP/1.1 302 Found\r\nLocation: /tool.tar.gz\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    }
    fn redirect_to_zip_response() -> &'static str {
        "HTTP/1.1 302 Found\r\nLocation: /tool.zip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    }
    fn resumed_download_response() -> &'static str {
        concat!(
            "HTTP/1.1 206 Partial Content\r\n",
            "Content-Length: 5\r\n",
            "Content-Range: bytes 5-9/10\r\n",
            "ETag: \"artifact-v1\"\r\n",
            "Connection: close\r\n",
            "\r\n",
            "world"
        )
    }
    fn truncated_last_modified_download_response() -> &'static str {
        concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Length: 10\r\n",
            "Last-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\n",
            "Date: Wed, 21 Oct 2015 07:29:00 GMT\r\n",
            "Connection: close\r\n",
            "\r\n",
            "hello"
        )
    }
    fn resumed_last_modified_download_response() -> &'static str {
        concat!(
            "HTTP/1.1 206 Partial Content\r\n",
            "Content-Length: 5\r\n",
            "Content-Range: bytes 5-9/10\r\n",
            "Last-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\n",
            "Date: Wed, 21 Oct 2015 07:30:00 GMT\r\n",
            "Connection: close\r\n",
            "\r\n",
            "world"
        )
    }
    fn invalid_resumed_download_response() -> &'static str {
        concat!(
            "HTTP/1.1 206 Partial Content\r\n",
            "Content-Length: 6\r\n",
            "Content-Range: bytes 4-9/10\r\n",
            "ETag: \"artifact-v1\"\r\n",
            "Connection: close\r\n",
            "\r\n",
            "oworld"
        )
    }
    fn changed_validator_download_response() -> &'static str {
        concat!(
            "HTTP/1.1 206 Partial Content\r\n",
            "Content-Length: 5\r\n",
            "Content-Range: bytes 5-9/10\r\n",
            "ETag: \"artifact-v2\"\r\n",
            "Connection: close\r\n",
            "\r\n",
            "world"
        )
    }
    fn full_download_response() -> &'static str {
        concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Length: 10\r\n",
            "ETag: \"artifact-v1\"\r\n",
            "Connection: close\r\n",
            "\r\n",
            "helloworld"
        )
    }
    fn truncated_download_without_validator_response() -> &'static str {
        concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Length: 10\r\n",
            "Connection: close\r\n",
            "\r\n",
            "hello"
        )
    }
    fn truncated_encoded_download_response() -> &'static str {
        concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Length: 10\r\n",
            "Content-Encoding: gzip\r\n",
            "ETag: \"artifact-v1\"\r\n",
            "Connection: close\r\n",
            "\r\n",
            "hello"
        )
    }
    fn range_not_satisfiable_response() -> &'static str {
        concat!(
            "HTTP/1.1 416 Range Not Satisfiable\r\n",
            "Content-Range: bytes */4\r\n",
            "Content-Length: 0\r\n",
            "Connection: close\r\n",
            "\r\n"
        )
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
    fn seed_github_oauth_cache(cache_path: &Path) {
        let settings = crate::config::Settings::get();
        let cache_key = crate::github::oauth::test_support::cache_key(
            "127.0.0.1",
            "Iv1.mock",
            settings.github.oauth_scopes.trim(),
        );
        std::fs::write(
            cache_path,
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
        seed_github_oauth_cache(&cache_path);

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

    #[tokio::test(flavor = "multi_thread")]
    async fn test_github_oauth_401_reports_refreshed_token_source() {
        let (port, count, _requests) = spawn_recording_server(vec![
            unauthorized_response(),
            github_oauth_token_response(),
            unauthorized_response(),
        ])
        .await;
        let server_url = format!("http://127.0.0.1:{port}");
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("github-oauth-tokens.toml");
        let _guard = set_test_github_oauth(&server_url, cache_path.clone());
        seed_github_oauth_cache(&cache_path);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer ghu-stale"));
        let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
        let err = client
            .get_text_request(format!("{server_url}/api/v3/repos/owner/repo/releases"))
            .headers(&headers)
            .send()
            .await
            .unwrap_err();
        let msg = format!("{err:?}");

        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert!(
            msg.contains("github auth: yes (token from GitHub OAuth)"),
            "{msg}"
        );
        assert!(
            msg.contains("token from GitHub OAuth was rejected by GitHub"),
            "{msg}"
        );
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

    #[tokio::test(flavor = "current_thread")]
    async fn test_github_unauthorized_report_names_token_source() {
        // env var known → the message names it and includes the token-guide hint.
        let (port, _count) = spawn_canned_server(vec![unauthorized_response()]).await;
        let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");
        let resp = reqwest::Client::new().get(url).send().await.unwrap();
        let status_error = resp
            .error_for_status_ref()
            .expect_err("401 response should be an error");
        let body = resp.text().await.unwrap();
        let err = github_unauthorized_report(
            status_error,
            true,
            Some(&crate::github::TokenSource::EnvVar("GITHUB_TOKEN")),
            &body,
        );
        let msg = format!("{err:?}");

        assert!(
            msg.contains("github auth: yes (token from GITHUB_TOKEN)"),
            "{msg}"
        );
        assert!(msg.contains("Bad credentials"), "{msg}");
        assert!(
            msg.contains("token in `GITHUB_TOKEN` was rejected by GitHub (401 Unauthorized)"),
            "{msg}"
        );
        assert!(msg.contains("github-tokens.html"), "{msg}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_github_unauthorized_report_names_non_env_token_sources() {
        let sources = [
            (crate::github::TokenSource::TokensFile, "github_tokens.toml"),
            (crate::github::TokenSource::GhCli, "gh CLI (hosts.yml)"),
            (
                crate::github::TokenSource::CredentialCommand,
                "credential_command",
            ),
            (crate::github::TokenSource::GithubOauth, "GitHub OAuth"),
            (
                crate::github::TokenSource::GitCredential,
                "git credential fill",
            ),
        ];
        let (port, _count) =
            spawn_canned_server(vec![unauthorized_response(); sources.len()]).await;
        let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");

        for (source, label) in sources {
            let resp = reqwest::Client::new().get(&url).send().await.unwrap();
            let status_error = resp.error_for_status_ref().unwrap_err();
            let body = resp.text().await.unwrap();
            let msg = format!(
                "{:?}",
                github_unauthorized_report(status_error, true, Some(&source), &body)
            );

            assert!(
                msg.contains(&format!("github auth: yes (token from {label})")),
                "{msg}"
            );
            assert!(
                msg.contains(&format!("token from {label} was rejected by GitHub")),
                "{msg}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_github_unauthorized_report_without_known_source() {
        // Token used but source unknown → generic auth "yes" and generic hint;
        // no token → auth "no" and no hint.
        let (port, _count) =
            spawn_canned_server(vec![unauthorized_response(), unauthorized_response()]).await;
        let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");

        let resp = reqwest::Client::new().get(&url).send().await.unwrap();
        let status_error = resp.error_for_status_ref().unwrap_err();
        let body = resp.text().await.unwrap();
        let used_msg = format!(
            "{:?}",
            github_unauthorized_report(status_error, true, None, &body)
        );
        assert!(used_msg.contains("github auth: yes"), "{used_msg}");
        assert!(!used_msg.contains("token from"), "{used_msg}");
        assert!(used_msg.contains("configured GitHub token"), "{used_msg}");

        let resp = reqwest::Client::new().get(&url).send().await.unwrap();
        let status_error = resp.error_for_status_ref().unwrap_err();
        let body = resp.text().await.unwrap();
        let anon_msg = format!(
            "{:?}",
            github_unauthorized_report(status_error, false, None, &body)
        );
        assert!(anon_msg.contains("github auth: no"), "{anon_msg}");
        assert!(!anon_msg.contains("hint:"), "{anon_msg}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_github_unauthorized_report_ignores_source_when_no_auth_sent() {
        // A GitHub token env var may be present in the process even when this
        // request sent no Authorization header; it must not be reported as used.
        let (port, _count) = spawn_canned_server(vec![unauthorized_response()]).await;
        let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");
        let resp = reqwest::Client::new().get(url).send().await.unwrap();
        let status_error = resp.error_for_status_ref().unwrap_err();
        let body = resp.text().await.unwrap();
        let msg = format!(
            "{:?}",
            github_unauthorized_report(
                status_error,
                false,
                Some(&crate::github::TokenSource::EnvVar("GITHUB_TOKEN")),
                &body
            )
        );

        assert!(msg.contains("github auth: no"), "{msg}");
        assert!(!msg.contains("token from"), "{msg}");
        assert!(!msg.contains("hint:"), "{msg}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_read_bounded_error_body_caps_large_body() {
        // An oversized error body must be truncated during reading, not buffered
        // whole, so a hostile endpoint can't exhaust memory.
        let big_body = "x".repeat(MAX_ERROR_BODY_BYTES + 4096);
        let raw = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            big_body.len(),
            big_body
        );
        let leaked: &'static str = Box::leak(raw.into_boxed_str());
        let (port, _count) = spawn_canned_server(vec![leaked]).await;
        let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");
        let resp = reqwest::Client::new().get(url).send().await.unwrap();

        let body = read_bounded_error_body(resp, Duration::from_secs(30)).await;
        assert_eq!(body.len(), MAX_ERROR_BODY_BYTES);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_read_bounded_error_body_honors_deadline() {
        // A response body that trickles forever (staying under the byte cap and
        // the idle read_timeout) must still be abandoned at the deadline instead
        // of blocking indefinitely.
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                // No Content-Length + `close` → body is read until EOF, which the
                // server never sends; it just trickles one byte at a time.
                let _ = sock
                    .write_all(b"HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n")
                    .await;
                loop {
                    if sock.write_all(b"x").await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        });
        let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");
        let resp = reqwest::Client::new().get(url).send().await.unwrap();

        let start = tokio::time::Instant::now();
        let body = read_bounded_error_body(resp, Duration::from_millis(150)).await;
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "read must stop at the deadline"
        );
        assert!(
            body.is_empty(),
            "timed-out read yields no body, got {body:?}"
        );
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
    async fn test_retry_rescues_send_failure_with_no_response() {
        // A connection that is accepted and then closed without a response fails
        // with a reqwest "request" error: connecting succeeded, so it is not
        // is_connect(), and no response arrived, so there is no status. HTTP/2
        // REFUSED_STREAM lands in the same class. Before these were classified as
        // transient, such failures exited on the first attempt even with retries
        // enabled.
        let _guard = set_test_http_retries(1);
        let (port, count) = spawn_canned_server(vec!["", ok_response()]).await;
        let url: Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let resp = client.get_async(url).await.unwrap();

        assert!(resp.status().is_success());
        // Two connections: the aborted one, then the retry that succeeded.
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
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

    #[test]
    fn test_remote_fetch_command_keeps_full_budget_under_prefer_offline() {
        // Commands whose job is to enumerate remote versions/tags (`mise lock`,
        // `ls-remote`, ...) must honor the configured timeout and retries even
        // when prefer_offline is set.
        // https://github.com/jdx/mise/discussions/11185
        let client = Client::new(Duration::from_secs(30), ClientKind::Fetch).unwrap();
        let _guard = set_test_prefer_offline(3);
        let _remote_fetch_guard = AtomicBoolGuard::set(&crate::env::REMOTE_FETCH_COMMAND, true);

        assert_eq!(client.request_timeout(), Duration::from_secs(30));
        assert_eq!(
            Settings::get().fetch_remote_versions_timeout(),
            Settings::get().configured_fetch_remote_versions_timeout()
        );
        assert_eq!(Settings::get().http_retries(), 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_reqwest_dns_error_is_not_transient_and_opens_circuit() {
        let _settings_guard = set_test_prefer_offline(3);
        let timeout = Duration::from_secs(3);
        let client = Client {
            reqwest: Ok(Client::_new()
                .no_proxy()
                .read_timeout(timeout)
                .connect_timeout(timeout)
                .build()
                .unwrap()),
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

    #[test]
    fn test_only_download_size_mismatches_are_transient_eof_errors() {
        let mismatch: Report = DownloadSizeMismatch {
            expected: 10,
            actual: 5,
        }
        .into();
        assert!(is_transient(&mismatch));

        let unrelated: Report =
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "local file truncated").into();
        assert!(!is_transient(&unrelated));
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

    #[derive(Debug, Default)]
    struct RecordingReport {
        positions: Mutex<Vec<u64>>,
        lengths: Mutex<Vec<u64>>,
    }

    impl SingleReport for RecordingReport {
        fn set_position(&self, position: u64) {
            self.positions.lock().unwrap().push(position);
        }

        fn set_length(&self, length: u64) {
            self.lengths.lock().unwrap().push(length);
        }
    }

    #[test]
    fn test_parse_content_range() {
        assert_eq!(
            parse_content_range("bytes 5-9/10"),
            Some(ParsedContentRange::Bytes {
                start: 5,
                end: 9,
                total: 10
            })
        );
        assert_eq!(
            parse_content_range("bytes */10"),
            Some(ParsedContentRange::Unsatisfied { total: 10 })
        );
        assert_eq!(parse_content_range("bytes 5-10/10"), None);
        assert_eq!(parse_content_range("items 5-9/10"), None);
    }

    #[test]
    fn test_response_validator_requires_strong_etag_or_last_modified() {
        let mut headers = HeaderMap::new();
        headers.insert(ETAG, HeaderValue::from_static("W/\"weak\""));
        assert_eq!(response_validator(&headers), None);

        headers.insert(
            LAST_MODIFIED,
            HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"),
        );
        assert_eq!(response_validator(&headers), None);

        headers.insert(
            DATE,
            HeaderValue::from_static("Wed, 21 Oct 2015 07:28:59 GMT"),
        );
        assert_eq!(response_validator(&headers), None);

        headers.insert(
            DATE,
            HeaderValue::from_static("Wed, 21 Oct 2015 07:29:00 GMT"),
        );
        assert_eq!(
            response_validator(&headers),
            Some(DownloadValidator::LastModified {
                value: "Wed, 21 Oct 2015 07:28:00 GMT".to_string(),
                response_date: "Wed, 21 Oct 2015 07:29:00 GMT".to_string(),
            })
        );

        headers.insert(ETAG, HeaderValue::from_static("\"strong\""));
        assert_eq!(
            response_validator(&headers),
            Some(DownloadValidator::Etag("\"strong\"".to_string()))
        );
    }

    #[test]
    fn test_cleanup_download_dir_preserves_only_partial_pairs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("artifact.tar.gz"), b"complete").unwrap();
        std::fs::write(dir.path().join(".artifact.tar.gz.mise-part"), b"partial").unwrap();
        std::fs::write(
            dir.path().join(".artifact.tar.gz.mise-part.json"),
            b"metadata",
        )
        .unwrap();
        std::fs::write(dir.path().join(".orphan.mise-part"), b"orphan").unwrap();
        let expired_partial = dir.path().join(".expired.mise-part");
        let expired_state = dir.path().join(".expired.mise-part.json");
        std::fs::write(&expired_partial, b"expired partial").unwrap();
        std::fs::write(&expired_state, b"expired metadata").unwrap();
        let expired_time = filetime::FileTime::from_system_time(
            SystemTime::now() - PARTIAL_DOWNLOAD_MAX_AGE - Duration::from_secs(1),
        );
        filetime::set_file_mtime(&expired_partial, expired_time).unwrap();
        filetime::set_file_mtime(&expired_state, expired_time).unwrap();
        std::fs::create_dir(dir.path().join("extracted")).unwrap();

        cleanup_download_dir(dir.path()).unwrap();

        assert!(!dir.path().join("artifact.tar.gz").exists());
        assert!(!dir.path().join("extracted").exists());
        assert!(dir.path().join(".artifact.tar.gz.mise-part").exists());
        assert!(dir.path().join(".artifact.tar.gz.mise-part.json").exists());
        assert!(!dir.path().join(".orphan.mise-part").exists());
        assert!(!expired_partial.exists());
        assert!(!expired_state.exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_retry_resumes_validated_partial() {
        let _guard = set_test_http_retries(1);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_response(),
            resumed_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let report = RecordingReport::default();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        client
            .download_file_with_headers(&url, &destination, &HeaderMap::new(), Some(&report))
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let requests = requests.lock().unwrap();
        let resumed = requests[1].to_ascii_lowercase();
        assert!(resumed.contains("range: bytes=5-"));
        assert!(resumed.contains("if-range: \"artifact-v1\""));
        assert!(resumed.contains("accept-encoding: identity"));
        assert!(report.positions.lock().unwrap().contains(&5));
        assert!(
            report
                .lengths
                .lock()
                .unwrap()
                .iter()
                .all(|length| *length == 10)
        );

        let partial = PartialDownload::new(
            &destination,
            download_request_hash(&url.parse().unwrap(), &HeaderMap::new()),
        )
        .unwrap();
        assert!(!partial.path.exists());
        assert!(!partial.state_path.exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_does_not_reacquire_destination_lock() {
        let (port, count) = spawn_canned_server(vec![ok_response()]).await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let _destination_lock = crate::lock_file::LockFile::new(&destination)
            .lock()
            .unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        tokio::time::timeout(
            Duration::from_secs(2),
            client.download_file(&url, &destination, None),
        )
        .await
        .expect("download deadlocked on a lock already held by its caller")
        .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"OK");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_retry_resumes_with_last_modified_validator() {
        let _guard = set_test_http_retries(1);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_last_modified_download_response(),
            resumed_last_modified_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        client
            .download_file(&url, &destination, None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let resumed = requests.lock().unwrap()[1].to_ascii_lowercase();
        assert!(resumed.contains("range: bytes=5-"));
        assert!(resumed.contains("if-range: wed, 21 oct 2015 07:28:00 gmt"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_resumes_across_calls_without_storing_secrets() {
        let _guard = set_test_http_retries(0);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_response(),
            resumed_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/private/artifact.tar.gz?token=url-secret");
        let parsed_url: Url = url.parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer header-secret"),
        );
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        std::fs::write(&destination, b"existing destination").unwrap();
        let partial =
            PartialDownload::new(&destination, download_request_hash(&parsed_url, &headers))
                .unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let _err = client
            .download_file_with_headers_metadata(&url, &destination, &headers, None)
            .await
            .unwrap_err();
        assert_eq!(std::fs::read(&partial.path).unwrap(), b"hello");
        let state = std::fs::read_to_string(&partial.state_path).unwrap();
        assert!(!state.contains("url-secret"));
        assert!(!state.contains("header-secret"));
        assert!(state.contains("artifact.tar.gz"));
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"existing destination"
        );

        let metadata = client
            .download_file_with_headers_metadata(&url, &destination, &headers, None)
            .await
            .unwrap();
        assert_eq!(
            metadata.effective_filename.as_deref(),
            Some("artifact.tar.gz")
        );
        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(
            requests.lock().unwrap()[1]
                .to_ascii_lowercase()
                .contains("range: bytes=5-")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_resume_preserves_stored_filename_after_redirect_changes() {
        let _guard = set_test_http_retries(0);
        let (port, count, requests) = spawn_recording_server(vec![
            redirect_to_tar_gz_response(),
            truncated_download_response(),
            redirect_to_zip_response(),
            resumed_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/download");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let _ = client
            .download_file_with_headers_metadata(&url, &destination, &HeaderMap::new(), None)
            .await
            .unwrap_err();

        let metadata = client
            .download_file_with_headers_metadata(&url, &destination, &HeaderMap::new(), None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(metadata.effective_filename.as_deref(), Some("tool.tar.gz"));
        assert_eq!(count.load(Ordering::SeqCst), 4);
        assert!(
            requests.lock().unwrap()[3]
                .to_ascii_lowercase()
                .contains("range: bytes=5-")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_resume_does_not_adopt_filename_when_initial_hint_missing() {
        let _guard = set_test_http_retries(0);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_response(),
            redirect_to_zip_response(),
            resumed_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let _ = client
            .download_file_with_headers_metadata(&url, &destination, &HeaderMap::new(), None)
            .await
            .unwrap_err();

        let metadata = client
            .download_file_with_headers_metadata(&url, &destination, &HeaderMap::new(), None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(metadata.effective_filename, None);
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert!(
            requests.lock().unwrap()[2]
                .to_ascii_lowercase()
                .contains("range: bytes=5-")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_without_validator_restarts_from_zero() {
        let _guard = set_test_http_retries(1);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_without_validator_response(),
            full_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        client
            .download_file(&url, &destination, None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(
            !requests.lock().unwrap()[1]
                .to_ascii_lowercase()
                .contains("range:")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_does_not_resume_automatically_decoded_response() {
        let _guard = set_test_http_retries(0);
        let (port, count) = spawn_canned_server(vec![truncated_encoded_download_response()]).await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let partial = PartialDownload::new(
            &destination,
            download_request_hash(&url.parse().unwrap(), &HeaderMap::new()),
        )
        .unwrap();
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let _err = client
            .download_file(&url, &destination, None)
            .await
            .unwrap_err();

        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(!partial.path.exists());
        assert!(!partial.state_path.exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_restarts_when_server_ignores_range() {
        let _guard = set_test_http_retries(0);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_response(),
            full_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let _err = client
            .download_file(&url, &destination, None)
            .await
            .unwrap_err();
        client
            .download_file(&url, &destination, None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(
            requests.lock().unwrap()[1]
                .to_ascii_lowercase()
                .contains("range: bytes=5-")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_restarts_after_invalid_content_range() {
        let _guard = set_test_http_retries(0);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_response(),
            invalid_resumed_download_response(),
            full_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let _err = client
            .download_file(&url, &destination, None)
            .await
            .unwrap_err();
        client
            .download_file(&url, &destination, None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 3);
        let requests = requests.lock().unwrap();
        assert!(requests[1].to_ascii_lowercase().contains("range: bytes=5-"));
        assert!(!requests[2].to_ascii_lowercase().contains("range:"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_restarts_when_validator_changes() {
        let _guard = set_test_http_retries(0);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_response(),
            changed_validator_download_response(),
            full_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let _err = client
            .download_file(&url, &destination, None)
            .await
            .unwrap_err();
        client
            .download_file(&url, &destination, None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 3);
        let requests = requests.lock().unwrap();
        assert!(requests[1].to_ascii_lowercase().contains("range: bytes=5-"));
        assert!(!requests[2].to_ascii_lowercase().contains("range:"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_discards_partial_when_request_headers_change() {
        let _guard = set_test_http_retries(0);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_response(),
            full_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
        let mut original_headers = HeaderMap::new();
        original_headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer old"));
        let mut changed_headers = HeaderMap::new();
        changed_headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer new"));

        let _err = client
            .download_file_with_headers(&url, &destination, &original_headers, None)
            .await
            .unwrap_err();
        client
            .download_file_with_headers(&url, &destination, &changed_headers, None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(
            !requests.lock().unwrap()[1]
                .to_ascii_lowercase()
                .contains("range:")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_download_recovers_from_unsatisfied_range() {
        let _guard = set_test_http_retries(0);
        let (port, count, requests) = spawn_recording_server(vec![
            truncated_download_response(),
            range_not_satisfiable_response(),
            full_download_response(),
        ])
        .await;
        let url = format!("http://127.0.0.1:{port}/artifact");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("artifact");
        let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

        let _err = client
            .download_file(&url, &destination, None)
            .await
            .unwrap_err();
        client
            .download_file(&url, &destination, None)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
        assert_eq!(count.load(Ordering::SeqCst), 3);
        let requests = requests.lock().unwrap();
        assert!(requests[1].to_ascii_lowercase().contains("range: bytes=5-"));
        assert!(!requests[2].to_ascii_lowercase().contains("range:"));
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
        let _guard = crate::test::lock_ignoring_poison(&TEST_SETTINGS_LOCK);
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
