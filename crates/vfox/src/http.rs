use reqwest::{Client, ClientBuilder, StatusCode};
use std::sync::LazyLock;
use std::time::Duration;

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    ClientBuilder::new()
        .user_agent(format!("vfox.rs/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("Failed to create reqwest client")
});

/// Default retry attempts when MISE_HTTP_RETRIES is unset. Mirrors the
/// `http_retries` setting default in the main mise crate.
const DEFAULT_HTTP_RETRIES: usize = 3;

/// Backoff schedule (ms) shared with the main mise crate. Hand-rolled rather
/// than using ExponentialBackoff::from_millis (which is geometric in the base
/// value) so the human-readable cadence is obvious. Jitter is applied per delay.
const BACKOFF_SCHEDULE_MS: &[u64] = &[200, 1_000, 4_000, 15_000];

/// Read MISE_HTTP_RETRIES so vfox honors the same opt-out as the rest of mise.
/// vfox is a separate crate without access to mise's Settings layer, so the env
/// var is the only shared signal.
fn http_retries() -> usize {
    std::env::var("MISE_HTTP_RETRIES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_HTTP_RETRIES)
}

/// Total attempts = retries + initial attempt.
pub(crate) fn http_retry_attempts() -> usize {
    http_retries().saturating_add(1)
}

pub(crate) fn should_retry_status(status: StatusCode) -> bool {
    let code = status.as_u16();
    code == 408 || code == 429 || (500..600).contains(&code)
}

pub(crate) fn is_transient(err: &reqwest::Error) -> bool {
    if err.is_timeout() || err.is_connect() || err.is_body() {
        return true;
    }
    if let Some(status) = err.status() {
        return should_retry_status(status);
    }
    false
}

/// Backoff for the `n`-th retry (0-indexed). Falls back to the longest delay
/// in the schedule for retries beyond it. A small uniform jitter in [50%, 100%]
/// of the base avoids thundering herd while keeping delays at least half the
/// nominal value.
pub(crate) fn retry_delay(attempt: usize) -> Duration {
    let base_ms = BACKOFF_SCHEDULE_MS
        .get(attempt)
        .copied()
        .unwrap_or_else(|| *BACKOFF_SCHEDULE_MS.last().unwrap());
    // Cheap deterministic-ish jitter from the system clock — vfox is a small
    // crate and pulling in `rand` just for this isn't worth it.
    let jitter_pct = 50
        + (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() % 50)
            .unwrap_or(0)) as u64;
    Duration::from_millis(base_ms * jitter_pct / 100)
}

/// Retry an async operation that issues a request AND extracts the body.
/// Use for download/text/bytes flows where mid-stream failures (is_body()) need
/// to restart the whole request. Warns immediately on each transient failure
/// (so users see flakiness without waiting through the backoff) and again on
/// eventual success or final exhaustion.
pub(crate) async fn retry_async<F, Fut, T>(
    url: &str,
    mut f: F,
) -> std::result::Result<T, reqwest::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, reqwest::Error>>,
{
    let attempts = http_retry_attempts().max(1);
    let mut had_transient_failure = false;
    let mut last_err: Option<reqwest::Error> = None;
    for attempt in 0..attempts {
        match f().await {
            Ok(value) => {
                if had_transient_failure {
                    log::warn!("HTTP {} succeeded on attempt {}", url, attempt + 1);
                }
                return Ok(value);
            }
            Err(err) => {
                if !is_transient(&err) {
                    return Err(err);
                }
                if attempt + 1 >= attempts {
                    log::warn!(
                        "HTTP {} failed after {} attempts: {}",
                        url,
                        attempt + 1,
                        err
                    );
                    return Err(err);
                }
                let delay = retry_delay(attempt);
                log::warn!(
                    "HTTP {} attempt {} failed (transient): {}; retrying in {:?}",
                    url,
                    attempt + 1,
                    err,
                    delay
                );
                had_transient_failure = true;
                last_err = Some(err);
                tokio::time::sleep(delay).await;
            }
        }
    }
    Err(last_err.expect("retry loop should always return"))
}
