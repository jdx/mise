use super::*;
use confique::Layer;
use indexmap::IndexMap;
use reqwest::dns::{Name, Resolve, Resolving};
use std::path::PathBuf;
use url::Url;

mod auth;
mod download;
mod progress;
mod request;
mod retry;
mod url_replacements;

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

struct FailingDnsResolver;

impl Resolve for FailingDnsResolver {
    fn resolve(&self, _name: Name) -> Resolving {
        Box::pin(async {
            Err(
                std::io::Error::new(std::io::ErrorKind::NotFound, "test DNS resolution failure")
                    .into(),
            )
        })
    }
}

// Helper to create test settings with specific URL replacements
fn with_test_settings<F, R>(replacements: IndexMap<String, String>, test_fn: F) -> R
where
    F: FnOnce() -> R,
{
    // `SettingsGuard` holds the lock and calls `crate::testing::reset_settings(None)` in `Drop`, which runs
    // while unwinding. Resetting after `test_fn` instead would leave the replacements behind
    // for the next test whenever this one panics -- previously the lock's poison flag hid
    // that by failing every later test outright.
    let _guard = SettingsGuard {
        _lock: crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK),
    };

    // Create settings with custom URL replacements
    let mut settings = mise_settings::SettingsPartial::empty();
    settings.url_replacements = Some(replacements);

    // Set settings for this test
    crate::testing::reset_settings(Some(settings));

    test_fn()
}

// RAII guard that holds the global test lock and resets settings on drop.
// Use this in async tests so the mutex stays held across .await points
// without sync/async closure shenanigans.
struct SettingsGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}
impl Drop for SettingsGuard {
    fn drop(&mut self) {
        crate::testing::reset_settings(None);
    }
}
fn set_test_http_retries(retries: i64) -> SettingsGuard {
    let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
    let mut settings = mise_settings::SettingsPartial::empty();
    settings.http_retries = Some(retries);
    crate::testing::reset_settings(Some(settings));
    SettingsGuard { _lock: lock }
}
fn set_test_prefer_offline(http_retries: i64) -> SettingsGuard {
    let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
    let mut settings = mise_settings::SettingsPartial::empty();
    settings.prefer_offline = Some(true);
    settings.http_retries = Some(http_retries);
    crate::testing::reset_settings(Some(settings));
    SettingsGuard { _lock: lock }
}
fn set_test_offline() -> SettingsGuard {
    let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
    let mut settings = mise_settings::SettingsPartial::empty();
    settings.offline = Some(true);
    crate::testing::reset_settings(Some(settings));
    SettingsGuard { _lock: lock }
}

#[tokio::test(flavor = "current_thread")]
async fn test_generation_shares_concurrent_artifact_downloads() {
    let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
    let mut settings = mise_settings::SettingsPartial::empty();
    settings.lockfile_mode = Some("generate".into());
    crate::testing::reset_settings(Some(settings));
    let _settings = SettingsGuard { _lock: lock };
    let _downloads = InvocationDownloads;
    let (port, count) = spawn_canned_server(vec![ok_response()]).await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let (a, b) = tokio::join!(
        client.download_file(&url, &first, None),
        client.download_file(&url, &second, None)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    let (a, b) = tokio::join!(
        client.download_file(&url, &first, None),
        client.download_file(&url, &first, None)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(
        std::fs::read(first).unwrap(),
        std::fs::read(second).unwrap()
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn shared_download_retention_evicts_idle_bytes_but_never_active_callers() {
    let make_entry = || {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("artifact"), b"1234").unwrap();
        Arc::new(tokio::sync::OnceCell::new_with(Some((
            directory,
            DownloadFileMetadata::default(),
        ))))
    };
    let active = make_entry();
    let idle = make_entry();
    let idle_path = idle.get().unwrap().0.path().to_path_buf();
    let mut downloads =
        std::collections::HashMap::from([("active".into(), active.clone()), ("idle".into(), idle)]);
    prune_shared_downloads(&mut downloads, 4, 64);
    assert_eq!(downloads.len(), 1);
    assert!(downloads.contains_key("active"));
    assert!(!idle_path.exists());
    prune_shared_downloads(&mut downloads, 0, 0);
    assert_eq!(downloads.len(), 1);
    drop(active);
    prune_shared_downloads(&mut downloads, 0, 0);
    assert!(downloads.is_empty());
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
        crate::testing::reset_settings(None);
    }
}

fn set_test_github_oauth(server_url: &str, cache_path: PathBuf) -> GithubOauthSettingsGuard {
    let settings_lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
    let github_env_lock = crate::testing::lock_ignoring_poison(&crate::github::TEST_ENV_LOCK);
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
    crate::testing::reset_settings(None);

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
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n")
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
fn too_many_requests_response(retry_after: u64) -> &'static str {
    // Leaked so a test can pick the wait; the canned server wants 'static.
    Box::leak(
        format!(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: {retry_after}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_boxed_str(),
    )
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
/// The shape GitHub actually returns once the primary limit is spent —
/// taken from the response that failed the v2026.9.9 docs deploy. Note the
/// 403: GitHub does not use 429 here.
fn github_rate_limited_response() -> &'static str {
    concat!(
        "HTTP/1.1 403 Forbidden\r\n",
        "Content-Type: application/json\r\n",
        "X-RateLimit-Limit: 5000\r\n",
        "X-RateLimit-Remaining: 0\r\n",
        "X-RateLimit-Resource: core\r\n",
        "X-RateLimit-Reset: 1789456621\r\n",
        "Content-Length: 55\r\n",
        "Connection: close\r\n",
        "\r\n",
        r#"{"message":"API rate limit exceeded for installation."}"#
    )
}
/// A secondary rate limit: quota still on the clock, `retry-after` set.
fn github_secondary_rate_limited_response() -> &'static str {
    concat!(
        "HTTP/1.1 403 Forbidden\r\n",
        "Content-Type: application/json\r\n",
        "X-RateLimit-Limit: 5000\r\n",
        "X-RateLimit-Remaining: 117\r\n",
        "X-RateLimit-Resource: core\r\n",
        "Retry-After: 60\r\n",
        "Content-Length: 55\r\n",
        "Connection: close\r\n",
        "\r\n",
        r#"{"message":"You have exceeded a secondary rate limit."}"#
    )
}
fn github_oauth_token_response() -> &'static str {
    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 51\r\nConnection: close\r\n\r\n{\"access_token\":\"ghu-refreshed\",\"expires_in\":28800}"
}
fn seed_github_oauth_cache(cache_path: &Path) {
    let settings = mise_settings::Settings::get();
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
