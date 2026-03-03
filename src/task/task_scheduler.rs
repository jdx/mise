use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::task::task_helpers::TaskOrderKey;
use crate::task::task_scheduler_policy::{
    AdmissionAction, AdmissionProbe, AdmissionResources, BarrierState, PermitState,
    SchedulerSnapshot, SpawnClass, decide_admission,
};
#[cfg(test)]
use crate::task::task_scheduler_policy::{SchedulerDecision, decide_ready};
use crate::task::{Deps, Task};
use eyre::Result;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinSet;

#[cfg(unix)]
use nix::sys::signal::SIGTERM;

pub type SchedMsg = (Task, Arc<Mutex<Deps>>);

#[derive(Debug, Clone)]
struct PendingInteractive {
    seq: u64,
    owner: Option<u64>,
    order_key: TaskOrderKey,
}

pub struct InteractiveGateGuard {
    owner: Arc<AtomicU64>,
    owner_id: u64,
    notify: Arc<Notify>,
}

impl InteractiveGateGuard {
    fn new(owner: Arc<AtomicU64>, owner_id: u64, notify: Arc<Notify>) -> Self {
        Self {
            owner,
            owner_id,
            notify,
        }
    }
}

impl Drop for InteractiveGateGuard {
    fn drop(&mut self) {
        let _ = self
            .owner
            .compare_exchange(self.owner_id, 0, Ordering::SeqCst, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

/// Schedules and executes tasks with concurrency control
pub struct Scheduler {
    pub semaphore: Arc<Semaphore>,
    pub jset: Arc<Mutex<JoinSet<Result<()>>>>,
    pub sched_tx: Arc<mpsc::UnboundedSender<SchedMsg>>,
    pub sched_rx: Option<mpsc::UnboundedReceiver<SchedMsg>>,
    pub in_flight: Arc<AtomicUsize>,
    pub runtime_in_flight: Arc<AtomicUsize>,
    pub runtime_in_flight_by_owner: Arc<StdMutex<HashMap<u64, usize>>>,
    pub interactive_owner: Arc<AtomicU64>,
    pub next_interactive_owner: Arc<AtomicU64>,
    pub next_scheduler_seq: Arc<AtomicU64>,
    pending_interactive_seqs: Arc<StdMutex<VecDeque<PendingInteractive>>>,
    pub pending_permit_seqs: Arc<StdMutex<VecDeque<u64>>>,
    pub stop_requested: Arc<AtomicBool>,
    pub state_notify: Arc<Notify>,
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
            runtime_in_flight: Arc::new(AtomicUsize::new(0)),
            runtime_in_flight_by_owner: Arc::new(StdMutex::new(HashMap::new())),
            interactive_owner: Arc::new(AtomicU64::new(0)),
            next_interactive_owner: Arc::new(AtomicU64::new(1)),
            next_scheduler_seq: Arc::new(AtomicU64::new(1)),
            pending_interactive_seqs: Arc::new(StdMutex::new(VecDeque::new())),
            pending_permit_seqs: Arc::new(StdMutex::new(VecDeque::new())),
            stop_requested: Arc::new(AtomicBool::new(false)),
            state_notify: Arc::new(Notify::new()),
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
            runtime_in_flight: self.runtime_in_flight.clone(),
            runtime_in_flight_by_owner: self.runtime_in_flight_by_owner.clone(),
            interactive_owner: self.interactive_owner.clone(),
            next_interactive_owner: self.next_interactive_owner.clone(),
            next_scheduler_seq: self.next_scheduler_seq.clone(),
            pending_interactive_seqs: self.pending_interactive_seqs.clone(),
            pending_permit_seqs: self.pending_permit_seqs.clone(),
            stop_requested: self.stop_requested.clone(),
            state_notify: self.state_notify.clone(),
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
    pub runtime_in_flight: Arc<AtomicUsize>,
    pub runtime_in_flight_by_owner: Arc<StdMutex<HashMap<u64, usize>>>,
    pub interactive_owner: Arc<AtomicU64>,
    pub next_interactive_owner: Arc<AtomicU64>,
    pub next_scheduler_seq: Arc<AtomicU64>,
    pending_interactive_seqs: Arc<StdMutex<VecDeque<PendingInteractive>>>,
    pub pending_permit_seqs: Arc<StdMutex<VecDeque<u64>>>,
    pub stop_requested: Arc<AtomicBool>,
    pub state_notify: Arc<Notify>,
}

pub struct AdmissionTicket {
    pub permit: Option<OwnedSemaphorePermit>,
    pub interactive_guard: Option<InteractiveGateGuard>,
    pub interactive_owner: Option<u64>,
}

pub enum AdmissionOutcome {
    Drop,
    Start(AdmissionTicket),
}

enum PermitLease {
    Missing,
    Held(OwnedSemaphorePermit),
}

impl PermitLease {
    fn state(&self) -> PermitState {
        match self {
            Self::Missing => PermitState::Missing,
            Self::Held(_) => PermitState::Held,
        }
    }

    fn is_held(&self) -> bool {
        matches!(self, Self::Held(_))
    }

    fn into_option(self) -> Option<OwnedSemaphorePermit> {
        match self {
            Self::Missing => None,
            Self::Held(permit) => Some(permit),
        }
    }
}

enum BarrierLease {
    Missing,
    Held(InteractiveGateGuard),
}

impl BarrierLease {
    fn state(&self) -> BarrierState {
        match self {
            Self::Missing => BarrierState::Missing,
            Self::Held(_) => BarrierState::Held,
        }
    }

    fn is_held(&self) -> bool {
        matches!(self, Self::Held(_))
    }

    fn into_option(self) -> Option<InteractiveGateGuard> {
        match self {
            Self::Missing => None,
            Self::Held(guard) => Some(guard),
        }
    }
}

impl SpawnContext {
    /// Capture atomics into a value object consumed by the pure policy engine.
    fn snapshot(&self, continue_on_error: bool, probe_owner: Option<u64>) -> SchedulerSnapshot {
        let active_owner = self.active_interactive_owner();
        let pending_interactive = self.pending_interactive_seqs.lock().unwrap();
        let pending_interactive_head_seq = pending_interactive.front().map(|p| p.seq);
        let pending_interactive_min_seq = pending_interactive.iter().map(|p| p.seq).min();
        let pending_interactive_owners: HashSet<u64> =
            pending_interactive.iter().filter_map(|p| p.owner).collect();
        drop(pending_interactive);
        let pending_permit_seq = self.pending_permit_seqs.lock().unwrap().front().copied();
        let runtime_by_owner = self.runtime_in_flight_by_owner.lock().unwrap();
        let runtime_in_flight_for_owner = probe_owner
            .and_then(|owner| runtime_by_owner.get(&owner).copied())
            .unwrap_or(0);
        let foreign_runtime_owners_without_pending_interactive = runtime_by_owner
            .iter()
            .filter(|(owner, count)| {
                **count > 0
                    && Some(**owner) != probe_owner
                    && !pending_interactive_owners.contains(owner)
            })
            .count();
        SchedulerSnapshot {
            continue_on_error,
            stopping: self.stop_requested.load(Ordering::SeqCst),
            runtime_in_flight: self.runtime_in_flight.load(Ordering::SeqCst),
            runtime_in_flight_for_owner,
            foreign_runtime_owners_without_pending_interactive,
            active_interactive_owner: active_owner,
            pending_interactive_head_seq,
            pending_interactive_min_seq,
            pending_permit_seq,
            permits_available: self.semaphore.available_permits(),
        }
    }

    pub fn active_interactive_owner(&self) -> Option<u64> {
        let owner = self.interactive_owner.load(Ordering::SeqCst);
        (owner != 0).then_some(owner)
    }

    pub fn assign_scheduler_seq(&self, seq: Option<u64>) -> u64 {
        seq.unwrap_or_else(|| self.next_scheduler_seq.fetch_add(1, Ordering::SeqCst))
    }

    pub fn allocate_owner_id(&self, owner: Option<u64>) -> Option<u64> {
        owner.or_else(|| Some(self.next_interactive_owner.fetch_add(1, Ordering::SeqCst)))
    }

    pub fn assign_interactive_owner(&self, class: SpawnClass, owner: Option<u64>) -> Option<u64> {
        if class.requires_interactive_barrier() {
            self.allocate_owner_id(owner)
        } else {
            owner
        }
    }

    pub fn enqueue_pending_interactive(
        &self,
        seq: u64,
        owner: Option<u64>,
        order_key: TaskOrderKey,
    ) {
        let mut queue = self.pending_interactive_seqs.lock().unwrap();
        let item = PendingInteractive {
            seq,
            owner,
            order_key,
        };
        let pos = queue.iter().position(|p| {
            p.order_key > item.order_key || (p.order_key == item.order_key && p.seq > item.seq)
        });
        match pos {
            Some(idx) => queue.insert(idx, item),
            None => queue.push_back(item),
        }
        self.state_notify.notify_waiters();
    }

    pub fn enqueue_pending_permit(&self, seq: u64) {
        self.pending_permit_seqs.lock().unwrap().push_back(seq);
        self.state_notify.notify_waiters();
    }

    fn release_pending_interactive_if_head(&self, seq: u64) {
        let mut queue = self.pending_interactive_seqs.lock().unwrap();
        if queue.front().map(|p| p.seq) == Some(seq) {
            queue.pop_front();
            self.state_notify.notify_waiters();
        }
    }

    fn release_pending_interactive(&self, seq: u64) {
        let mut queue = self.pending_interactive_seqs.lock().unwrap();
        if let Some(pos) = queue.iter().position(|p| p.seq == seq) {
            queue.remove(pos);
            self.state_notify.notify_waiters();
        }
    }

    fn release_pending_permit(&self, seq: u64) {
        let mut queue = self.pending_permit_seqs.lock().unwrap();
        if let Some(pos) = queue.iter().position(|s| *s == seq) {
            queue.remove(pos);
            self.state_notify.notify_waiters();
        }
    }

    fn release_queued_admission(&self, class: SpawnClass, seq: u64) {
        if class.is_runtime() {
            self.release_pending_permit(seq);
        }
        if class.requires_interactive_barrier() {
            self.release_pending_interactive(seq);
        }
    }

    fn admission_resources(
        class: SpawnClass,
        barrier: BarrierState,
        permit: PermitState,
    ) -> AdmissionResources {
        match class {
            SpawnClass::NonRuntime => AdmissionResources::NonRuntime,
            SpawnClass::Runtime => AdmissionResources::Runtime { permit },
            SpawnClass::InteractiveRuntime => {
                AdmissionResources::InteractiveRuntime { barrier, permit }
            }
        }
    }

    pub fn admission_action(
        &self,
        resources: AdmissionResources,
        owner: Option<u64>,
        seq: u64,
        continue_on_error: bool,
        runnable_post_dep: bool,
    ) -> AdmissionAction {
        decide_admission(
            self.snapshot(continue_on_error, owner),
            AdmissionProbe {
                resources,
                owner,
                seq,
                runnable_post_dep,
            },
        )
    }

    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::SeqCst);
        self.state_notify.notify_waiters();
    }

    pub fn mark_task_finished(&self, class: SpawnClass, owner: Option<u64>) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        if class.is_runtime() {
            self.runtime_in_flight.fetch_sub(1, Ordering::SeqCst);
            if let Some(owner_id) = owner {
                let mut by_owner = self.runtime_in_flight_by_owner.lock().unwrap();
                if let Some(count) = by_owner.get_mut(&owner_id) {
                    if *count > 1 {
                        *count -= 1;
                    } else {
                        by_owner.remove(&owner_id);
                    }
                }
            }
        }
        self.state_notify.notify_waiters();
    }

    #[cfg(test)]
    pub async fn wait_for_spawn_slot(
        &self,
        class: SpawnClass,
        task_name: &str,
    ) -> Option<InteractiveGateGuard> {
        if class.requires_interactive_barrier() {
            let owner_id = 1;
            let owner = Some(owner_id);
            let seq = 1;
            loop {
                // Register+enable before policy evaluation to avoid lost wakeups
                // between decision and await.
                let state_changed = self.state_notify.notified();
                tokio::pin!(state_changed);
                state_changed.as_mut().enable();
                let decision = decide_ready(self.snapshot(true, owner), class, owner, seq, true);
                let no_runtime_running = !matches!(decision, SchedulerDecision::WaitBarrier);
                let claimed = self
                    .interactive_owner
                    .compare_exchange(0, owner_id, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok();
                if no_runtime_running && claimed {
                    trace!("interactive gate acquired for {task_name}");
                    break Some(InteractiveGateGuard::new(
                        self.interactive_owner.clone(),
                        owner_id,
                        self.state_notify.clone(),
                    ));
                }
                if claimed {
                    self.interactive_owner.store(0, Ordering::SeqCst);
                }
                trace!(
                    "waiting interactive gate for {task_name} (runtime_in_flight={}, interactive_owner={})",
                    self.runtime_in_flight.load(Ordering::SeqCst),
                    self.interactive_owner.load(Ordering::SeqCst)
                );
                state_changed.as_mut().await;
            }
        } else if class.is_runtime() {
            loop {
                // Register+enable before decision to avoid missing a wakeup that
                // arrives between `decide_ready` and `await`.
                let state_changed = self.state_notify.notified();
                tokio::pin!(state_changed);
                state_changed.as_mut().enable();
                if !matches!(
                    decide_ready(self.snapshot(true, None), class, None, 1, true),
                    SchedulerDecision::WaitBarrier
                ) {
                    break;
                }
                trace!("waiting for interactive task to finish before starting {task_name}");
                state_changed.as_mut().await;
            }
            None
        } else {
            None
        }
    }

    pub async fn admit_task(
        &self,
        class: SpawnClass,
        task_name: &str,
        task_owner: Option<u64>,
        task_seq: u64,
        continue_on_error: bool,
        runnable_post_dep: bool,
    ) -> Result<AdmissionOutcome> {
        let owner = self.assign_interactive_owner(class, task_owner);
        let mut interactive_guard = BarrierLease::Missing;
        let mut permit = PermitLease::Missing;

        loop {
            // Register+enable before consulting policy to avoid lost wakeups where
            // state changes after decision=WaitBarrier but before await.
            let state_changed = self.state_notify.notified();
            tokio::pin!(state_changed);
            state_changed.as_mut().enable();
            let resources =
                Self::admission_resources(class, interactive_guard.state(), permit.state());
            match self.admission_action(
                resources,
                owner,
                task_seq,
                continue_on_error,
                runnable_post_dep,
            ) {
                AdmissionAction::Drop => {
                    self.release_queued_admission(class, task_seq);
                    return Ok(AdmissionOutcome::Drop);
                }
                AdmissionAction::WaitBarrier => {
                    trace!(
                        "waiting barrier for {task_name} (owner={owner:?}, seq={task_seq}, runtime_in_flight={}, interactive_owner={})",
                        self.runtime_in_flight.load(Ordering::SeqCst),
                        self.interactive_owner.load(Ordering::SeqCst)
                    );
                    state_changed.as_mut().await;
                }
                AdmissionAction::ClaimInteractiveBarrier => {
                    let owner_id = owner.expect("interactive barrier claim requires owner");
                    let claimed = self
                        .interactive_owner
                        .compare_exchange(0, owner_id, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok();
                    if claimed {
                        trace!("interactive gate acquired for {task_name}");
                        interactive_guard = BarrierLease::Held(InteractiveGateGuard::new(
                            self.interactive_owner.clone(),
                            owner_id,
                            self.state_notify.clone(),
                        ));
                    } else {
                        trace!("interactive gate busy for {task_name}; waiting for state change");
                        state_changed.as_mut().await;
                    }
                }
                AdmissionAction::WaitPermit | AdmissionAction::AcquirePermit => {
                    if permit.is_held() {
                        continue;
                    }
                    let wait_start = std::time::Instant::now();
                    let acquired_permit = match self.semaphore.clone().acquire_owned().await {
                        Ok(permit) => permit,
                        Err(err) => {
                            self.release_queued_admission(class, task_seq);
                            return Err(err.into());
                        }
                    };
                    trace!(
                        "semaphore acquired for {task_name} after {}ms",
                        wait_start.elapsed().as_millis()
                    );
                    permit = PermitLease::Held(acquired_permit);
                }
                AdmissionAction::Start => {
                    if class.is_runtime() {
                        self.release_pending_permit(task_seq);
                    }
                    if class.requires_interactive_barrier() && interactive_guard.is_held() {
                        self.release_pending_interactive_if_head(task_seq);
                    }
                    self.in_flight.fetch_add(1, Ordering::SeqCst);
                    if class.is_runtime() {
                        self.runtime_in_flight.fetch_add(1, Ordering::SeqCst);
                        if let Some(owner_id) = owner {
                            *self
                                .runtime_in_flight_by_owner
                                .lock()
                                .unwrap()
                                .entry(owner_id)
                                .or_insert(0) += 1;
                        }
                    }
                    self.state_notify.notify_waiters();

                    return Ok(AdmissionOutcome::Start(AdmissionTicket {
                        permit: permit.into_option(),
                        interactive_guard: interactive_guard.into_option(),
                        interactive_owner: owner,
                    }));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use tokio::time::{Duration, timeout};

    fn order_key(name: &str) -> TaskOrderKey {
        (name.to_string(), vec![], vec![])
    }

    #[test]
    fn test_scheduler_new() {
        let scheduler = Scheduler::new(4);
        // Verify basic initialization
        assert_eq!(
            scheduler.in_flight_count(),
            0,
            "in_flight should start at 0"
        );
        assert_eq!(
            scheduler.runtime_in_flight.load(Ordering::SeqCst),
            0,
            "runtime_in_flight should start at 0"
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

    #[tokio::test]
    async fn runtime_waits_while_interactive_running() {
        // Matrix: B08 (C1, C11)
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);
        ctx.interactive_owner.store(42, Ordering::SeqCst);
        let ctx2 = ctx.clone();
        let waiter = tokio::spawn(async move {
            ctx2.wait_for_spawn_slot(SpawnClass::Runtime, "runtime")
                .await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !waiter.is_finished(),
            "runtime should wait until interactive task is done"
        );

        ctx.interactive_owner.store(0, Ordering::SeqCst);
        ctx.state_notify.notify_waiters();
        let guard = timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
        assert!(guard.is_none());
    }

    #[tokio::test]
    async fn interactive_waits_for_runtime_to_drain_before_acquiring_barrier() {
        // Matrix: B07 (C1, C11)
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);
        ctx.runtime_in_flight.store(1, Ordering::SeqCst);

        let ctx2 = ctx.clone();
        let waiter = tokio::spawn(async move {
            ctx2.wait_for_spawn_slot(SpawnClass::InteractiveRuntime, "interactive")
                .await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !waiter.is_finished(),
            "interactive should wait while runtime tasks are in flight"
        );

        ctx.runtime_in_flight.store(0, Ordering::SeqCst);
        ctx.state_notify.notify_waiters();
        let guard = timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap()
            .expect("interactive guard expected");
        assert_ne!(ctx.interactive_owner.load(Ordering::SeqCst), 0);
        drop(guard);
        assert_eq!(ctx.interactive_owner.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn non_runtime_does_not_wait_on_interactive_barrier() {
        // Matrix: C7/B11 (orchestrator semantics)
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);
        ctx.interactive_owner.store(42, Ordering::SeqCst);
        let guard = timeout(
            Duration::from_secs(1),
            ctx.wait_for_spawn_slot(SpawnClass::NonRuntime, "orchestrator"),
        )
        .await
        .unwrap();
        assert!(guard.is_none());
    }

    #[tokio::test]
    async fn interactive_guard_drop_notifies_waiters() {
        // Matrix: F01/F02/F03/F04/F05/F10 (C12)
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);
        let guard = ctx
            .wait_for_spawn_slot(SpawnClass::InteractiveRuntime, "interactive")
            .await
            .expect("interactive guard expected");
        let ctx2 = ctx.clone();
        let waiter = tokio::spawn(async move {
            ctx2.wait_for_spawn_slot(SpawnClass::Runtime, "runtime")
                .await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!waiter.is_finished());
        drop(guard);
        let result = timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn jobs_one_progresses_without_deadlock_between_runtime_and_interactive() {
        // Matrix: B12 (C13, C16)
        let scheduler = Scheduler::new(1);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);

        let permit = ctx.semaphore.clone().acquire_owned().await.expect("permit");
        ctx.runtime_in_flight.store(1, Ordering::SeqCst);

        let ctx2 = ctx.clone();
        let waiter = tokio::spawn(async move {
            ctx2.wait_for_spawn_slot(SpawnClass::InteractiveRuntime, "interactive")
                .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!waiter.is_finished(), "interactive should wait for runtime");

        ctx.runtime_in_flight.store(0, Ordering::SeqCst);
        drop(permit);
        ctx.state_notify.notify_waiters();

        let guard = timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap()
            .expect("interactive guard expected");
        drop(guard);
    }

    #[tokio::test]
    async fn admit_task_runtime_waits_for_permit_then_starts() {
        // Matrix: B12/F13 (C13)
        let scheduler = Scheduler::new(1);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);

        let held = ctx.semaphore.clone().acquire_owned().await.expect("permit");
        let ctx2 = ctx.clone();
        let waiter = tokio::spawn(async move {
            let seq = ctx2.assign_scheduler_seq(None);
            ctx2.admit_task(SpawnClass::Runtime, "runtime", None, seq, true, true)
                .await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !waiter.is_finished(),
            "runtime admission should wait while permit unavailable"
        );

        drop(held);
        let outcome = timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        match outcome {
            AdmissionOutcome::Start(ticket) => {
                assert!(ticket.permit.is_some());
                assert!(ticket.interactive_guard.is_none());
            }
            AdmissionOutcome::Drop => panic!("runtime admission unexpectedly dropped"),
        }
    }

    #[tokio::test]
    async fn admit_task_runtime_waits_while_earlier_interactive_seq_is_pending() {
        // Matrix: B06/B08 (C1, C10, C11) - deterministic admission ordering.
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);

        let owner = ctx
            .assign_interactive_owner(SpawnClass::InteractiveRuntime, None)
            .expect("interactive owner should be assigned");
        let interactive_seq = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_interactive(interactive_seq, Some(owner), order_key("interactive"));

        let ctx2 = ctx.clone();
        let waiter = tokio::spawn(async move {
            let runtime_seq = ctx2.assign_scheduler_seq(None);
            ctx2.admit_task(
                SpawnClass::Runtime,
                "runtime",
                None,
                runtime_seq,
                true,
                true,
            )
            .await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !waiter.is_finished(),
            "runtime should wait while an earlier interactive sequence is pending"
        );

        let interactive = ctx
            .admit_task(
                SpawnClass::InteractiveRuntime,
                "interactive",
                Some(owner),
                interactive_seq,
                true,
                true,
            )
            .await
            .unwrap();
        let ticket = match interactive {
            AdmissionOutcome::Start(ticket) => ticket,
            AdmissionOutcome::Drop => panic!("interactive admission unexpectedly dropped"),
        };
        drop(ticket);

        let outcome = timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        match outcome {
            AdmissionOutcome::Start(ticket) => {
                assert!(ticket.interactive_guard.is_none());
                drop(ticket);
            }
            AdmissionOutcome::Drop => panic!("runtime admission unexpectedly dropped"),
        }
    }

    #[tokio::test]
    async fn admit_task_interactive_claims_barrier_and_returns_guard() {
        // Matrix: B07/B08/F01/F10 (C1, C11, C12)
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);

        let outcome = ctx
            .admit_task(
                SpawnClass::InteractiveRuntime,
                "interactive",
                None,
                ctx.assign_scheduler_seq(None),
                true,
                true,
            )
            .await
            .unwrap();
        match outcome {
            AdmissionOutcome::Start(ticket) => {
                assert!(ticket.interactive_guard.is_some());
                assert_ne!(ctx.interactive_owner.load(Ordering::SeqCst), 0);
                drop(ticket);
                assert_eq!(ctx.interactive_owner.load(Ordering::SeqCst), 0);
            }
            AdmissionOutcome::Drop => panic!("interactive admission unexpectedly dropped"),
        }
    }

    #[tokio::test]
    async fn admit_task_runtime_same_owner_can_start_while_interactive_owner_is_active() {
        // Matrix: B08/B10 (C1, C11)
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);

        let parent = ctx
            .admit_task(
                SpawnClass::InteractiveRuntime,
                "mixed-parent",
                None,
                {
                    let seq = ctx.assign_scheduler_seq(None);
                    ctx.enqueue_pending_interactive(seq, None, order_key("mixed-parent"));
                    seq
                },
                true,
                true,
            )
            .await
            .unwrap();
        let parent_ticket = match parent {
            AdmissionOutcome::Start(ticket) => ticket,
            AdmissionOutcome::Drop => panic!("interactive parent unexpectedly dropped"),
        };
        let owner = parent_ticket
            .interactive_owner
            .expect("interactive owner should be assigned");

        let child = timeout(
            Duration::from_secs(1),
            ctx.admit_task(
                SpawnClass::Runtime,
                "owned-child",
                Some(owner),
                ctx.assign_scheduler_seq(None),
                true,
                true,
            ),
        )
        .await
        .expect("owned child admission timed out")
        .expect("owned child admission failed");

        match child {
            AdmissionOutcome::Start(ticket) => {
                assert!(ticket.permit.is_none());
                assert!(ticket.interactive_guard.is_none());
                drop(ticket);
            }
            AdmissionOutcome::Drop => panic!("owned child unexpectedly dropped"),
        }

        drop(parent_ticket);
    }

    #[tokio::test]
    async fn admit_task_interactive_same_owner_runtime_parent_does_not_deadlock() {
        // Regression: mixed runtime parent waiting on injected interactive child must progress.
        // Matrix: B07/B09/B10/B12 (C1, C11, C13)
        let scheduler = Scheduler::new(1);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);

        let owner = ctx
            .allocate_owner_id(None)
            .expect("owner must be assigned for mixed runtime parent");
        let parent_seq = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_permit(parent_seq);
        let parent = ctx
            .admit_task(
                SpawnClass::Runtime,
                "qa-suite",
                Some(owner),
                parent_seq,
                true,
                true,
            )
            .await
            .unwrap();
        let parent_ticket = match parent {
            AdmissionOutcome::Start(ticket) => ticket,
            AdmissionOutcome::Drop => panic!("runtime parent unexpectedly dropped"),
        };
        assert!(parent_ticket.permit.is_some());

        let child_seq = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_permit(child_seq);
        ctx.enqueue_pending_interactive(child_seq, Some(owner), order_key("smoke-interactive"));
        let child = timeout(
            Duration::from_secs(1),
            ctx.admit_task(
                SpawnClass::InteractiveRuntime,
                "smoke-interactive",
                Some(owner),
                child_seq,
                true,
                true,
            ),
        )
        .await
        .expect("interactive child admission timed out")
        .expect("interactive child admission failed");

        match child {
            AdmissionOutcome::Start(ticket) => {
                assert!(ticket.interactive_guard.is_some());
                assert!(
                    ticket.permit.is_none(),
                    "child should reuse owner runtime slot"
                );
                drop(ticket);
            }
            AdmissionOutcome::Drop => panic!("interactive child unexpectedly dropped"),
        }

        drop(parent_ticket);
    }

    #[tokio::test]
    async fn admit_task_two_mixed_runtime_parents_should_not_deadlock_injected_interactives() {
        // Regression: with 2 concurrent mixed runtime parents, injected interactive children
        // must still make forward progress (no circular wait on foreign runtime count).
        // Matrix: B05/B07/B10/B12/B13 + O7 (C1, C10, C11, C13)
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);

        let owner_a = ctx
            .allocate_owner_id(None)
            .expect("owner A should be assigned");
        let owner_b = ctx
            .allocate_owner_id(None)
            .expect("owner B should be assigned");

        let parent_a_seq = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_permit(parent_a_seq);
        let parent_a = ctx
            .admit_task(
                SpawnClass::Runtime,
                "qa-a",
                Some(owner_a),
                parent_a_seq,
                true,
                true,
            )
            .await
            .unwrap();
        let parent_a_ticket = match parent_a {
            AdmissionOutcome::Start(ticket) => ticket,
            AdmissionOutcome::Drop => panic!("parent A unexpectedly dropped"),
        };

        let parent_b_seq = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_permit(parent_b_seq);
        let parent_b = ctx
            .admit_task(
                SpawnClass::Runtime,
                "qa-b",
                Some(owner_b),
                parent_b_seq,
                true,
                true,
            )
            .await
            .unwrap();
        let parent_b_ticket = match parent_b {
            AdmissionOutcome::Start(ticket) => ticket,
            AdmissionOutcome::Drop => panic!("parent B unexpectedly dropped"),
        };

        let child_a_seq = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_permit(child_a_seq);
        ctx.enqueue_pending_interactive(child_a_seq, Some(owner_a), order_key("ia"));

        let child_b_seq = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_permit(child_b_seq);
        ctx.enqueue_pending_interactive(child_b_seq, Some(owner_b), order_key("ib"));

        let child_a = timeout(
            Duration::from_millis(500),
            ctx.admit_task(
                SpawnClass::InteractiveRuntime,
                "ia",
                Some(owner_a),
                child_a_seq,
                true,
                true,
            ),
        )
        .await
        .expect("child A admission timed out (deadlock)");

        match child_a.unwrap() {
            AdmissionOutcome::Start(ticket) => drop(ticket),
            AdmissionOutcome::Drop => panic!("child A unexpectedly dropped"),
        }

        drop(parent_b_ticket);
        drop(parent_a_ticket);
    }

    #[tokio::test]
    async fn admit_task_should_drop_when_stop_is_requested_while_waiting_permit() {
        // Matrix: B14/F13 (C5, C12, C13) - regression guard for stale stopping snapshot.
        let scheduler = Scheduler::new(1);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);

        let held = ctx.semaphore.clone().acquire_owned().await.expect("permit");
        let ctx2 = ctx.clone();

        let waiter = tokio::spawn(async move {
            let seq = ctx2.assign_scheduler_seq(None);
            ctx2.admit_task(SpawnClass::Runtime, "late-runtime", None, seq, false, false)
                .await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        ctx.request_stop();
        drop(held);

        let outcome = timeout(Duration::from_secs(2), waiter)
            .await
            .expect("waiter timed out")
            .expect("waiter join failed")
            .expect("admission failed");

        assert!(
            matches!(outcome, AdmissionOutcome::Drop),
            "expected Drop after stop request while waiting permit, got Start"
        );
    }

    #[tokio::test]
    async fn dropping_non_head_interactive_should_not_leave_stale_pending_barrier() {
        // Matrix: B16/F14/G04 (C5, C12, C13) - dropped non-head interactive must not stall cleanup runtime.
        let scheduler = Scheduler::new(2);
        let config = Config::get().await.unwrap();
        let ctx = scheduler.spawn_context(config);
        ctx.request_stop();

        let seq1 = ctx.assign_scheduler_seq(None);
        let seq2 = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_interactive(seq1, None, order_key("interactive-head"));
        ctx.enqueue_pending_interactive(seq2, None, order_key("interactive-late"));

        let drop_late = ctx
            .admit_task(
                SpawnClass::InteractiveRuntime,
                "interactive-late",
                None,
                seq2,
                false,
                false,
            )
            .await
            .unwrap();
        assert!(matches!(drop_late, AdmissionOutcome::Drop));

        let drop_head = ctx
            .admit_task(
                SpawnClass::InteractiveRuntime,
                "interactive-head",
                None,
                seq1,
                false,
                false,
            )
            .await
            .unwrap();
        assert!(matches!(drop_head, AdmissionOutcome::Drop));

        let cleanup_seq = ctx.assign_scheduler_seq(None);
        ctx.enqueue_pending_permit(cleanup_seq);
        let cleanup = timeout(
            Duration::from_secs(1),
            ctx.admit_task(
                SpawnClass::Runtime,
                "cleanup-runtime",
                None,
                cleanup_seq,
                false,
                true,
            ),
        )
        .await
        .expect("cleanup admission timed out")
        .expect("cleanup admission failed");

        match cleanup {
            AdmissionOutcome::Start(ticket) => drop(ticket),
            AdmissionOutcome::Drop => panic!("cleanup runtime unexpectedly dropped"),
        }
    }
}
