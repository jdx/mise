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

    /// Pump dependency graph leaves into the scheduler
    ///
    /// Forwards initial leaves synchronously, then spawns an async task to forward
    /// remaining leaves as they become available. Returns a watch receiver that signals
    /// when all dependencies are complete.
    pub async fn pump_deps(&self, deps: Arc<Mutex<Deps>>) -> tokio::sync::watch::Receiver<bool> {
        let (main_done_tx, main_done_rx) = tokio::sync::watch::channel(false);
        let sched_tx = self.sched_tx.clone();
        let deps_clone = deps.clone();

        // Forward initial leaves synchronously
        {
            let mut rx = deps_clone.lock().await.subscribe();
            loop {
                match rx.try_recv() {
                    Ok(Some(task)) => {
                        trace!(
                            "main deps initial leaf: {} {}",
                            task.name,
                            task.args.join(" ")
                        );
                        let _ = sched_tx.send((task, deps_clone.clone()));
                    }
                    Ok(None) => {
                        trace!("main deps initial done");
                        break;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {
                        break;
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        break;
                    }
                }
            }
        }

        // Forward remaining leaves asynchronously
        tokio::spawn(async move {
            let mut rx = deps_clone.lock().await.subscribe();
            while let Some(msg) = rx.recv().await {
                match msg {
                    Some(task) => {
                        trace!(
                            "main deps leaf scheduled: {} {}",
                            task.name,
                            task.args.join(" ")
                        );
                        let _ = sched_tx.send((task, deps_clone.clone()));
                    }
                    None => {
                        trace!("main deps completed");
                        let _ = main_done_tx.send(true);
                        break;
                    }
                }
            }
        });

        main_done_rx
    }

    /// Run the scheduler loop, draining tasks and spawning them via the callback
    ///
    /// The loop continues until:
    /// - main_done signal is received AND
    /// - no tasks are in-flight AND
    /// - no tasks were recently drained
    ///
    /// Or if should_stop returns true (for early exit due to failures)
    pub async fn run_loop<F, Fut>(
        &mut self,
        main_done_rx: &mut tokio::sync::watch::Receiver<bool>,
        main_deps: Arc<Mutex<Deps>>,
        should_stop: impl Fn() -> bool,
        continue_on_error: bool,
        mut spawn_job: F,
    ) -> Result<()>
    where
        F: FnMut(Task, Arc<Mutex<Deps>>) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut sched_rx = self.take_receiver().expect("receiver already taken");
        let mut failure_cleanup_done = false;

        loop {
            // Drain ready tasks without awaiting
            let mut drained_any = false;
            loop {
                match sched_rx.try_recv() {
                    Ok((task, deps_for_remove)) => {
                        drained_any = true;
                        trace!("scheduler received: {} {}", task.name, task.args.join(" "));
                        if should_stop() && !continue_on_error {
                            // Still allow post-dep (cleanup) tasks to run on failure,
                            // but only if their parent was actually started
                            let mut deps = deps_for_remove.lock().await;
                            if !deps.is_runnable_post_dep(&task) {
                                deps.remove(&task);
                                continue;
                            }
                            drop(deps);
                        }
                        spawn_job(task, deps_for_remove).await?;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            }

            // Check if we should stop early due to failure (run cleanup only once)
            if should_stop() && !continue_on_error && !failure_cleanup_done {
                failure_cleanup_done = true;
                trace!("scheduler: stopping early due to failure, cleaning up non-post-dep tasks");
                // Clean up tasks that shouldn't run: non-post-deps and post-deps whose
                // parent was never started. Use batch removal so intermediate emit_leaves
                // calls don't schedule post-deps of never-started tasks.
                let mut deps = main_deps.lock().await;
                let tasks_to_remove: Vec<Task> = deps
                    .all()
                    .filter(|t| !deps.is_runnable_post_dep(t))
                    .cloned()
                    .collect();
                deps.remove_batch(&tasks_to_remove);
                if deps.is_empty() {
                    drop(deps);
                    break;
                }
                drop(deps);
                // Don't break — continue loop to process remaining post-dep tasks
            }

            // Exit if main deps finished and nothing is running/queued
            if *main_done_rx.borrow() && self.in_flight_count() == 0 && !drained_any {
                trace!("scheduler drain complete; exiting loop");
                break;
            }

            // Await either new work or main_done change
            tokio::select! {
                m = sched_rx.recv() => {
                    if let Some((task, deps_for_remove)) = m {
                        trace!("scheduler received: {} {}", task.name, task.args.join(" "));
                        if should_stop() && !continue_on_error {
                            let mut deps = deps_for_remove.lock().await;
                            if !deps.is_runnable_post_dep(&task) {
                                deps.remove(&task);
                                continue;
                            }
                            drop(deps);
                        }
                        spawn_job(task, deps_for_remove).await?;
                    } else {
                        // channel closed; rely on main_done/in_flight to exit soon
                    }
                }
                _ = main_done_rx.changed() => {
                    trace!("main_done changed: {}", *main_done_rx.borrow());
                }
            }
        }

        Ok(())
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
