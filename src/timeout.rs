use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::ui::time::format_duration;
use color_eyre::eyre::{Context, Result};

pub fn run_with_timeout<F, T>(f: F, timeout: Duration) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send,
    T: Send,
{
    let (tx, rx) = mpsc::channel();
    thread::scope(|s| {
        s.spawn(move || {
            let result = f();
            // If sending fails, the timeout has already been reached.
            let _ = tx.send(result);
        });
        rx.recv_timeout(timeout)
            .context(format!("timed out after {}", format_duration(timeout)))
    })?
}

pub async fn run_with_timeout_async<F, Fut, T>(f: F, timeout: Duration) -> Result<T>
where
    Fut: Future<Output = Result<T>> + Send,
    T: Send,
    F: FnOnce() -> Fut,
{
    match tokio::time::timeout(timeout, f()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(eyre::eyre!(format!(
            "timed out after {}",
            format_duration(timeout)
        ))),
    }
}
