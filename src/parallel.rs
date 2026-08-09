use crate::Result;
use crate::config::Settings;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn parallel<T, F, Fut, U>(input: Vec<T>, f: F) -> Result<Vec<U>>
where
    T: Send + 'static,
    U: Send + 'static,
    F: Fn(T) -> Fut + Send + Copy + 'static,
    Fut: Future<Output = Result<U>> + Send + 'static,
{
    let semaphore = Arc::new(Semaphore::new(crate::jobs::normalize(Settings::get().jobs)));
    let mut jset = JoinSet::new();
    let mut results = input.iter().map(|_| None).collect::<Vec<_>>();
    for item in input.into_iter().enumerate() {
        let semaphore = semaphore.clone();
        let permit = semaphore.acquire_owned().await?;
        jset.spawn(async move {
            let _permit = permit;
            let res = f(item.1).await?;
            Ok((item.0, res))
        });
    }
    while let Some(result) = jset.join_next().await {
        let err: eyre::Report = match result {
            Ok(Ok((i, result))) => {
                results[i] = Some(result);
                continue;
            }
            Ok(Err(e)) => e,
            Err(e) => e.into(),
        };
        jset.abort_all();
        // Drain remaining tasks - don't use join_all() as it panics on cancelled tasks
        while jset.join_next().await.is_some() {}
        return Err(err);
    }
    Ok(results.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::test;

    #[test]
    async fn test_parallel() {
        let input = vec![1, 2, 3, 4, 5];
        let results = parallel(input, |x| async move { Ok(x * 2) }).await.unwrap();
        assert_eq!(results, vec![2, 4, 6, 8, 10]);
    }

    /// Yield until `flag` is set, giving up after a bounded number of turns.
    ///
    /// The bound is what keeps these tests independent of the `jobs` setting. A permit is taken
    /// *before* `spawn`, so with a budget of one — which `raw = true` forces, see
    /// `Settings::try_get` — a task that waits unconditionally on a sibling stops that sibling
    /// from ever being spawned, and the test hangs. Giving up early only weakens the interleaving;
    /// it never changes what is asserted.
    ///
    /// Shared state is a `static` rather than a captured handle because `F` is bound by `Copy`,
    /// which a closure holding an `Arc` would not satisfy.
    async fn yield_until(flag: &AtomicBool) {
        for _ in 0..1_000 {
            if flag.load(Ordering::SeqCst) {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    static VICTIM_STARTED: AtomicBool = AtomicBool::new(false);
    static VICTIM_FINISHED: AtomicBool = AtomicBool::new(false);

    /// One task failing must abort its unfinished siblings and return the error — not panic.
    ///
    /// `JoinSet::join_all()` panics on any task that was cancelled rather than completed, which is
    /// why the drain above uses `join_next()` (#7280). The panic it replaced reached users as
    /// `task N was cancelled` out of `join_set.rs` (discussions #5263, #5369, #5414), and nothing
    /// pinned the replacement.
    ///
    /// The failing task is item 0 so that it is spawned first and can never be starved of a
    /// permit by the sibling. The sibling parks for far longer than the test takes, so it is
    /// guaranteed to be unfinished when the drain reaches it — cancelled before its first poll
    /// counts just the same, which is what makes this hold at any `jobs` budget.
    #[test]
    async fn a_failure_aborts_unfinished_siblings_without_panicking() {
        let err = parallel(vec![0, 1], |i| async move {
            if i == 0 {
                // the trigger: nudge the sibling into actually running first, so the usual case is
                // a cancelled *in-flight* task rather than one aborted before it was ever polled
                yield_until(&VICTIM_STARTED).await;
                Err(eyre::eyre!("task {i} failed"))
            } else {
                // the victim: finishing this would mean the abort never happened
                VICTIM_STARTED.store(true, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(10)).await;
                VICTIM_FINISHED.store(true, Ordering::SeqCst);
                Ok(i)
            }
        })
        .await
        .unwrap_err();

        assert_eq!(format!("{err:#}"), "task 0 failed");
        assert!(
            !VICTIM_FINISHED.load(Ordering::SeqCst),
            "sibling ran to completion, so it was never aborted"
        );
    }

    static FAST_TASK_DONE: AtomicBool = AtomicBool::new(false);

    /// Results are indexed by input position, not by completion order.
    ///
    /// `src/backend/conda.rs` zips its input vector against this function's output, which is only
    /// correct while that holds. `test_parallel` cannot catch a regression here — `x * 2` over a
    /// sorted input looks the same either way.
    ///
    /// The completion order is forced by a handshake rather than a timer: item 1 sets the flag as
    /// its last act, and item 0 does not return until it observes that.
    #[test]
    async fn results_follow_input_order_not_completion_order() {
        let results = parallel(vec![0, 1], |i| async move {
            if i == 0 {
                // first in the input, last to finish
                yield_until(&FAST_TASK_DONE).await;
            } else {
                FAST_TASK_DONE.store(true, Ordering::SeqCst);
            }
            Ok(format!("item-{i}"))
        })
        .await
        .unwrap();

        assert_eq!(results, vec!["item-0", "item-1"]);
    }
}
