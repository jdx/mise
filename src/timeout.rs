use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::ui::time::format_duration;
use color_eyre::eyre::{Report, Result};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy)]
pub struct TimeoutError {
    pub duration: Duration,
}

impl Display for TimeoutError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "timed out after {}", format_duration(self.duration))
    }
}

impl std::error::Error for TimeoutError {}

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
        let recv: Result<T> = rx
            .recv_timeout(timeout)
            .map_err(|_| Report::from(TimeoutError { duration: timeout }))?;
        recv
    })
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
        Err(_) => Err(TimeoutError { duration: timeout }.into()),
    }
}
