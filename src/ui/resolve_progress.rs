//! Network status belongs to the resolver future, not a thread or a global
//! current tool. The scope ends before installation, so artifact downloads
//! cannot overwrite install phases. Spawned resolver jobs enter their own scope.
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use super::progress_report::SingleReport;

tokio::task_local! {
    static REPORTER: Option<Arc<dyn SingleReport>>;
}

pub(crate) async fn scope<T>(
    reporter: Option<Box<dyn SingleReport>>,
    future: impl Future<Output = T>,
) -> T {
    let reporter = reporter.map(Arc::from);
    REPORTER.scope(reporter, future).await
}

/// Only the hostname is displayed: URLs can contain credentials, signed query
/// strings, and private repository paths. This describes the request, not its
/// DNS/TCP/TLS phase (which reqwest does not expose here).
pub(crate) fn fetching(url: &url::Url) {
    update(|| {
        format!(
            "resolving · fetching from {}",
            url.host_str().unwrap_or("remote")
        )
    });
}

pub(crate) fn retrying(url: &url::Url, attempt: usize, delay: Duration) {
    update(|| {
        format!(
            "resolving · retrying {} (attempt {}, {:.1}s backoff)",
            url.host_str().unwrap_or("remote"),
            attempt,
            delay.as_secs_f64(),
        )
    });
}

fn update(message: impl FnOnce() -> String) {
    let _ = REPORTER.try_with(|reporter| {
        if let Some(reporter) = reporter {
            reporter.set_message(message());
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Debug, Default)]
    pub(crate) struct RecordingReport(pub(crate) Arc<Mutex<Vec<String>>>);

    impl SingleReport for RecordingReport {
        fn set_message(&self, message: String) {
            self.0.lock().unwrap().push(message);
        }
    }

    #[tokio::test]
    async fn interleaved_resolvers_keep_their_hosts_and_hide_url_secrets() {
        let first = RecordingReport::default();
        let second = RecordingReport::default();
        let a = url::Url::parse("https://user:secret@one.example/private?token=secret").unwrap();
        let b = url::Url::parse("https://two.example/versions").unwrap();
        let barrier = tokio::sync::Barrier::new(2);
        tokio::join!(
            scope(Some(Box::new(first.clone())), async {
                fetching(&a);
                barrier.wait().await;
                retrying(&a, 2, Duration::from_secs(1));
            }),
            scope(Some(Box::new(second.clone())), async {
                fetching(&b);
                barrier.wait().await;
                fetching(&b);
            }),
        );
        assert_eq!(
            *first.0.lock().unwrap(),
            vec![
                "resolving · fetching from one.example",
                "resolving · retrying one.example (attempt 2, 1.0s backoff)",
            ]
        );
        assert_eq!(
            *second.0.lock().unwrap(),
            vec![
                "resolving · fetching from two.example",
                "resolving · fetching from two.example",
            ]
        );
        // An artifact request after resolution cannot alter a completed lookup.
        fetching(&a);
        assert_eq!(first.0.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn cancellation_restores_the_enclosing_scope() {
        let outer = RecordingReport::default();
        let inner = RecordingReport::default();
        let url = url::Url::parse("https://example.com/versions").unwrap();
        scope(Some(Box::new(outer.clone())), async {
            let lookup = scope(Some(Box::new(inner.clone())), async {
                fetching(&url);
                std::future::pending::<()>().await;
            });
            // Poll once and then drop the pending lookup, as a timeout would.
            let mut lookup = Box::pin(lookup);
            assert!(futures_util::poll!(&mut lookup).is_pending());
            drop(lookup);
            fetching(&url);
        })
        .await;
        assert_eq!(inner.0.lock().unwrap().len(), 1);
        assert_eq!(outer.0.lock().unwrap().len(), 1);
        fetching(&url);
        assert_eq!(outer.0.lock().unwrap().len(), 1);
    }
}
