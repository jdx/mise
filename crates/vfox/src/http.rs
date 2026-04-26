use reqwest::{Client, ClientBuilder, StatusCode};
use std::sync::LazyLock;
use std::time::Duration;

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    ClientBuilder::new()
        .user_agent(format!("vfox.rs/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("Failed to create reqwest client")
});

pub(crate) const HTTP_RETRY_ATTEMPTS: usize = 3;

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

pub(crate) fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(200 * (attempt as u64 + 1))
}

/// Retry an async operation that issues a request AND extracts the body.
/// Use for download/text/bytes flows where mid-stream failures (is_body()) need
/// to restart the whole request. Emits a warn! on a successful retry.
pub(crate) async fn retry_async<F, Fut, T>(
    url: &str,
    mut f: F,
) -> std::result::Result<T, reqwest::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, reqwest::Error>>,
{
    let mut last_err_msg: Option<String> = None;
    let mut last_err: Option<reqwest::Error> = None;
    for attempt in 0..HTTP_RETRY_ATTEMPTS {
        match f().await {
            Ok(value) => {
                if let Some(prev) = last_err_msg {
                    log::warn!(
                        "HTTP {} succeeded on attempt {} after transient error: {}",
                        url,
                        attempt + 1,
                        prev
                    );
                }
                return Ok(value);
            }
            Err(err) => {
                if !is_transient(&err) || attempt + 1 >= HTTP_RETRY_ATTEMPTS {
                    return Err(err);
                }
                let delay = retry_delay(attempt);
                log::debug!(
                    "HTTP {} attempt {} failed (transient): {}; retrying in {:?}",
                    url,
                    attempt + 1,
                    err,
                    delay
                );
                last_err_msg = Some(err.to_string());
                last_err = Some(err);
                tokio::time::sleep(delay).await;
            }
        }
    }
    Err(last_err.expect("retry loop should always return"))
}
