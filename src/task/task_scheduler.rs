use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::task::{Deps, Task};
use eyre::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Mutex, Semaphore, mpsc};
use tokio::task::JoinSet;

#[cfg(unix)]
use nix::sys::signal::SIGTERM;

pub type SchedMsg = (Task, Arc<Mutex<Deps>>);

/// Schedules and executes tasks with concurrency control
pub struct Scheduler {
    pub semaphore: Arc<Semaphore>,
    pub jset: Arc<Mutex<JoinSet<Result<()>>>>,
    pub sched_tx: Arc<mpsc::UnboundedSender<SchedMsg>>,
    pub sched_rx: Option<mpsc::UnboundedReceiver<SchedMsg>>,
    pub in_flight: Arc<AtomicUsize>,
}

impl Scheduler {
    pub fn new(jobs: usize) -> Self {
        let (sched_tx, sched_rx) = mpsc::unbounded_channel::<SchedMsg>();
        Self {
            semaphore: Arc::new(Semaphore::new(jobs)),
            jset: Arc::new(Mutex::new(JoinSet::new())),
            sched_tx: Arc::new(sched_tx),
            sched_rx: Some(sched_rx),
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Take ownership of the receiver (can only be called once)
    pub fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<SchedMsg>> {
        self.sched_rx.take()
    }

    /// Get the sender for scheduling tasks
    pub fn sender(&self) -> Arc<mpsc::UnboundedSender<SchedMsg>> {
        self.sched_tx.clone()
    }

    /// Wait for all spawned tasks to complete
    pub async fn join_all(&self, continue_on_error: bool) -> Result<()> {
        while let Some(result) = self.jset.lock().await.join_next().await {
            if result.is_ok() || continue_on_error {
                continue;
            }
            #[cfg(unix)]
            CmdLineRunner::kill_all(SIGTERM);
            #[cfg(windows)]
            CmdLineRunner::kill_all();
            break;
        }
        Ok(())
    }

    /// Create a spawn context
    pub fn spawn_context(&self, config: Arc<Config>) -> SpawnContext {
        SpawnContext {
            semaphore: self.semaphore.clone(),
            config,
            sched_tx: self.sched_tx.clone(),
            jset: self.jset.clone(),
            in_flight: self.in_flight.clone(),
        }
    }

    /// Get the in-flight task count
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }
}

/// Context passed to spawned tasks
#[derive(Clone)]
pub struct SpawnContext {
    pub semaphore: Arc<Semaphore>,
    pub config: Arc<Config>,
    pub sched_tx: Arc<mpsc::UnboundedSender<SchedMsg>>,
    pub jset: Arc<Mutex<JoinSet<Result<()>>>>,
    pub in_flight: Arc<AtomicUsize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_new() {
        let scheduler = Scheduler::new(4);
        // Verify basic initialization
        assert_eq!(
            scheduler.in_flight_count(),
            0,
            "in_flight should start at 0"
        );
    }

    #[tokio::test]
    async fn test_scheduler_sender() {
        let scheduler = Scheduler::new(4);
        let sender = scheduler.sender();
        // Verify we can send messages
        assert!(!sender.is_closed(), "sender should not be closed");
    }

    #[tokio::test]
    async fn test_spawn_context_clone() {
        let scheduler = Scheduler::new(4);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config.clone());
        let ctx2 = ctx.clone();
        // Verify cloning works
        assert!(Arc::ptr_eq(&ctx.config, &ctx2.config));
    }

    #[tokio::test]
    async fn test_scheduler_receiver_take() {
        let mut scheduler = Scheduler::new(4);
        let rx = scheduler.take_receiver();
        assert!(rx.is_some(), "should be able to take receiver once");
        let rx2 = scheduler.take_receiver();
        assert!(rx2.is_none(), "should not be able to take receiver twice");
    }
}
