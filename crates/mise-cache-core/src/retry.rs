use super::*;

/// The longest one request waits because of `Retry-After` headers, summed over
/// its retries. A server that asks for more ends the retries, and the caller
/// reports the request as failed.
pub(crate) const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);

/// A response whose status is an error, with the wait its `Retry-After` header
/// asked for. It displays as the `reqwest` error it holds.
#[derive(Debug)]
pub(crate) struct StatusError {
    source: reqwest::Error,
    retry_after: Option<Duration>,
}

impl std::fmt::Display for StatusError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for StatusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.source)
    }
}

/// The response when its status is 2xx. Any other status is an error: a 4xx or
/// 5xx keeps its `Retry-After`, and a 1xx or 3xx is one the protocol does not
/// allow for the request.
pub(crate) fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response> {
    check_status(response, true)
}

/// [`ensure_success`] for a delegated URL, which is kept out of the error: its
/// query can hold a signature that grants access to the object.
pub(crate) fn ensure_delegated_success(response: reqwest::Response) -> Result<reqwest::Response> {
    check_status(response, false)
}

fn check_status(response: reqwest::Response, show_url: bool) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retry_after = parse_retry_after(response.headers(), SystemTime::now());
    match response.error_for_status() {
        Err(source) => Err(StatusError {
            source: if show_url {
                source
            } else {
                source.without_url()
            },
            retry_after,
        }
        .into()),
        Ok(response) if show_url => bail!(
            "remote cache returned unexpected status {status} for {}",
            response.url()
        ),
        Ok(_) => bail!("remote cache delegated transfer returned unexpected status {status}"),
    }
}

/// The wait a `Retry-After` header asks for, counted from `now`: a number of
/// seconds, or an HTTP date, which asks for no wait once it has passed.
pub(crate) fn parse_retry_after(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    Some(date.duration_since(now).unwrap_or_default())
}

fn retry_after(error: &eyre::Report) -> Option<Duration> {
    error
        .chain()
        .find_map(|source| source.downcast_ref::<StatusError>())
        .and_then(|error| error.retry_after)
}

pub(crate) fn retry_delays(retries: i64) -> impl Iterator<Item = Duration> {
    [200u64, 1_000, 4_000, 15_000]
        .into_iter()
        .chain(std::iter::repeat(15_000))
        .map(Duration::from_millis)
        .map(|duration| {
            let factor = 0.5 + rand::random::<f64>() * 0.5;
            Duration::from_secs_f64(duration.as_secs_f64() * factor)
        })
        .take(retries.max(0) as usize)
}

/// hyper-util exposes DNS failures in the error chain as a `dns error` source,
/// but reqwest intentionally erases the concrete connector type. Match that
/// stable connector error label rather than platform-specific resolver text.
pub(crate) fn is_dns_error(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        if source.to_string() == "dns error" {
            return true;
        }
        current = source.source();
    }
    false
}

pub(crate) fn is_transient(error: &eyre::Report) -> bool {
    // An unavailable hostname is a deterministic configuration error. reqwest
    // categorizes it as a connect error, but retrying only delays the diagnosis.
    if is_dns_error(error.as_ref()) {
        return false;
    }
    error.chain().any(|source| {
        let error = source
            .downcast_ref::<reqwest::Error>()
            .or_else(|| {
                source
                    .downcast_ref::<StatusError>()
                    .map(|error| &error.source)
            })
            .or_else(|| {
                source
                    .downcast_ref::<std::io::Error>()
                    .and_then(std::io::Error::get_ref)
                    .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
            });
        let Some(error) = error else {
            return false;
        };
        if error.is_timeout() || error.is_connect() || error.is_body() {
            return true;
        }
        error.status().is_some_and(|status| {
            let status = status.as_u16();
            status == 408 || status == 429 || (500..600).contains(&status)
        })
    })
}

pub(crate) async fn retry_async<F, Fut, T>(
    verb: &str,
    url: &Url,
    retries: i64,
    mut operation: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut delays = retry_delays(retries);
    let mut retry_after_left = MAX_RETRY_AFTER;
    let mut attempt = 1;
    loop {
        let started_at = Instant::now();
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if is_transient(&error) => {
                let Some(backoff) = delays.next() else {
                    return Err(error);
                };
                let Some(delay) = retry_delay(backoff, retry_after(&error), &mut retry_after_left)
                else {
                    debug!(
                        "HTTP {verb} {url} attempt {attempt}: not retrying, the server asked for \
                         a longer wait than the {retry_after_left:?} left of {MAX_RETRY_AFTER:?}"
                    );
                    return Err(error);
                };
                warn!(
                    "HTTP {verb} {url} attempt {attempt} failed after {:?} (transient): {error}; retrying in {delay:?}",
                    started_at.elapsed()
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// The wait before the next attempt: `backoff`, or the server's `retry_after`
/// when that is longer. `None` when `retry_after` is more than `left`, which
/// is what remains of [`MAX_RETRY_AFTER`] and shrinks by each wait it allows.
pub(crate) fn retry_delay(
    backoff: Duration,
    retry_after: Option<Duration>,
    left: &mut Duration,
) -> Option<Duration> {
    let Some(retry_after) = retry_after else {
        return Some(backoff);
    };
    *left = left.checked_sub(retry_after)?;
    Some(backoff.max(retry_after))
}
