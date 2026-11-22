//! A count of problems that are logged at debug level instead of warned about one
//! by one, so the caller can summarize them in a single line. mise uses it while it
//! scans every tracked config for something the user did not ask about directly.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

tokio::task_local! {
    static COUNT: Arc<AtomicUsize>;
}

/// Runs `future` with problems counted rather than shown. Returns its result and
/// how many problems there were.
pub async fn scope<T>(future: impl Future<Output = T>) -> (T, usize) {
    let count = Arc::new(AtomicUsize::new(0));
    let out = COUNT.scope(count.clone(), future).await;
    (out, count.load(Ordering::Relaxed))
}

/// Whether problems are being counted rather than shown. Counts the problem when
/// they are.
pub fn count() -> bool {
    COUNT
        .try_with(|count| count.fetch_add(1, Ordering::Relaxed))
        .is_ok()
}

/// Wraps `future`, about to be spawned as a task of its own, so that it counts
/// into the scope of the task spawning it: task-locals do not pass to a spawned
/// task, so its problems would otherwise be warned about.
pub fn inherit<F: Future>(future: F) -> impl Future<Output = F::Output> {
    let count = COUNT.try_with(Arc::clone).ok();
    async move {
        match count {
            Some(count) => COUNT.scope(count, future).await,
            None => future.await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn counts_problems_in_spawned_tasks() {
        let ((), count) = scope(async {
            assert!(count());
            tokio::spawn(inherit(async { assert!(count()) }))
                .await
                .unwrap();
            crate::parallel::parallel(vec![1, 2], |_| async { Ok(count()) })
                .await
                .unwrap();
        })
        .await;
        assert_eq!(count, 4);
        assert!(!self::count(), "outside the scope problems are shown");
    }
}
