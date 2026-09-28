use super::*;

pub(crate) const GITHUB_API_URL: &str = "https://api.github.com";
pub(crate) const USER_AGENT_VALUE: &str = "mise-sigstore/0.1.0";

/// Default per-request timeout for attestation API calls. Without this the
/// client would wait indefinitely on a stalled connection (reqwest has no
/// default timeout). Mirrors mise's `http_timeout` default; the embedding crate
/// overrides it via [`RetryConfig`] to honor `MISE_HTTP_TIMEOUT`.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Default number of retries on transient failures. GitHub's attestations API
/// intermittently returns 5xx (e.g. 504 Gateway Timeout) and 429 under load; a
/// single attempt fails the whole install. Mirrors mise's `http_retries`
/// default; the embedding crate overrides it to honor `MISE_HTTP_RETRIES`.
pub(crate) const DEFAULT_RETRIES: usize = 3;
/// Default base backoff before the first retry. Doubles each attempt: ~0.5s / 1s / 2s.
pub(crate) const DEFAULT_BACKOFF_BASE: Duration = Duration::from_millis(500);

/// HTTP retry/timeout policy for the attestation client. Lets the embedding
/// crate (mise) pass its `http_retries` / `http_timeout` settings through
/// instead of the attestation path using a hardcoded policy of its own.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Per-request timeout.
    pub timeout: Duration,
    /// Number of retries on transient failures (total attempts = `retries + 1`).
    pub retries: usize,
    /// Attempt-1 backoff; doubles each subsequent attempt, with equal jitter.
    pub backoff_base: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            retries: DEFAULT_RETRIES,
            backoff_base: DEFAULT_BACKOFF_BASE,
        }
    }
}
/// Upper bound on a server-supplied `Retry-After` wait, so a hostile or buggy
/// header can't stall an install for minutes.
pub(crate) const RETRY_AFTER_MAX: Duration = Duration::from_secs(60);

/// Whether an HTTP status warrants a retry. 429 (rate limit) and any 5xx are
/// transient server-side conditions; everything else (incl. 404) is terminal.
pub(crate) fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Whether a transport-level error warrants a retry: timeouts, connection
/// failures, and mid-stream body drops, which are all transient. Broader classes
/// (TLS handshake, genuine decode errors, builder errors) won't recover on retry
/// so they surface immediately. Matches the transient classification used by
/// mise's main HTTP client and vfox.
pub(crate) fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_body() || is_incomplete_body(err)
}

/// A buffered body read (`.bytes()`) of a truncated response surfaces as a
/// `Decode` error wrapping an `io::ErrorKind::UnexpectedEof` (rather than the
/// `is_body()` kind a streamed read would yield). Detect that specific case so a
/// connection dropped mid-body is retried, without retrying genuine decode
/// errors.
pub(crate) fn is_incomplete_body(err: &reqwest::Error) -> bool {
    use std::error::Error;
    let mut source: Option<&(dyn Error + 'static)> = err.source();
    while let Some(e) = source {
        if let Some(io_err) = e.downcast_ref::<std::io::Error>()
            && io_err.kind() == std::io::ErrorKind::UnexpectedEof
        {
            return true;
        }
        source = e.source();
    }
    false
}

/// Honor a `429`'s `Retry-After` header when present and expressed as
/// delta-seconds (GitHub's form). HTTP-date values are ignored and fall back to
/// exponential backoff. Capped at [`RETRY_AFTER_MAX`].
pub(crate) fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs: u64 = raw.trim().parse().ok()?;
    Some(Duration::from_secs(secs).min(RETRY_AFTER_MAX))
}

/// Backoff delay for the given attempt (1-based) with "equal jitter" in
/// `[d/2, d)` to avoid synchronized retries across concurrent installs. `base`
/// is the attempt-1 delay; it doubles each attempt. A zero base yields no delay
/// (used by tests to keep the suite fast).
pub(crate) fn backoff_delay(base: Duration, attempt: usize) -> Duration {
    let exp = (attempt.saturating_sub(1)).min(16) as u32;
    let scaled = base.saturating_mul(1u32 << exp);
    let half = scaled / 2;
    if half.is_zero() {
        return scaled;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    half + Duration::from_nanos(nanos % half.as_nanos().max(1) as u64)
}
