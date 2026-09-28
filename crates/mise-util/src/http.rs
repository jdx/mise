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
    ACCEPT_ENCODING, AUTHORIZATION, CONTENT_RANGE, CONTENT_TYPE, COOKIE, DATE, ETAG, HeaderMap,
    HeaderName, HeaderValue, IF_RANGE, LAST_MODIFIED, PROXY_AUTHORIZATION, RANGE,
};
use reqwest::{ClientBuilder, IntoUrl, Method, Response};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock as Lazy;
use tokio::io::AsyncWriteExt;
use tokio::sync::OnceCell;
use url::Url;

use crate::file::display_path;
use crate::netrc;
use crate::progress::SingleReport;
use crate::time::format_duration;
use crate::{env, file};
use mise_settings::Settings;

pub static HTTP: Lazy<Client> = Lazy::new(|| {
    Client::new_shared(
        crate::network::http_timeout(&Settings::get()),
        ClientKind::Http,
    )
});

pub static HTTP_FETCH: Lazy<Client> = Lazy::new(|| {
    Client::new_shared(
        crate::network::configured_fetch_remote_versions_timeout(&Settings::get()),
        ClientKind::Fetch,
    )
});

/// Follows HTTPS-to-HTTP redirects, which [`HTTP`] refuses. Private so the only
/// way to use it is [`download_file_checksum_pinned`], which rejects the bytes
/// unless they match a checksum the caller already holds; the transport then
/// adds nothing to the integrity of the result. Mirror redirectors need this:
/// `ftpmirror.gnu.org`, the URL of every GNU formula, sends some regions to
/// plain-HTTP mirrors. Unix-only, like its one caller, brew source builds.
#[cfg(unix)]
static HTTP_CHECKSUM_PINNED: Lazy<Client> = Lazy::new(|| {
    Client::new_shared_with(
        crate::network::http_timeout(&Settings::get()),
        ClientKind::Http,
        Downgrade::FollowChecksumPinned,
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
    fn check_response(&self, response: Response) -> Result<Response> {
        if self.error_for_status
            && !(self.allow_range_not_satisfiable
                && response.status() == StatusCode::RANGE_NOT_SATISFIABLE)
        {
            response.error_for_status_ref()?;
        }
        Ok(response)
    }

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

/// A GitHub 403 that is really a rate limit.
///
/// GitHub answers an exhausted rate limit with 403, not 429, so without this
/// marker such a response is indistinguishable from a deterministic "you may
/// not do that" and [`is_transient`] declines to retry it — even though
/// `http_retries` documents 429 as retryable. Carrying the whole message means
/// the marker changes classification without changing what the user reads.
#[derive(Debug)]
struct GithubRateLimited(String);

impl std::fmt::Display for GithubRateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GithubRateLimited {}

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
pub struct DownloadFileMetadata {
    pub effective_filename: Option<String>,
}

type SharedDownload =
    std::sync::Arc<tokio::sync::OnceCell<(tempfile::TempDir, DownloadFileMetadata)>>;
static INVOCATION_DOWNLOADS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, SharedDownload>>,
> = std::sync::LazyLock::new(Default::default);

// Bound retained scratch space, excluding transfers still borrowed by callers.
// Active transfers are bounded by their callers' jobs limit and remain shared.
fn prune_shared_downloads(
    downloads: &mut std::collections::HashMap<String, SharedDownload>,
    max_bytes: u64,
    max_entries: usize,
) {
    let size = |entry: &SharedDownload| {
        entry
            .get()
            .and_then(|(directory, _)| {
                std::fs::metadata(directory.path().join("artifact"))
                    .ok()
                    .map(|m| m.len())
            })
            .unwrap_or(0)
    };
    let mut bytes: u64 = downloads.values().map(size).sum();
    let mut count = downloads.len();
    downloads.retain(|_, entry| {
        if (bytes > max_bytes || count > max_entries) && Arc::strong_count(entry) == 1 {
            bytes = bytes.saturating_sub(size(entry));
            count -= 1;
            false
        } else {
            true
        }
    });
}

/// Own temporary shared artifacts for this command, including cancellation paths.
pub struct InvocationDownloads;

impl Drop for InvocationDownloads {
    fn drop(&mut self) {
        INVOCATION_DOWNLOADS.lock().unwrap().clear();
    }
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

/// The prefix `tempfile` builds the download-state name from. Named so the length the hint
/// measures and the length that is actually created cannot drift apart.
const DOWNLOAD_STATE_PREFIX: &str = ".mise-download-state.";

/// Name the operation and the path, and add the platform hint the bare error does not carry.
///
/// Every downloaded file lands through `tempfile`, whose persist does **not** get the
/// extended-length path handling `std::fs` applies — `file::PreparedAtomicWrite::commit` records
/// the measurement: it breaks at a 253-character target while `fs::rename` on the same tree
/// succeeds at 415. So on Windows this is the first thing to fail as a directory approaches
/// `MAX_PATH`, and until now it failed as a bare `os error 3` with nothing naming the cause.
fn io_error(err: std::io::Error, doing: &str, path: &Path) -> eyre::Report {
    // The hint is resolved while `err` is still borrowable, then the error becomes the source and
    // the message the context -- the same order `PreparedAtomicWrite::commit` uses.
    let msg = file::with_io_hint(format!("{doing}: {}", display_path(path)), path, &err);
    eyre::Report::new(err).wrap_err(msg)
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
        // The name `tempfile` is about to generate, not the directory holding it: the hint decides
        // from the length of what it is given, and the generated name is 27 units longer than the
        // directory. Measuring the directory would leave a window where the temp path is over the
        // limit while the directory is under the hint's threshold, and the failure would go back
        // to being unexplained. `XXXXXX` stands in for the six random characters and is the same
        // length, so what is measured is what Windows sees.
        let temp_name = parent.join(format!("{DOWNLOAD_STATE_PREFIX}XXXXXX"));
        let mut temp = tempfile::NamedTempFile::with_prefix_in(DOWNLOAD_STATE_PREFIX, parent)
            .map_err(|err| io_error(err, "failed to create the download state file", &temp_name))?;
        serde_json::to_writer(&mut temp, state)?;
        temp.as_file_mut().sync_all()?;
        temp.persist(&self.state_path).map_err(|err| {
            io_error(
                err.error,
                "failed to write the download state file",
                &self.state_path,
            )
        })?;
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
            return Err(io_error(
                error,
                "failed to move the downloaded file into place",
                destination,
            ));
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
pub fn cleanup_download_dir(path: &Path) -> Result<()> {
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

/// Download `url` to `path` and verify it against `sha256`, following a
/// redirect from HTTPS to HTTP if a mirror sends one. On a mismatch the file is
/// removed so no caller can pick up unverified bytes.
#[cfg(unix)]
pub async fn download_file_checksum_pinned<U: IntoUrl>(
    url: U,
    path: &Path,
    sha256: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    HTTP_CHECKSUM_PINNED.download_file(url, path, pr).await?;
    verify_sha256_or_remove(path, sha256, pr)
}

#[cfg(unix)]
fn verify_sha256_or_remove(path: &Path, sha256: &str, pr: Option<&dyn SingleReport>) -> Result<()> {
    if let Err(err) = crate::hash::ensure_checksum(path, sha256, pr, "sha256") {
        let _ = file::remove_file(path);
        return Err(err);
    }
    Ok(())
}

/// Whether a client may follow a redirect from HTTPS to HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Downgrade {
    Refuse,
    /// Only for `HTTP_CHECKSUM_PINNED`.
    #[cfg(unix)]
    FollowChecksumPinned,
}

/// Follow redirects as reqwest normally would, but refuse to step down from
/// HTTPS to HTTP part-way through.
///
/// `what` names the request in the error, since the two clients carry very
/// different traffic.
fn https_downgrade_policy(what: &'static str) -> reqwest::redirect::Policy {
    use reqwest::redirect::Policy;

    Policy::custom(move |attempt| {
        if is_https_downgrade(attempt.previous(), attempt.url()) {
            attempt.error(std::io::Error::other(format!(
                "refusing to redirect {what} from HTTPS to HTTP"
            )))
        } else {
            Policy::default().redirect(attempt)
        }
    })
}

pub fn is_https_downgrade(previous: &[Url], next: &Url) -> bool {
    previous
        .last()
        .is_some_and(|url| url.scheme() == "https" && next.scheme() != "https")
}

#[derive(Debug)]
pub struct Client {
    reqwest: Result<reqwest::Client, String>,
    timeout: Duration,
    kind: ClientKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    Http,
    Fetch,
}

impl ClientKind {
    /// Names this client's traffic in the redirect error. Exhaustive, so a new
    /// kind cannot be added without deciding what to call it.
    fn redirect_subject(self) -> &'static str {
        match self {
            Self::Http => "a download",
            Self::Fetch => "a remote version request",
        }
    }
}

impl Client {
    #[doc(hidden)]
    pub fn new(timeout: Duration, kind: ClientKind) -> Result<Self> {
        Ok(Self {
            reqwest: Ok(Self::build(timeout, kind, Downgrade::Refuse)?),
            timeout,
            kind,
        })
    }

    fn new_shared(timeout: Duration, kind: ClientKind) -> Self {
        Self::new_shared_with(timeout, kind, Downgrade::Refuse)
    }

    fn new_shared_with(timeout: Duration, kind: ClientKind, downgrade: Downgrade) -> Self {
        Self {
            reqwest: Self::build(timeout, kind, downgrade).map_err(|err| format!("{err:#}")),
            timeout,
            kind,
        }
    }

    fn build(timeout: Duration, kind: ClientKind, downgrade: Downgrade) -> Result<reqwest::Client> {
        let builder = Self::_new().read_timeout(timeout).connect_timeout(timeout);
        // Applied to every kind rather than per match arm, so no client can be
        // added — or edited back — into existence without it. Downloads are
        // checksum-verified where a checksum is known, but not every caller has
        // one, and a silent downgrade is worth refusing on its own. Redirects
        // are otherwise unchanged: the policy defers to the default for
        // anything that is not a downgrade. The one exception is
        // `HTTP_CHECKSUM_PINNED`, whose only caller verifies every byte.
        let policy = match downgrade {
            Downgrade::Refuse => https_downgrade_policy(kind.redirect_subject()),
            #[cfg(unix)]
            Downgrade::FollowChecksumPinned => reqwest::redirect::Policy::default(),
        };
        let builder = builder.redirect(policy);
        Ok(builder.build()?)
    }

    #[doc(hidden)]
    pub fn with_init_error(error: impl Into<String>) -> Self {
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
        ClientBuilder::new()
            .user_agent(crate::user_agent::get())
            .gzip(true)
            .zstd(true)
    }

    fn request_timeout(&self) -> Duration {
        match self.kind {
            ClientKind::Fetch if crate::network::bound_remote_version_lookups(&Settings::get()) => {
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
        ensure!(
            !crate::network::offline(&Settings::get()),
            "offline mode is enabled"
        );
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
        ensure!(
            !crate::network::offline(&Settings::get()),
            "offline mode is enabled"
        );
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
        ensure!(
            !crate::network::offline(&Settings::get()),
            "offline mode is enabled"
        );
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
            retries: crate::network::http_retries(&Settings::get()),
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
        ensure!(
            !crate::network::offline(&Settings::get()),
            "offline mode is enabled"
        );
        let url = url.into_url()?;
        debug!("POST {}", url);
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

    pub async fn download_file_with_metadata<U: IntoUrl>(
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
        let url = url.into_url()?;
        if Settings::get().generate_lockfiles() {
            let key = format!("{:p}:{}", self, download_request_hash(&url, headers));
            let shared = INVOCATION_DOWNLOADS
                .lock()
                .unwrap()
                .entry(key)
                .or_default()
                .clone();
            let (directory, metadata) = shared
                .get_or_try_init(|| async {
                    let directory = tempfile::tempdir()?;
                    let metadata = self
                        .download_file_with_headers_timeout(
                            url.clone(),
                            &directory.path().join("artifact"),
                            headers,
                            pr,
                            crate::network::http_download_timeout(&Settings::get()),
                        )
                        .await?;
                    Ok::<_, eyre::Report>((directory, metadata))
                })
                .await?;
            if let Some(parent) = path.parent() {
                file::create_dir_all(parent)?;
            }
            let partial = PartialDownload::new(path, download_request_hash(&url, headers))?;
            let lock_path = partial.path.clone();
            let _download_lock = tokio::task::spawn_blocking(move || {
                crate::lock_file::LockFile::new(&lock_path).lock()
            })
            .await??;
            partial.clear()?;
            file::copy(directory.path().join("artifact"), &partial.path)?;
            partial.persist(path)?;
            let metadata = metadata.clone();
            drop(shared);
            prune_shared_downloads(
                &mut INVOCATION_DOWNLOADS.lock().unwrap(),
                512 * 1024 * 1024,
                64,
            );
            return Ok(metadata);
        }
        self.download_file_with_headers_timeout(
            url,
            path,
            headers,
            pr,
            crate::network::http_download_timeout(&Settings::get()),
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
        ensure!(
            !crate::network::offline(&Settings::get()),
            "offline mode is enabled"
        );
        let url = url.into_url()?;
        debug!("GET Downloading {} to {}", url, display_path(path));
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
        let progress = Arc::new(DownloadProgress::default());

        // Retry the whole transfer, resuming a validated partial response when
        // possible. send_once_with_https_fallback_allow_416 (not
        // send_with_https_fallback) is used inside to avoid retry-on-retry.
        let download = retry_async("GET", &url, || {
            let attempt = attempt.clone();
            let progress = progress.clone();
            let request_url = url.clone();
            let partial = partial.clone();
            async move {
                attempt.fetch_add(1, Ordering::Relaxed);
                progress.start_attempt();
                self.download_file_attempt(request_url, headers, &partial, pr, &progress)
                    .await
            }
        });

        // Warn (once) when the transfer crawls, instead of silently waiting out
        // the whole budget: a throttled host that trickles bytes never trips the
        // per-read `http_timeout`, and shims queued behind this install's lock
        // would otherwise wait with no hint of why.
        let download = async {
            tokio::select! {
                result = download => result,
                never = warn_when_download_is_slow(&url, &progress) => match never {},
            }
        };

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
                    progress.attempt.load(Ordering::Relaxed),
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
        progress: &DownloadProgress,
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
            // A relayed response comes from the local adapter (`localhost`);
            // the upstream GitHub host is the one actually sending the bytes.
            #[cfg(unix)]
            let served_by = if github_relay_socket(&url).is_some() {
                &url
            } else {
                resp.url()
            };
            #[cfg(not(unix))]
            let served_by = resp.url();
            progress.served_by(served_by);

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
                    if crate::cancel::is_cancelled() {
                        bail!("download cancelled by user");
                    }
                    file.write_all(&chunk).await?;
                    progress
                        .attempt
                        .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    if let Some(pr) = pr {
                        pr.inc(chunk.len() as u64);
                    }
                }
                Ok::<_, Report>(())
            }
            .await;
            // Outside the transfer, so it also runs when the transfer failed.
            // `tokio::fs::File` buffers writes and dropping one does not flush,
            // so a transfer that dies part way through was leaving the bytes it
            // had already received unwritten — the resume then asked for a range
            // starting before them and downloaded them again. Whether any of it
            // survived depended on when the background flush happened to land.
            let persisted = async {
                file.shutdown().await?;
                file.sync_all().await?;
                Ok::<_, Report>(())
            }
            .await;
            // Before `clear()`: flushing into a file that has just been removed
            // would recreate it.
            if transfer.is_err()
                && !resumable
                && let Err(err) = partial.clear()
            {
                debug!("failed to remove unvalidated partial download: {err:#}");
            }
            transfer?;
            persisted?;

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
            crate::network::http_retries(&Settings::get()),
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
            crate::network::http_retries(&Settings::get()),
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
        crate::resolve_progress::fetching(&url);
        #[cfg(unix)]
        if let Some(socket) = github_relay_socket(&url) {
            let response = crate::github_relay::unix::request(
                std::path::Path::new(&socket),
                method,
                &url,
                headers,
            )
            .await?;
            // This path returns before the GitHub 403 handling below, so without
            // this a relayed rate limit stays a bare status error that
            // `is_transient` calls deterministic and never retries. The relay
            // authenticates upstream itself, so report auth from the headers we
            // sent rather than claiming a token we do not hold. If the adapter
            // does not forward the rate-limit headers this simply does not
            // match, leaving the previous behaviour.
            if options.error_for_status
                && is_github_forbidden(&url, &response)
                && is_github_rate_limited(&response)
            {
                let status_error = response
                    .error_for_status_ref()
                    .expect_err("403 response should be an error");
                let used_github_token = headers.contains_key(AUTHORIZATION);
                let rate_limit = github_rate_limit_summary(&response);
                let body = read_bounded_error_body(response, self.timeout).await;
                return Err(github_forbidden_report(
                    status_error,
                    used_github_token,
                    rate_limit,
                    true,
                    &body,
                ));
            }
            return options.check_response(response);
        }
        apply_url_replacements(&mut url);
        crate::resolve_progress::fetching(&url);
        let host_key = http_host_key(&url);
        if crate::network::prefer_offline(&Settings::get())
            && let Some(host) = &host_key
            && let Some(cause) = UNAVAILABLE_HTTP_HOSTS.lock().unwrap().get(host).cloned()
        {
            return Err(UnavailableHttpHost {
                origin: host.clone(),
                cause,
            }
            .into());
        }
        debug!("{} {}", verb_label, url);

        // Apply netrc credentials after URL replacement.
        //
        // netrc is treated as a *fallback*, mirroring curl's behavior: an
        // explicit Authorization header (e.g. the forge token resolved by
        // `host_auth_headers` from GITHUB_TOKEN/gh/github_tokens.toml) wins
        // over netrc. The one exception is when a URL replacement actually
        // redirected the request to a different URL — in that case the
        // pre-existing credentials were built for the *original* host and are
        // removed before netrc credentials scoped to the new host are applied.
        // This preserves the #7164 use case
        // (replace a public URL with a private mirror authenticated via
        // netrc) without clobbering forge tokens on un-redirected requests.
        let mut final_headers = headers.clone();
        clear_cross_host_credentials(&mut final_headers, &original_url, &mut url);
        // Refuse the downgrade for credentials that survive host scoping: a same-host
        // rewrite keeps the original Authorization, and userinfo written into the rule
        // itself is sent as-is. This runs before netrc because netrc credentials are
        // scoped to the *replacement* host by the user, who already gets them sent to a
        // plain `http://` URL with no rewrite involved — a rewrite must not be stricter
        // than the direct request, or an http mirror fronting an https origin (#7164)
        // becomes unusable.
        ensure_secure_replacement_credentials(&original_url, &url, &final_headers)?;
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
                if crate::network::prefer_offline(&Settings::get())
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
                            verb_label, url
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
            let rate_limited = is_github_rate_limited(&resp);
            let body = read_bounded_error_body(resp, self.timeout).await;
            // Retry without auth when the response mentions IP allow lists: GitHub App
            // installation tokens (`ghs_*`) get 403 on public API resources for orgs with IP
            // allow lists; stripping auth avoids that path.
            // https://github.com/orgs/community/discussions/191185
            // https://github.com/jdx/mise/discussions/9119
            // Kept to api.github.com: stripping Authorization and re-sending is
            // a response to that specific GitHub.com behaviour, not something
            // to start doing against an enterprise host.
            if used_github_token
                && url.host_str() == Some("api.github.com")
                && body.contains("IP allow list")
            {
                let mut headers = final_headers;
                headers.remove(AUTHORIZATION);
                debug!(
                    "{} {} retrying without GitHub auth after {}",
                    verb_label, url, status
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
                rate_limited,
                &body,
            ));
        }
        options.check_response(resp)
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
        ensure!(
            !crate::network::offline(&Settings::get()),
            "offline mode is enabled"
        );
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

/// Matches what [`is_github_unauthorized`] accepts, so a GitHub Enterprise
/// Server host gets the same 403 report and rate-limit classification as
/// api.github.com rather than a bare status error.
fn is_github_forbidden(url: &Url, resp: &Response) -> bool {
    resp.status() == StatusCode::FORBIDDEN && crate::github::is_github_api_url(url)
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
    rate_limited: bool,
    body: &str,
) -> Report {
    let token_status = if used_github_token { "yes" } else { "no" };
    let rate_limit = rate_limit
        .map(|summary| format!("\ngithub rate limit: {summary}"))
        .unwrap_or_default();
    let body = format_response_body(body);
    let message =
        format!("{status_error}\ngithub auth: {token_status}{rate_limit}\ngithub response: {body}");
    if rate_limited {
        return GithubRateLimited(message).into();
    }
    eyre!("{message}")
}

/// Whether a GitHub 403 is a rate limit rather than a refusal.
///
/// `x-ratelimit-remaining: 0` covers the primary limit; `retry-after` covers
/// the secondary limits, which can arrive with quota still on the clock. Mirrors
/// what [`display_github_rate_limit`] reports so the two cannot disagree.
fn is_github_rate_limited(resp: &Response) -> bool {
    let headers = resp.headers();
    let exhausted = headers
        .get("x-ratelimit-remaining")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|remaining| remaining == "0");
    exhausted || headers.contains_key("retry-after")
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
    // raw.githubusercontent.com is not an API host, but a private repository's
    // files are a 404 without the token, so it is routed here too. `get_headers`
    // decides what each host actually gets.
    if crate::github::is_github_api_url(url) || crate::github::is_github_raw_content_url(url) {
        return crate::github::get_headers(url.as_str());
    }

    // A generic URL does not carry the configured GitLab or Forgejo API origin,
    // so it cannot establish a safe trust boundary for those tokens. Their
    // backend-specific callers must pass the request and API URLs directly to
    // the corresponding `get_headers` function.
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

fn is_credential_header(name: &HeaderName, value: &HeaderValue) -> bool {
    if value.is_sensitive()
        || name == AUTHORIZATION
        || name == PROXY_AUTHORIZATION
        || name == COOKIE
    {
        return true;
    }
    let name = name.as_str();
    name == "api-key"
        || name == "x-api-key"
        || name.ends_with("-api-key")
        || name.contains("token")
        || name.contains("secret")
        || name.contains("credential")
}

/// Drop credentials scoped to the original host before contacting a replacement host.
pub fn clear_cross_host_credentials(headers: &mut HeaderMap, original_url: &Url, url: &mut Url) {
    if url.host() == original_url.host() {
        return;
    }
    let credential_headers = headers
        .iter()
        .filter(|(name, value)| is_credential_header(name, value))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    for name in credential_headers {
        headers.remove(name);
    }
    let inherited_userinfo = (!original_url.username().is_empty()
        || original_url.password().is_some())
        && url.username() == original_url.username()
        && url.password() == original_url.password();
    if inherited_userinfo {
        url.set_password(None)
            .expect("HTTP URLs support clearing passwords");
        url.set_username("")
            .expect("HTTP URLs support clearing usernames");
    }
}

/// Merge `netrc` credentials into `final_headers`, honoring the fallback
/// policy in [`netrc_should_apply`]. `original_url` is the URL before any
/// `apply_url_replacements` rewrite and `url` is the (possibly rewritten)
/// URL actually being requested; a change of *host* means the request was
/// redirected to a different server, which lets netrc supply replacement-host
/// authentication after original-host credentials have been removed. Netrc
/// values are `insert`ed so only one Authorization value is present.
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

/// Reject credentials when a URL replacement downgrades an HTTPS request to HTTP.
pub fn ensure_secure_replacement_credentials(
    original_url: &Url,
    url: &Url,
    headers: &HeaderMap,
) -> Result<()> {
    let has_credentials = headers
        .iter()
        .any(|(name, value)| is_credential_header(name, value));
    ensure_secure_url_replacement(original_url, url, has_credentials)
}

pub fn ensure_secure_url_replacement(
    original_url: &Url,
    url: &Url,
    has_credentials: bool,
) -> Result<()> {
    let downgraded = original_url.scheme() == "https" && url.scheme() == "http";
    let has_credentials = has_credentials || !url.username().is_empty() || url.password().is_some();
    ensure!(
        !downgraded || !has_credentials,
        "refusing to send credentials over an HTTPS-to-HTTP URL replacement"
    );
    Ok(())
}

/// Get HTTP Basic authentication headers from netrc file for the given URL
pub fn netrc_headers(url: &Url) -> HeaderMap {
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
pub fn resolve_pagination_url(current: &str, next: &str) -> Result<String> {
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
/// The GitHub relay socket to send `url` through instead of the network,
/// when mise runs behind a relay adapter.
#[cfg(unix)]
fn github_relay_socket(url: &Url) -> Option<std::ffi::OsString> {
    if matches!(url.host_str(), Some("github.com" | "api.github.com")) {
        std::env::var_os("MISE_GITHUB_RELAY_SOCKET")
    } else {
        None
    }
}

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

pub fn default_backoff_strategy(retries: i64) -> impl Iterator<Item = Duration> {
    // Hand-rolled schedule (with jitter): ~200ms / ~1s / ~4s / ~15s, then 15s
    // for every retry beyond the schedule. The trailing repeat matters because
    // `MISE_HTTP_RETRIES` can be set arbitrarily high — a fixed-length array
    // would silently cap retries at its length. tokio_retry's ExponentialBackoff
    // ::from_millis is geometric in the base (base, base*base, …) so picking a
    // base that gives nice human-scale delays is awkward; explicit is clearer.
    // Retry tests assert attempts and outcomes, not wall-clock sleeping.
    let schedule = if crate::testing::in_tests() {
        [2u64, 10, 40, 150]
    } else {
        [200u64, 1_000, 4_000, 15_000]
    };
    schedule
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
pub fn is_transient(err: &Report) -> bool {
    if is_dns_error(err.as_ref()) {
        return false;
    }
    err.chain().any(|e| {
        if e.downcast_ref::<DownloadSizeMismatch>().is_some() {
            return true;
        }
        // GitHub answers a rate limit with 403, which the status check below
        // treats as deterministic. Classify it with the 429 it means.
        if e.downcast_ref::<GithubRateLimited>().is_some() {
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
pub async fn retry_async<F, Fut, T>(verb_label: &str, url: &Url, f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    retry_async_with_retries(
        verb_label,
        url,
        crate::network::http_retries(&Settings::get()),
        f,
    )
    .await
}

pub async fn retry_async_with_retries<F, Fut, T>(
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
                crate::resolve_progress::retrying(url, attempt + 1, delay);
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

/// Averaged over a full [`SLOW_DOWNLOAD_WINDOW`], throughput below this is
/// reported. At 16 KiB/s the default 30 minute `http_download_timeout` covers
/// under 30 MB, so a typical tool archive would not finish in time.
const SLOW_DOWNLOAD_BYTES_PER_SEC: u64 = 16 * 1024;
const SLOW_DOWNLOAD_WINDOW: Duration = Duration::from_secs(60);
const SLOW_DOWNLOAD_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

/// Progress of a retried download. `attempt` restarts at zero on each retry
/// (the timeout error reports it per attempt); `total` never goes backwards,
/// which the slow-download watchdog relies on.
#[derive(Default)]
struct DownloadProgress {
    attempt: AtomicU64,
    earlier_attempts: AtomicU64,
    /// Server of the latest response, after `url_replacements` and redirects
    /// (the upstream GitHub host for relayed responses).
    served_by: Mutex<Option<ServedBy>>,
}

/// When a host started serving a download, so throughput can be split
/// exactly between hosts when a retry lands somewhere else.
#[derive(Clone, Debug, PartialEq)]
struct ServedBy {
    host: String,
    since: Instant,
    total_at_start: u64,
}

impl DownloadProgress {
    fn served_by(&self, url: &Url) {
        let Some(host) = url.host_str() else {
            return;
        };
        let mut served_by = self.served_by.lock().unwrap();
        if served_by.as_ref().is_none_or(|s| s.host != host) {
            *served_by = Some(ServedBy {
                host: host.to_string(),
                since: Instant::now(),
                total_at_start: self.total(),
            });
        }
    }

    fn start_attempt(&self) {
        self.earlier_attempts
            .fetch_add(self.attempt.swap(0, Ordering::Relaxed), Ordering::Relaxed);
    }

    fn total(&self) -> u64 {
        self.earlier_attempts.load(Ordering::Relaxed) + self.attempt.load(Ordering::Relaxed)
    }
}

/// Tracks transfer throughput over tumbling windows of a monotonic byte total.
struct SlowDownloadDetector {
    window_start: Instant,
    window_start_total: u64,
}

impl SlowDownloadDetector {
    fn new(now: Instant) -> Self {
        Self {
            window_start: now,
            window_start_total: 0,
        }
    }

    /// Starts a fresh window, discarding what the current one measured.
    fn restart(&mut self, now: Instant, total: u64) {
        self.window_start = now;
        self.window_start_total = total;
    }

    /// Records the bytes received so far across all attempts and, once a full
    /// window has elapsed, returns that window's rate in bytes/sec if it was
    /// too slow.
    fn observe(&mut self, now: Instant, total: u64) -> Option<u64> {
        let elapsed = now.duration_since(self.window_start);
        if elapsed < SLOW_DOWNLOAD_WINDOW {
            return None;
        }
        let bytes = total.saturating_sub(self.window_start_total);
        self.window_start = now;
        self.window_start_total = total;
        let rate = (bytes as f64 / elapsed.as_secs_f64()) as u64;
        (rate < SLOW_DOWNLOAD_BYTES_PER_SEC).then_some(rate)
    }
}

/// Decides, sample by sample, whether a download has been slow and which host
/// to blame. Each host is measured on its own: when a retry lands on another
/// host, the outgoing host's window ends exactly where the new host started.
struct SlowDownloadWatch {
    detector: SlowDownloadDetector,
    host: Option<String>,
}

impl SlowDownloadWatch {
    fn new(now: Instant) -> Self {
        Self {
            detector: SlowDownloadDetector::new(now),
            host: None,
        }
    }

    /// Returns the host to blame (if known) and its rate when a full window
    /// was too slow.
    fn sample(
        &mut self,
        now: Instant,
        total: u64,
        served_by: Option<&ServedBy>,
    ) -> Option<(Option<String>, u64)> {
        if let Some(served) = served_by
            && self.host.as_deref() != Some(served.host.as_str())
        {
            let previous = self.host.replace(served.host.clone());
            let slow = self.detector.observe(served.since, served.total_at_start);
            self.detector.restart(served.since, served.total_at_start);
            if let Some(rate) = slow {
                return Some((previous, rate));
            }
        }
        self.detector
            .observe(now, total)
            .map(|rate| (self.host.clone(), rate))
    }
}

/// Runs alongside a download and never completes. Warns at most once.
///
/// Names only the host: download URLs can carry credentials in their userinfo,
/// query, or path (presigned URLs), and this is a warning, not a debug line.
async fn warn_when_download_is_slow(
    url: &Url,
    progress: &DownloadProgress,
) -> std::convert::Infallible {
    let mut watch = SlowDownloadWatch::new(Instant::now());
    let mut ticks = tokio::time::interval(SLOW_DOWNLOAD_SAMPLE_INTERVAL);
    ticks.tick().await;
    loop {
        ticks.tick().await;
        let served_by = progress.served_by.lock().unwrap().clone();
        if let Some((host, rate)) =
            watch.sample(Instant::now(), progress.total(), served_by.as_ref())
        {
            let host = host
                .or_else(|| url.host_str().map(str::to_string))
                .unwrap_or_else(|| "the server".to_string());
            warn!(
                "download from {} is very slow ({}/s over the last minute). \
                 mise keeps trying until `http_download_timeout` runs out; \
                 if the host is throttling this connection, switching to a mirror may help",
                host,
                bytesize::ByteSize::b(rate).display().iec(),
            );
            return std::future::pending().await;
        }
    }
}

#[cfg(test)]
mod tests;
