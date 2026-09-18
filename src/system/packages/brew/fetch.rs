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
/// Run `futures` with at most `limit` in flight and return their outputs in the
/// order the futures were given, not the order they finished.
///
/// `concurrently` yields on completion, which is the right default for work
/// whose results are interchangeable. Callers that map results back onto a
/// caller-supplied list need the original order restored, and doing that by
/// hand at each call site is how an off-by-one silently attributes one
/// package's result to another.
pub(super) async fn concurrently_in_order<F, T>(futures: Vec<F>, limit: usize) -> Vec<T>
where
    F: Future<Output = T>,
{
    let indexed: Vec<_> = futures
        .into_iter()
        .enumerate()
        .map(|(idx, fut)| async move { (idx, fut.await) })
        .collect();
    let mut slots: Vec<Option<T>> = (0..indexed.len()).map(|_| None).collect();
    let mut running = concurrently(indexed, limit);
    while let Some((idx, output)) = running.next().await {
        slots[idx] = Some(output);
    }
    slots
        .into_iter()
        .map(|slot| slot.expect("every future yielded exactly one output"))
        .collect()
}

/// Like [`concurrently_in_order`], but for fallible work: stop admitting new
/// jobs as soon as one fails.
///
/// A serial loop stops at its first failure, so nothing after it is ever
/// requested. Collecting every result before reporting the error would give
/// that up, and a single endpoint burning its timeout and retry budget would
/// hold up the whole run. Cancelling pending work on the first failure, while
/// still draining what is already in flight, is what the bottle path does.
///
/// The reported failure is the earliest by position among the jobs that
/// actually ran. It is not always the one a fully serial pass would have
/// reported, since a lower-positioned job may have been cancelled before it
/// started, but it does not depend on which request lost a race either.
pub(super) async fn concurrently_results_in_order<F, T, E>(
    futures: Vec<F>,
    limit: usize,
) -> std::result::Result<Vec<T>, E>
where
    F: Future<Output = std::result::Result<T, E>>,
{
    let indexed: Vec<_> = futures
        .into_iter()
        .enumerate()
        .map(|(idx, fut)| async move { (idx, fut.await) })
        .collect();
    let mut slots: Vec<Option<T>> = (0..indexed.len()).map(|_| None).collect();
    let mut failure: Option<(usize, E)> = None;
    let mut running = concurrently(indexed, limit);
    while let Some((idx, result)) = running.next().await {
        match result {
            Ok(value) => slots[idx] = Some(value),
            Err(err) => {
                running.cancel_pending();
                let earlier = match &failure {
                    None => true,
                    Some((previous, _)) => idx < *previous,
                };
                if earlier {
                    failure = Some((idx, err));
                }
            }
        }
    }
    if let Some((_, err)) = failure {
        return Err(err);
    }
    // Only reachable when nothing failed, so nothing was cancelled and every
    // slot was filled.
    Ok(slots
        .into_iter()
        .map(|slot| slot.expect("every future yielded exactly one output"))
        .collect())
}

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
    /// Completion order must not leak into the result order: these futures
    /// finish in reverse, so a version that collected as they landed would
    /// return the list backwards.
    #[tokio::test]
    async fn concurrently_in_order_returns_input_order() {
        let futures: Vec<_> = (0..16usize)
            .map(|i| async move {
                tokio::time::sleep(std::time::Duration::from_millis((16 - i) as u64 * 5)).await;
                i
            })
            .collect();
        let out = super::concurrently_in_order(futures, 8).await;
        assert_eq!(out, (0..16usize).collect::<Vec<_>>());
    }

    /// The bound is what makes it concurrent rather than serial, so the order
    /// guarantee has to survive every limit, including a limit of one and a
    /// limit larger than the job count.
    #[tokio::test]
    async fn concurrently_in_order_holds_at_every_limit() {
        for limit in [1usize, 2, 5, 32] {
            let futures: Vec<_> = (0..10usize)
                .map(|i| async move {
                    tokio::time::sleep(std::time::Duration::from_millis((10 - i) as u64 * 3)).await;
                    i * 2
                })
                .collect();
            let out = super::concurrently_in_order(futures, limit).await;
            assert_eq!(
                out,
                (0..10usize).map(|i| i * 2).collect::<Vec<_>>(),
                "limit {limit}"
            );
        }
    }

    /// A failure must not cost the whole list: later jobs are never admitted.
    #[tokio::test]
    async fn concurrently_results_in_order_stops_admitting_work_after_a_failure() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let started = Arc::new(AtomicUsize::new(0));
        let futures: Vec<_> = (0..50usize)
            .map(|i| {
                let started = Arc::clone(&started);
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    if i == 0 {
                        return Err("boom");
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    Ok(i)
                }
            })
            .collect();

        assert_eq!(
            super::concurrently_results_in_order(futures, 4).await,
            Err("boom")
        );
        let started = started.load(Ordering::SeqCst);
        assert!(
            started < 50,
            "admitted {started} of 50 jobs after a failure"
        );
    }

    /// Which failure is reported must not depend on which one lost the race.
    #[tokio::test]
    async fn concurrently_results_in_order_prefers_the_earliest_failure() {
        let futures: Vec<_> = (0..4usize)
            .map(|i| async move {
                // The earlier failure resolves last.
                if i == 1 {
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    return Err(1usize);
                }
                if i == 3 {
                    return Err(3usize);
                }
                Ok(i)
            })
            .collect();
        assert_eq!(
            super::concurrently_results_in_order(futures, 4).await,
            Err(1usize)
        );
    }

    /// The ordering guarantee has to survive the fallible path too, not just
    /// the infallible one.
    #[tokio::test]
    async fn concurrently_results_in_order_returns_input_order() {
        let futures: Vec<_> = (0..12usize)
            .map(|i| async move {
                tokio::time::sleep(std::time::Duration::from_millis((12 - i) as u64 * 4)).await;
                Ok::<usize, ()>(i)
            })
            .collect();
        assert_eq!(
            super::concurrently_results_in_order(futures, 6).await,
            Ok((0..12usize).collect::<Vec<_>>())
        );
    }

    /// Nothing to run is not an error, and must not trip the slot bookkeeping.
    #[tokio::test]
    async fn concurrently_in_order_handles_empty_input() {
        let futures: Vec<std::future::Ready<usize>> = Vec::new();
        assert!(super::concurrently_in_order(futures, 4).await.is_empty());
    }

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
