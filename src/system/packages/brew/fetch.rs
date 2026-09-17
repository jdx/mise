//! Bottle downloads from ghcr.io with sha256 verification.

use std::future::Future;
use std::path::PathBuf;

use futures_util::stream::{FuturesUnordered, StreamExt};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use super::api::BottleFile;
use crate::http::HTTP;
use crate::result::Result;
use crate::ui::progress_report::SingleReport;

/// Bounded concurrent jobs that can discard work not yet admitted to the
/// active set while still draining active jobs after a failure.
pub(super) struct ConcurrentJobs<F> {
    pending: std::vec::IntoIter<F>,
    active: FuturesUnordered<F>,
    limit: usize,
}

impl<F> ConcurrentJobs<F>
where
    F: Future,
{
    fn fill(&mut self) {
        while self.active.len() < self.limit {
            let Some(job) = self.pending.next() else {
                break;
            };
            self.active.push(job);
        }
    }

    pub(super) async fn next(&mut self) -> Option<F::Output> {
        self.fill();
        self.active.next().await
    }

    pub(super) fn cancel_pending(&mut self) {
        self.pending = Vec::new().into_iter();
    }
}

/// Drive independent jobs concurrently and yield each result as soon as it
/// completes, without requiring the futures to be `Send + 'static`.
pub(super) fn concurrently<F>(futures: Vec<F>, limit: usize) -> ConcurrentJobs<F>
where
    F: Future,
{
    ConcurrentJobs {
        pending: futures.into_iter(),
        active: FuturesUnordered::new(),
        limit: limit.max(1),
    }
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
    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn concurrent_downloads_respect_the_limit() {
        let started = Arc::new(AtomicUsize::new(0));
        let futures = (0..4)
            .map(|_| {
                let started = started.clone();
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    pending::<()>().await
                }
            })
            .collect();

        assert!(
            timeout(Duration::from_millis(10), async {
                let mut jobs = concurrently(futures, 2);
                while jobs.next().await.is_some() {}
            },)
            .await
            .is_err()
        );
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrent_downloads_collect_results() {
        let futures = (0..4).map(|i| async move { i }).collect();

        let mut jobs = concurrently(futures, 2);
        let mut completed = Vec::new();
        while let Some(result) = jobs.next().await {
            completed.push(result);
        }
        completed.sort_unstable();
        assert_eq!(completed, vec![0, 1, 2, 3]);
    }

    #[tokio::test]
    async fn concurrent_jobs_yield_before_every_job_finishes() {
        let futures = (0..2)
            .map(|i| async move {
                if i == 1 {
                    pending::<()>().await;
                }
                i
            })
            .collect();
        let mut jobs = concurrently(futures, 2);

        assert_eq!(
            timeout(Duration::from_millis(10), jobs.next()).await,
            Ok(Some(0))
        );
        assert!(
            timeout(Duration::from_millis(10), jobs.next())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn concurrent_jobs_can_drop_pending_work_and_drain_active_work() {
        let started = Arc::new(AtomicUsize::new(0));
        let futures = (0..5)
            .map(|i| {
                let started = started.clone();
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    i
                }
            })
            .collect();
        let mut jobs = concurrently(futures, 2);

        let first = jobs.next().await.unwrap();
        jobs.cancel_pending();
        let second = jobs.next().await.unwrap();
        assert_eq!(jobs.next().await, None);
        let mut completed = vec![first, second];
        completed.sort_unstable();
        assert_eq!(completed, vec![0, 1]);
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }
}
