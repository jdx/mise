//! Bottle downloads from ghcr.io with sha256 verification.

use std::future::Future;
use std::path::PathBuf;

use futures_util::stream::{self, StreamExt, TryStreamExt};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use super::api::BottleFile;
use crate::http::HTTP;
use crate::result::Result;
use crate::ui::progress_report::SingleReport;

/// Drive independent downloads concurrently without requiring their futures
/// to be `Send + 'static`. The brew install path may also contain source
/// builds whose future is intentionally local to the package-manager driver.
pub(super) async fn concurrently<T, E, F>(
    futures: Vec<F>,
    limit: usize,
) -> std::result::Result<Vec<T>, E>
where
    F: Future<Output = std::result::Result<T, E>>,
{
    stream::iter(futures)
        .buffer_unordered(limit.max(1))
        .try_collect()
        .await
}

/// Download a bottle to the mise cache (or reuse a verified cached copy).
pub(super) async fn fetch_bottle(
    name: &str,
    pkg_version: &str,
    bottle: &BottleFile,
    pr: Option<&dyn SingleReport>,
) -> Result<PathBuf> {
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("bottles");
    let path = cache_dir.join(format!("{name}-{pkg_version}.tar.gz"));
    if path.exists() && crate::hash::ensure_checksum(&path, &bottle.sha256, None, "sha256").is_ok()
    {
        debug!("bottle cache hit: {}", path.display());
        return Ok(path);
    }
    if let Some(pr) = pr {
        pr.set_message(format!("download {name}-{pkg_version}.tar.gz"));
    }
    // ghcr.io allows anonymous pulls with this static bearer token
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer QQ=="));
    HTTP.download_file_with_headers(&bottle.url, &path, &headers, pr)
        .await?;
    if let Some(pr) = pr {
        pr.set_message("checksum".to_string());
    }
    crate::hash::ensure_checksum(&path, &bottle.sha256, pr, "sha256")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::sync::Barrier;

    use super::*;

    #[tokio::test]
    async fn concurrent_downloads_respect_the_limit() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(2));
        let futures = (0..4)
            .map(|i| {
                let active = active.clone();
                let max_active = max_active.clone();
                let barrier = barrier.clone();
                async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(current, Ordering::SeqCst);
                    barrier.wait().await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok::<_, ()>(i)
                }
            })
            .collect();

        let mut completed = concurrently(futures, 2).await.unwrap();
        completed.sort_unstable();
        assert_eq!(completed, vec![0, 1, 2, 3]);
        assert_eq!(max_active.load(Ordering::SeqCst), 2);
    }
}
