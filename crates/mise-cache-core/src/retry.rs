use super::*;

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
        let error = source.downcast_ref::<reqwest::Error>().or_else(|| {
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
    let mut attempt = 1;
    loop {
        let started_at = Instant::now();
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if is_transient(&error) => {
                let Some(delay) = delays.next() else {
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
