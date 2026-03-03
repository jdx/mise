use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::task::task_execution_plan::{
    ExecutionPlan, ExecutionStageKind, PlanContextIndex, execution_stage_kind_label,
};
use crate::task::task_identity::TaskIdentity;
use crate::task::{Deps, Task};
use eyre::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Mutex, Notify, Semaphore, mpsc};
use tokio::task::JoinSet;

#[cfg(unix)]
use nix::sys::signal::SIGTERM;

pub type SchedMsg = (Task, Arc<Mutex<Deps>>);

#[derive(Debug, Clone, Copy)]
struct TaskStageTrace {
    stage_index: usize,
    stage_kind: ExecutionStageKind,
}

#[derive(Debug, Clone)]
struct SchedulerPlanTraceContext {
    plan_hash: Option<String>,
    stage_count: usize,
    by_identity: HashMap<TaskIdentity, TaskStageTrace>,
}

#[derive(Debug, Default)]
struct PlanStageBarrierState {
    active_stage: usize,
    stage_count: usize,
    stage_remaining: HashMap<usize, usize>,
    by_identity: HashMap<TaskIdentity, usize>,
}

#[derive(Clone, Debug, Default)]
pub struct PlanStageBarrier {
    state: Arc<StdMutex<PlanStageBarrierState>>,
    notify: Arc<Notify>,
}

#[derive(Debug, Default)]
struct InteractiveBarrierState {
    interactive_active: bool,
    runtime_in_flight: usize,
}

#[derive(Clone, Debug, Default)]
pub struct InteractiveBarrier {
    state: Arc<StdMutex<InteractiveBarrierState>>,
    notify: Arc<Notify>,
}

#[derive(Debug, Clone, Copy)]
enum InteractiveBarrierMode {
    Runtime,
    Interactive,
}

#[derive(Debug)]
pub struct InteractiveBarrierGuard {
    barrier: InteractiveBarrier,
    mode: InteractiveBarrierMode,
    released: bool,
}

impl InteractiveBarrier {
    pub fn new() -> Self {
        Self {
            state: Arc::new(StdMutex::new(InteractiveBarrierState::default())),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Acquire a runtime slot. This blocks only while an interactive task is active.
    pub async fn acquire_runtime(&self) -> InteractiveBarrierGuard {
        loop {
            let notified = self.notify.notified();
            let acquired = {
                let mut state = self.state.lock().unwrap();
                if state.interactive_active {
                    false
                } else {
                    state.runtime_in_flight += 1;
                    true
                }
            };
            if acquired {
                return InteractiveBarrierGuard {
                    barrier: self.clone(),
                    mode: InteractiveBarrierMode::Runtime,
                    released: false,
                };
            }
            notified.await;
        }
    }

    /// Acquire the global interactive lock. This blocks until:
    /// - no interactive task is active
    /// - no runtime task is in flight
    pub async fn acquire_interactive(&self) -> InteractiveBarrierGuard {
        loop {
            let notified = self.notify.notified();
            let acquired = {
                let mut state = self.state.lock().unwrap();
                if state.interactive_active || state.runtime_in_flight > 0 {
                    false
                } else {
                    state.interactive_active = true;
                    true
                }
            };
            if acquired {
                return InteractiveBarrierGuard {
                    barrier: self.clone(),
                    mode: InteractiveBarrierMode::Interactive,
                    released: false,
                };
            }
            notified.await;
        }
    }

    fn release_runtime(&self) {
        {
            let mut state = self.state.lock().unwrap();
            state.runtime_in_flight = state.runtime_in_flight.saturating_sub(1);
        }
        self.notify.notify_waiters();
    }

    fn release_interactive(&self) {
        {
            let mut state = self.state.lock().unwrap();
            state.interactive_active = false;
        }
        self.notify.notify_waiters();
    }

    #[cfg(test)]
    fn snapshot(&self) -> (bool, usize) {
        let state = self.state.lock().unwrap();
        (state.interactive_active, state.runtime_in_flight)
    }
}

impl InteractiveBarrierGuard {
    fn release(&mut self) {
        if self.released {
            return;
        }
        match self.mode {
            InteractiveBarrierMode::Runtime => self.barrier.release_runtime(),
            InteractiveBarrierMode::Interactive => self.barrier.release_interactive(),
        }
        self.released = true;
    }
}

impl Drop for InteractiveBarrierGuard {
    fn drop(&mut self) {
        self.release();
    }
}

impl PlanStageBarrier {
    pub fn from_plan(plan: &ExecutionPlan) -> Self {
        let plan_index = PlanContextIndex::from_plan(plan, None);
        let by_identity = plan_index
            .contexts()
            .iter()
            .map(|(identity, context)| (identity.clone(), context.stage_index))
            .collect();
        let stage_remaining = plan
            .stages
            .iter()
            .enumerate()
            .map(|(idx, stage)| (idx + 1, stage.tasks.len()))
            .collect();
        let stage_count = plan.stages.len();
        let active_stage = if stage_count == 0 { 0 } else { 1 };
        Self {
            state: Arc::new(StdMutex::new(PlanStageBarrierState {
                active_stage,
                stage_count,
                stage_remaining,
                by_identity,
            })),
            notify: Arc::new(Notify::new()),
        }
    }

    pub async fn wait_for_task_stage(&self, task: &Task) {
        let identity = TaskIdentity::from_task(task);
        let stage = {
            let state = self.state.lock().unwrap();
            state.by_identity.get(&identity).copied()
        };
        let Some(stage) = stage else {
            return;
        };
        loop {
            let notified = self.notify.notified();
            let ready = {
                let state = self.state.lock().unwrap();
                state.active_stage == 0 || stage <= state.active_stage
            };
            if ready {
                return;
            }
            notified.await;
        }
    }

    pub fn mark_task_complete(&self, task: &Task) {
        let identity = TaskIdentity::from_task(task);
        let mut advanced = false;
        {
            let mut state = self.state.lock().unwrap();
            let Some(stage) = state.by_identity.get(&identity).copied() else {
                return;
            };
            let Some(remaining) = state.stage_remaining.get_mut(&stage) else {
                return;
            };
            if *remaining == 0 {
                return;
            }
            *remaining = remaining.saturating_sub(1);
            if stage == state.active_stage {
                while state.active_stage > 0 {
                    let current_remaining = state
                        .stage_remaining
                        .get(&state.active_stage)
                        .copied()
                        .unwrap_or(0);
                    if current_remaining > 0 {
                        break;
                    }
                    if state.active_stage >= state.stage_count {
                        state.active_stage = 0;
                        advanced = true;
                        break;
                    }
                    state.active_stage += 1;
                    advanced = true;
                }
            }
        }
        if advanced {
            self.notify.notify_waiters();
        }
    }

    #[cfg(test)]
    fn snapshot(&self) -> (usize, HashMap<usize, usize>) {
        let state = self.state.lock().unwrap();
        (state.active_stage, state.stage_remaining.clone())
    }
}

/// Schedules and executes tasks with concurrency control
pub struct Scheduler {
    pub semaphore: Arc<Semaphore>,
    pub interactive_barrier: InteractiveBarrier,
    pub jset: Arc<Mutex<JoinSet<Result<()>>>>,
    pub sched_tx: Arc<mpsc::UnboundedSender<SchedMsg>>,
    pub sched_rx: Option<mpsc::UnboundedReceiver<SchedMsg>>,
    pub in_flight: Arc<AtomicUsize>,
    plan_trace_context: Option<SchedulerPlanTraceContext>,
    plan_stage_barrier: Option<PlanStageBarrier>,
}

impl Scheduler {
    pub fn new(jobs: usize) -> Self {
        let (sched_tx, sched_rx) = mpsc::unbounded_channel::<SchedMsg>();
        Self {
            semaphore: Arc::new(Semaphore::new(jobs)),
            interactive_barrier: InteractiveBarrier::new(),
            jset: Arc::new(Mutex::new(JoinSet::new())),
            sched_tx: Arc::new(sched_tx),
            sched_rx: Some(sched_rx),
            in_flight: Arc::new(AtomicUsize::new(0)),
            plan_trace_context: None,
            plan_stage_barrier: None,
        }
    }

    pub fn set_plan_trace_context(&mut self, plan: &ExecutionPlan, plan_hash: Option<String>) {
        let plan_index = PlanContextIndex::from_plan(plan, plan_hash.clone());
        let by_identity = plan_index
            .contexts()
            .clone()
            .into_iter()
            .map(|(identity, context)| {
                (
                    identity,
                    TaskStageTrace {
                        stage_index: context.stage_index,
                        stage_kind: context.stage_kind,
                    },
                )
            })
            .collect();

        self.plan_trace_context = Some(SchedulerPlanTraceContext {
            plan_hash,
            stage_count: plan_index.stage_count(),
            by_identity,
        });
        self.plan_stage_barrier = Some(PlanStageBarrier::from_plan(plan));
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
            interactive_barrier: self.interactive_barrier.clone(),
            plan_stage_barrier: self.plan_stage_barrier.clone(),
            config,
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
        let plan_trace_context = self.plan_trace_context.clone();

        // Forward initial leaves synchronously
        {
            let mut rx = deps_clone.lock().await.subscribe();
            loop {
                match rx.try_recv() {
                    Ok(Some(task)) => {
                        trace!(
                            "main deps initial leaf: {} {}{}",
                            task.name,
                            task.args.join(" "),
                            self.task_trace_suffix(&task)
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
                            "main deps leaf scheduled: {} {}{}",
                            task.name,
                            task.args.join(" "),
                            Scheduler::task_trace_suffix_with_context(
                                plan_trace_context.as_ref(),
                                &task,
                            )
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
                        trace!(
                            "scheduler received: {} {}{}",
                            task.name,
                            task.args.join(" "),
                            self.task_trace_suffix(&task)
                        );
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
                        trace!(
                            "scheduler received: {} {}{}",
                            task.name,
                            task.args.join(" "),
                            self.task_trace_suffix(&task)
                        );
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

    fn task_trace_suffix(&self, task: &Task) -> String {
        Self::task_trace_suffix_with_context(self.plan_trace_context.as_ref(), task)
    }

    fn task_trace_suffix_with_context(
        context: Option<&SchedulerPlanTraceContext>,
        task: &Task,
    ) -> String {
        let Some(context) = context else {
            return String::new();
        };
        let identity = TaskIdentity::from_task(task);
        let Some(stage) = context.by_identity.get(&identity) else {
            return String::new();
        };
        let kind = execution_stage_kind_label(stage.stage_kind);
        let hash = context
            .plan_hash
            .as_ref()
            .map(|h| format!(", plan={h}"))
            .unwrap_or_default();
        format!(
            " [stage {}/{}, kind={}{}]",
            stage.stage_index, context.stage_count, kind, hash
        )
    }
}

/// Context passed to spawned tasks
#[derive(Clone)]
pub struct SpawnContext {
    pub semaphore: Arc<Semaphore>,
    pub interactive_barrier: InteractiveBarrier,
    pub plan_stage_barrier: Option<PlanStageBarrier>,
    pub config: Arc<Config>,
    pub jset: Arc<Mutex<JoinSet<Result<()>>>>,
    pub in_flight: Arc<AtomicUsize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::task_execution_plan::{ExecutionStage, PlannedTask, TaskDeclarationRef};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

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

    #[test]
    fn test_scheduler_trace_suffix_includes_stage_and_plan_hash() {
        let mut scheduler = Scheduler::new(4);
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage::parallel(vec![PlannedTask {
                identity: TaskIdentity {
                    name: "build".to_string(),
                    args: vec![],
                    env: vec![],
                },
                runtime: true,
                interactive: false,
                declaration: TaskDeclarationRef {
                    source: "<generated>".to_string(),
                    line: None,
                },
            }])],
        };

        scheduler.set_plan_trace_context(&plan, Some("sha256:test".to_string()));
        let task = Task {
            name: "build".to_string(),
            ..Default::default()
        };

        let suffix = scheduler.task_trace_suffix(&task);
        assert!(suffix.contains("stage 1/1"));
        assert!(suffix.contains("kind=parallel"));
        assert!(suffix.contains("plan=sha256:test"));
    }

    #[test]
    fn test_scheduler_trace_suffix_empty_without_plan_context() {
        let scheduler = Scheduler::new(4);
        let task = Task {
            name: "build".to_string(),
            ..Default::default()
        };
        assert!(scheduler.task_trace_suffix(&task).is_empty());
    }

    #[tokio::test]
    async fn test_plan_stage_barrier_blocks_future_stage_until_current_completes() {
        let build = Task {
            name: "build".to_string(),
            ..Default::default()
        };
        let ask = Task {
            name: "ask".to_string(),
            ..Default::default()
        };
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage::parallel(vec![PlannedTask {
                    identity: TaskIdentity::from_task(&build),
                    runtime: true,
                    interactive: false,
                    declaration: Default::default(),
                }]),
                ExecutionStage::interactive(PlannedTask {
                    identity: TaskIdentity::from_task(&ask),
                    runtime: true,
                    interactive: true,
                    declaration: Default::default(),
                }),
            ],
        };
        let barrier = PlanStageBarrier::from_plan(&plan);
        assert_eq!(barrier.snapshot().0, 1);
        let barrier_wait = barrier.clone();
        let ask_task = ask.clone();
        let waiter = tokio::spawn(async move {
            barrier_wait.wait_for_task_stage(&ask_task).await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter.is_finished());

        barrier.mark_task_complete(&build);
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(barrier.snapshot().0, 2);
    }

    #[tokio::test]
    async fn test_plan_stage_barrier_waits_for_all_tasks_in_current_stage() {
        let a = Task {
            name: "a".to_string(),
            ..Default::default()
        };
        let b = Task {
            name: "b".to_string(),
            ..Default::default()
        };
        let c = Task {
            name: "c".to_string(),
            ..Default::default()
        };
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage::parallel(vec![
                    PlannedTask {
                        identity: TaskIdentity::from_task(&a),
                        runtime: true,
                        interactive: false,
                        declaration: Default::default(),
                    },
                    PlannedTask {
                        identity: TaskIdentity::from_task(&b),
                        runtime: true,
                        interactive: false,
                        declaration: Default::default(),
                    },
                ]),
                ExecutionStage::interactive(PlannedTask {
                    identity: TaskIdentity::from_task(&c),
                    runtime: true,
                    interactive: true,
                    declaration: Default::default(),
                }),
            ],
        };
        let barrier = PlanStageBarrier::from_plan(&plan);
        assert_eq!(barrier.snapshot().0, 1);
        let barrier_wait = barrier.clone();
        let c_task = c.clone();
        let waiter = tokio::spawn(async move {
            barrier_wait.wait_for_task_stage(&c_task).await;
        });

        barrier.mark_task_complete(&a);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter.is_finished());
        assert_eq!(barrier.snapshot().0, 1);

        barrier.mark_task_complete(&b);
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(barrier.snapshot().0, 2);
    }

    #[tokio::test]
    async fn test_interactive_barrier_interactive_waits_for_runtime() {
        // MatrixRef: B01,B05 / C1,C10,C12
        let barrier = InteractiveBarrier::new();
        let runtime_guard = barrier.acquire_runtime().await;
        let acquired = Arc::new(AtomicBool::new(false));
        let acquired_c = acquired.clone();
        let barrier_c = barrier.clone();
        let jh = tokio::spawn(async move {
            let _interactive_guard = barrier_c.acquire_interactive().await;
            acquired_c.store(true, Ordering::SeqCst);
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!acquired.load(Ordering::SeqCst));

        drop(runtime_guard);
        tokio::time::timeout(Duration::from_secs(1), jh)
            .await
            .unwrap()
            .unwrap();
        assert!(acquired.load(Ordering::SeqCst));
        assert_eq!(barrier.snapshot(), (false, 0));
    }

    #[tokio::test]
    async fn test_interactive_barrier_runtime_waits_for_interactive() {
        // MatrixRef: B08 / C1,C11,C12
        let barrier = InteractiveBarrier::new();
        let interactive_guard = barrier.acquire_interactive().await;
        let acquired = Arc::new(AtomicBool::new(false));
        let acquired_c = acquired.clone();
        let barrier_c = barrier.clone();
        let jh = tokio::spawn(async move {
            let _runtime_guard = barrier_c.acquire_runtime().await;
            acquired_c.store(true, Ordering::SeqCst);
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!acquired.load(Ordering::SeqCst));

        drop(interactive_guard);
        tokio::time::timeout(Duration::from_secs(1), jh)
            .await
            .unwrap()
            .unwrap();
        assert!(acquired.load(Ordering::SeqCst));
        assert_eq!(barrier.snapshot(), (false, 0));
    }

    #[tokio::test]
    async fn test_interactive_barrier_release_on_drop() {
        // MatrixRef: F01,F06,F10 / C12
        let barrier = InteractiveBarrier::new();
        {
            let _guard = barrier.acquire_interactive().await;
            assert_eq!(barrier.snapshot(), (true, 0));
        }
        assert_eq!(barrier.snapshot(), (false, 0));
    }

    #[tokio::test]
    async fn test_interactive_barrier_runtime_release_on_drop() {
        // MatrixRef: F07,F08,F09,F10 / C12
        let barrier = InteractiveBarrier::new();
        {
            let _guard = barrier.acquire_runtime().await;
            assert_eq!(barrier.snapshot(), (false, 1));
        }
        assert_eq!(barrier.snapshot(), (false, 0));
    }

    #[tokio::test]
    async fn test_interactive_barrier_two_interactive_waiters_are_serialized() {
        // MatrixRef: B05 / C1,C10,C12
        let barrier = InteractiveBarrier::new();
        let first = barrier.acquire_interactive().await;
        let second_started = Arc::new(AtomicBool::new(false));
        let second_started_c = second_started.clone();
        let barrier_c = barrier.clone();
        let jh = tokio::spawn(async move {
            let _second = barrier_c.acquire_interactive().await;
            second_started_c.store(true, Ordering::SeqCst);
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!second_started.load(Ordering::SeqCst));
        drop(first);

        tokio::time::timeout(Duration::from_secs(1), jh)
            .await
            .unwrap()
            .unwrap();
        assert!(second_started.load(Ordering::SeqCst));
        assert_eq!(barrier.snapshot(), (false, 0));
    }

    #[tokio::test]
    async fn test_interactive_barrier_release_wakes_all_runtime_waiters() {
        // MatrixRef: G02,F10 / C12,C13
        let barrier = InteractiveBarrier::new();
        let interactive_guard = barrier.acquire_interactive().await;

        let barrier_a = barrier.clone();
        let waiter_a = tokio::spawn(async move {
            let _guard = barrier_a.acquire_runtime().await;
        });
        let barrier_b = barrier.clone();
        let waiter_b = tokio::spawn(async move {
            let _guard = barrier_b.acquire_runtime().await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter_a.is_finished());
        assert!(!waiter_b.is_finished());

        drop(interactive_guard);

        tokio::time::timeout(Duration::from_secs(1), waiter_a)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), waiter_b)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(barrier.snapshot(), (false, 0));
    }

    #[tokio::test]
    async fn test_interactive_barrier_waiter_abort_does_not_leak_state() {
        // MatrixRef: G01,G02 / C12,C13
        let barrier = InteractiveBarrier::new();
        let _hold = barrier.acquire_interactive().await;
        let barrier_c = barrier.clone();
        let waiter = tokio::spawn(async move {
            // This waits for the barrier and is then aborted.
            let _guard = barrier_c.acquire_runtime().await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        waiter.abort();
        let _ = waiter.await;
        assert_eq!(barrier.snapshot(), (true, 0));
    }

    #[tokio::test]
    async fn test_interactive_barrier_wait_timeout_does_not_leak_state() {
        // MatrixRef: G03,G04 / C12,C13
        let barrier = InteractiveBarrier::new();
        let _hold = barrier.acquire_interactive().await;
        let timed =
            tokio::time::timeout(Duration::from_millis(50), barrier.acquire_runtime()).await;
        assert!(timed.is_err(), "runtime acquire should still be waiting");
        assert_eq!(barrier.snapshot(), (true, 0));
    }
}
