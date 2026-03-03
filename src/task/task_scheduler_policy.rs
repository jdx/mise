//! Pure admission policy for task scheduling.
//!
//! This module is intentionally side-effect free: it maps an immutable scheduler
//! snapshot + task probe into a deterministic decision.

use crate::task::Task;
use crate::task::task_helpers::{task_is_interactive_barrier_runtime, task_needs_permit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnClass {
    InteractiveRuntime,
    Runtime,
    NonRuntime,
}

impl SpawnClass {
    pub fn is_runtime(self) -> bool {
        matches!(self, Self::InteractiveRuntime | Self::Runtime)
    }

    pub fn requires_interactive_barrier(self) -> bool {
        matches!(self, Self::InteractiveRuntime)
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerDecision {
    Start,
    WaitBarrier,
    WaitPermit,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionAction {
    Drop,
    WaitBarrier,
    WaitPermit,
    ClaimInteractiveBarrier,
    AcquirePermit,
    Start,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermitState {
    Missing,
    Held,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierState {
    Missing,
    Held,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionResources {
    NonRuntime,
    Runtime {
        permit: PermitState,
    },
    InteractiveRuntime {
        barrier: BarrierState,
        permit: PermitState,
    },
}

impl AdmissionResources {
    pub fn class(self) -> SpawnClass {
        match self {
            Self::NonRuntime => SpawnClass::NonRuntime,
            Self::Runtime { .. } => SpawnClass::Runtime,
            Self::InteractiveRuntime { .. } => SpawnClass::InteractiveRuntime,
        }
    }

    pub fn has_permit(self) -> bool {
        match self {
            Self::NonRuntime => false,
            Self::Runtime { permit } => permit == PermitState::Held,
            Self::InteractiveRuntime { permit, .. } => permit == PermitState::Held,
        }
    }

    pub fn has_interactive_guard(self) -> bool {
        match self {
            Self::InteractiveRuntime { barrier, .. } => barrier == BarrierState::Held,
            Self::Runtime { .. } | Self::NonRuntime => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerSnapshot {
    pub continue_on_error: bool,
    pub stopping: bool,
    pub runtime_in_flight: usize,
    pub runtime_in_flight_for_owner: usize,
    pub foreign_runtime_owners_without_pending_interactive: usize,
    pub active_interactive_owner: Option<u64>,
    pub pending_interactive_head_seq: Option<u64>,
    pub pending_interactive_min_seq: Option<u64>,
    pub pending_permit_seq: Option<u64>,
    pub permits_available: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionProbe {
    pub resources: AdmissionResources,
    pub owner: Option<u64>,
    pub seq: u64,
    pub runnable_post_dep: bool,
}

/// Matrix: B01/B02/B05/B07/B08/B11/B13 (C1/C3/C10/C11)
pub fn classify_spawn_class(task: &Task) -> SpawnClass {
    if task_is_interactive_barrier_runtime(task) {
        SpawnClass::InteractiveRuntime
    } else if task_needs_permit(task) {
        SpawnClass::Runtime
    } else {
        SpawnClass::NonRuntime
    }
}

/// Matrix: B14/B15/B16/B17/B18/F14 (C5, C13)
pub fn should_skip_spawn_when_stopping(
    stopping: bool,
    continue_on_error: bool,
    runnable_post_dep: bool,
) -> bool {
    stopping && !continue_on_error && !runnable_post_dep
}

fn is_blocked_by_interactive_owner(active_owner: Option<u64>, task_owner: Option<u64>) -> bool {
    matches!(active_owner, Some(owner) if Some(owner) != task_owner)
}

fn runtime_blocked_by_pending_interactive(
    pending_interactive_min_seq: Option<u64>,
    seq: u64,
) -> bool {
    pending_interactive_min_seq.is_some_and(|pending| seq > pending)
}

fn blocked_by_permit_queue_head(pending_permit_seq: Option<u64>, seq: u64) -> bool {
    pending_permit_seq.is_some() && pending_permit_seq != Some(seq)
}

fn can_break_foreign_runtime_wait_cycle(
    snapshot: SchedulerSnapshot,
    owner: Option<u64>,
    seq: u64,
) -> bool {
    owner.is_some()
        && snapshot.runtime_in_flight_for_owner > 0
        && snapshot.active_interactive_owner.is_none()
        && snapshot.pending_interactive_head_seq == Some(seq)
        && snapshot.foreign_runtime_owners_without_pending_interactive == 0
}

fn blocked_by_foreign_runtime(snapshot: SchedulerSnapshot, owner: Option<u64>, seq: u64) -> bool {
    let foreign_runtime_in_flight = if owner.is_some() {
        snapshot.runtime_in_flight > snapshot.runtime_in_flight_for_owner
    } else {
        snapshot.runtime_in_flight > 0
    };
    foreign_runtime_in_flight && !can_break_foreign_runtime_wait_cycle(snapshot, owner, seq)
}

fn owner_uses_active_runtime_slot(snapshot: SchedulerSnapshot, owner: Option<u64>) -> bool {
    owner.is_some()
        && snapshot.active_interactive_owner == owner
        && snapshot.runtime_in_flight_for_owner > 0
}

fn interactive_owner_holds_barrier(
    snapshot: SchedulerSnapshot,
    resources: AdmissionResources,
    owner: Option<u64>,
) -> bool {
    resources.class().requires_interactive_barrier()
        && resources.has_interactive_guard()
        && owner.is_some()
        && snapshot.active_interactive_owner == owner
}

#[cfg(test)]
pub fn decide_ready(
    snapshot: SchedulerSnapshot,
    class: SpawnClass,
    owner: Option<u64>,
    seq: u64,
    runnable_post_dep: bool,
) -> SchedulerDecision {
    if should_skip_spawn_when_stopping(
        snapshot.stopping,
        snapshot.continue_on_error,
        runnable_post_dep,
    ) {
        return SchedulerDecision::Drop;
    }

    if class.requires_interactive_barrier() {
        if blocked_by_foreign_runtime(snapshot, owner, seq)
            || is_blocked_by_interactive_owner(snapshot.active_interactive_owner, owner)
            || (snapshot.active_interactive_owner.is_none()
                && snapshot.pending_interactive_head_seq.is_some()
                && snapshot.pending_interactive_head_seq != Some(seq))
        {
            SchedulerDecision::WaitBarrier
        } else if class.is_runtime() && owner_uses_active_runtime_slot(snapshot, owner) {
            SchedulerDecision::Start
        } else if blocked_by_permit_queue_head(snapshot.pending_permit_seq, seq) {
            SchedulerDecision::WaitBarrier
        } else if class.is_runtime() && snapshot.permits_available == 0 {
            SchedulerDecision::WaitPermit
        } else {
            SchedulerDecision::Start
        }
    } else if class.is_runtime() {
        if is_blocked_by_interactive_owner(snapshot.active_interactive_owner, owner)
            || (snapshot.active_interactive_owner.is_none()
                && runtime_blocked_by_pending_interactive(
                    snapshot.pending_interactive_min_seq,
                    seq,
                ))
        {
            SchedulerDecision::WaitBarrier
        } else if owner_uses_active_runtime_slot(snapshot, owner) {
            SchedulerDecision::Start
        } else if blocked_by_permit_queue_head(snapshot.pending_permit_seq, seq) {
            SchedulerDecision::WaitBarrier
        } else if snapshot.permits_available == 0 {
            SchedulerDecision::WaitPermit
        } else {
            SchedulerDecision::Start
        }
    } else {
        SchedulerDecision::Start
    }
}

pub fn decide_admission(snapshot: SchedulerSnapshot, probe: AdmissionProbe) -> AdmissionAction {
    let class = probe.resources.class();
    if should_skip_spawn_when_stopping(
        snapshot.stopping,
        snapshot.continue_on_error,
        probe.runnable_post_dep,
    ) {
        return AdmissionAction::Drop;
    }

    if class.requires_interactive_barrier() && !probe.resources.has_interactive_guard() {
        if blocked_by_foreign_runtime(snapshot, probe.owner, probe.seq) {
            return AdmissionAction::WaitBarrier;
        }
        if is_blocked_by_interactive_owner(snapshot.active_interactive_owner, probe.owner) {
            return AdmissionAction::WaitBarrier;
        }
        if snapshot.active_interactive_owner.is_none() {
            if snapshot.pending_interactive_head_seq.is_some()
                && snapshot.pending_interactive_head_seq != Some(probe.seq)
            {
                return AdmissionAction::WaitBarrier;
            }
            return AdmissionAction::ClaimInteractiveBarrier;
        }
    }

    if class == SpawnClass::Runtime
        && (is_blocked_by_interactive_owner(snapshot.active_interactive_owner, probe.owner)
            || (snapshot.active_interactive_owner.is_none()
                && runtime_blocked_by_pending_interactive(
                    snapshot.pending_interactive_min_seq,
                    probe.seq,
                )))
    {
        return AdmissionAction::WaitBarrier;
    }

    if class.is_runtime() && owner_uses_active_runtime_slot(snapshot, probe.owner) {
        // Owned runtime/interactive tasks execute within the active interactive session slot.
        return AdmissionAction::Start;
    }

    let bypass_permit_head =
        interactive_owner_holds_barrier(snapshot, probe.resources, probe.owner);
    if class.is_runtime()
        && !bypass_permit_head
        && blocked_by_permit_queue_head(snapshot.pending_permit_seq, probe.seq)
    {
        return AdmissionAction::WaitBarrier;
    }

    if class.is_runtime() && !probe.resources.has_permit() {
        if snapshot.permits_available == 0 {
            return AdmissionAction::WaitPermit;
        }
        return AdmissionAction::AcquirePermit;
    }

    AdmissionAction::Start
}

#[cfg(test)]
mod tests {
    use super::{
        AdmissionAction, AdmissionProbe, AdmissionResources, BarrierState, PermitState,
        SchedulerDecision, SchedulerSnapshot, SpawnClass, classify_spawn_class, decide_admission,
        decide_ready, should_skip_spawn_when_stopping,
    };
    use crate::task::{RunEntry, Task};

    fn script_task(name: &str) -> Task {
        Task {
            name: name.to_string(),
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        }
    }

    fn snapshot() -> SchedulerSnapshot {
        SchedulerSnapshot {
            continue_on_error: true,
            stopping: false,
            runtime_in_flight: 0,
            runtime_in_flight_for_owner: 0,
            foreign_runtime_owners_without_pending_interactive: 0,
            active_interactive_owner: None,
            pending_interactive_head_seq: None,
            pending_interactive_min_seq: None,
            pending_permit_seq: None,
            permits_available: 1,
        }
    }

    fn interactive_probe(
        owner: Option<u64>,
        seq: u64,
        runnable_post_dep: bool,
        barrier: BarrierState,
        permit: PermitState,
    ) -> AdmissionProbe {
        AdmissionProbe {
            resources: AdmissionResources::InteractiveRuntime { barrier, permit },
            owner,
            seq,
            runnable_post_dep,
        }
    }

    fn runtime_probe(
        owner: Option<u64>,
        seq: u64,
        runnable_post_dep: bool,
        permit: PermitState,
    ) -> AdmissionProbe {
        AdmissionProbe {
            resources: AdmissionResources::Runtime { permit },
            owner,
            seq,
            runnable_post_dep,
        }
    }

    fn non_runtime_probe(seq: u64, runnable_post_dep: bool) -> AdmissionProbe {
        AdmissionProbe {
            resources: AdmissionResources::NonRuntime,
            owner: None,
            seq,
            runnable_post_dep,
        }
    }

    #[test]
    fn classify_spawn_class_interactive_runtime() {
        let mut task = script_task("interactive");
        task.interactive = true;
        assert_eq!(classify_spawn_class(&task), SpawnClass::InteractiveRuntime);
    }

    #[test]
    fn classify_spawn_class_runtime_non_interactive() {
        let task = script_task("runtime");
        assert_eq!(classify_spawn_class(&task), SpawnClass::Runtime);
    }

    #[test]
    fn classify_spawn_class_non_runtime_orchestrator() {
        let task = Task {
            run: vec![RunEntry::SingleTask {
                task: "other".to_string(),
            }],
            ..Default::default()
        };
        assert_eq!(classify_spawn_class(&task), SpawnClass::NonRuntime);
    }

    #[test]
    fn classify_spawn_class_mixed_runtime_interactive_uses_interactive_barrier() {
        let task = Task {
            interactive: true,
            run: vec![
                RunEntry::Script("echo hi".to_string()),
                RunEntry::SingleTask {
                    task: "other".to_string(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(classify_spawn_class(&task), SpawnClass::InteractiveRuntime);
    }

    #[test]
    fn should_skip_spawn_when_stopping_matrix() {
        assert!(should_skip_spawn_when_stopping(true, false, false));
        assert!(!should_skip_spawn_when_stopping(true, true, false));
        assert!(!should_skip_spawn_when_stopping(true, false, true));
        assert!(!should_skip_spawn_when_stopping(false, false, false));
    }

    #[test]
    fn ready_decision_runtime_waits_when_pending_interactive_has_earlier_seq() {
        let mut s = snapshot();
        s.pending_interactive_head_seq = Some(1);
        s.pending_interactive_min_seq = Some(1);
        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 2, true),
            SchedulerDecision::WaitBarrier
        );

        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 1, true),
            SchedulerDecision::Start
        );

        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 0, true),
            SchedulerDecision::Start
        );
    }

    #[test]
    fn ready_decision_runtime_waits_for_permit_queue_head() {
        let mut s = snapshot();
        s.pending_permit_seq = Some(1);
        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 2, true),
            SchedulerDecision::WaitBarrier
        );
        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 1, true),
            SchedulerDecision::Start
        );
    }

    #[test]
    fn ready_decision_waits_on_active_interactive_then_permit() {
        let mut s = snapshot();
        s.active_interactive_owner = Some(42);
        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 1, true),
            SchedulerDecision::WaitBarrier
        );

        s.active_interactive_owner = None;
        s.permits_available = 0;
        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 1, true),
            SchedulerDecision::WaitPermit
        );
    }

    #[test]
    fn ready_decision_runtime_with_same_owner_does_not_wait_on_barrier() {
        let mut s = snapshot();
        s.active_interactive_owner = Some(7);
        s.runtime_in_flight = 1;
        s.runtime_in_flight_for_owner = 1;
        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, Some(7), 2, true),
            SchedulerDecision::Start
        );
    }

    #[test]
    fn ready_decision_interactive_with_only_same_owner_runtime_can_start() {
        let mut s = snapshot();
        s.runtime_in_flight = 1;
        s.runtime_in_flight_for_owner = 1;
        assert_eq!(
            decide_ready(s, SpawnClass::InteractiveRuntime, Some(7), 3, true),
            SchedulerDecision::Start
        );
    }

    #[test]
    fn ready_decision_drops_non_post_dep_when_stopping() {
        let mut s = snapshot();
        s.continue_on_error = false;
        s.stopping = true;

        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 1, false),
            SchedulerDecision::Drop
        );
        assert_eq!(
            decide_ready(s, SpawnClass::Runtime, None, 1, true),
            SchedulerDecision::Start
        );
    }

    #[test]
    fn admission_decision_interactive_waits_until_its_seq_is_head_then_claims() {
        let mut s = snapshot();
        s.pending_interactive_head_seq = Some(1);
        s.pending_interactive_min_seq = Some(1);

        assert_eq!(
            decide_admission(
                s,
                interactive_probe(
                    Some(9),
                    2,
                    true,
                    BarrierState::Missing,
                    PermitState::Missing,
                )
            ),
            AdmissionAction::WaitBarrier
        );

        assert_eq!(
            decide_admission(
                s,
                interactive_probe(
                    Some(9),
                    1,
                    true,
                    BarrierState::Missing,
                    PermitState::Missing,
                )
            ),
            AdmissionAction::ClaimInteractiveBarrier
        );
    }

    #[test]
    fn admission_decision_interactive_claim_then_permit_then_start() {
        let mut s = snapshot();
        s.pending_interactive_head_seq = Some(1);
        s.pending_interactive_min_seq = Some(1);
        assert_eq!(
            decide_admission(
                s,
                interactive_probe(
                    Some(1),
                    1,
                    true,
                    BarrierState::Missing,
                    PermitState::Missing,
                )
            ),
            AdmissionAction::ClaimInteractiveBarrier
        );

        s.active_interactive_owner = Some(1);
        assert_eq!(
            decide_admission(
                s,
                interactive_probe(Some(1), 1, true, BarrierState::Held, PermitState::Missing,)
            ),
            AdmissionAction::AcquirePermit
        );

        s.permits_available = 0;
        assert_eq!(
            decide_admission(
                s,
                interactive_probe(Some(1), 1, true, BarrierState::Held, PermitState::Missing,)
            ),
            AdmissionAction::WaitPermit
        );

        assert_eq!(
            decide_admission(
                s,
                interactive_probe(Some(1), 1, true, BarrierState::Held, PermitState::Held)
            ),
            AdmissionAction::Start
        );
    }

    #[test]
    fn admission_decision_runtime_waits_while_other_interactive_owner_running() {
        let mut s = snapshot();
        s.active_interactive_owner = Some(9);

        assert_eq!(
            decide_admission(s, runtime_probe(None, 2, true, PermitState::Missing)),
            AdmissionAction::WaitBarrier
        );
    }

    #[test]
    fn admission_decision_runtime_waits_while_pending_interactive_has_earlier_seq() {
        let mut s = snapshot();
        s.pending_interactive_head_seq = Some(5);
        s.pending_interactive_min_seq = Some(5);

        assert_eq!(
            decide_admission(s, runtime_probe(None, 6, true, PermitState::Missing)),
            AdmissionAction::WaitBarrier
        );

        assert_eq!(
            decide_admission(s, runtime_probe(None, 5, true, PermitState::Missing)),
            AdmissionAction::AcquirePermit
        );
    }

    #[test]
    fn admission_decision_runtime_waits_for_permit_queue_head() {
        let mut s = snapshot();
        s.pending_permit_seq = Some(1);
        assert_eq!(
            decide_admission(s, runtime_probe(None, 2, true, PermitState::Missing)),
            AdmissionAction::WaitBarrier
        );
    }

    #[test]
    fn admission_decision_runtime_same_owner_can_start_while_barrier_active() {
        let mut s = snapshot();
        s.active_interactive_owner = Some(9);
        s.runtime_in_flight = 1;
        s.runtime_in_flight_for_owner = 1;

        assert_eq!(
            decide_admission(s, runtime_probe(Some(9), 10, true, PermitState::Missing)),
            AdmissionAction::Start
        );
    }

    #[test]
    fn admission_decision_interactive_waits_when_foreign_runtime_is_in_flight() {
        let mut s = snapshot();
        s.runtime_in_flight = 2;
        s.runtime_in_flight_for_owner = 1;
        assert_eq!(
            decide_admission(
                s,
                interactive_probe(
                    Some(11),
                    3,
                    true,
                    BarrierState::Missing,
                    PermitState::Missing,
                )
            ),
            AdmissionAction::WaitBarrier
        );
    }

    #[test]
    fn admission_decision_interactive_same_owner_reuses_active_runtime_slot_without_permit() {
        let mut s = snapshot();
        s.active_interactive_owner = Some(5);
        s.runtime_in_flight = 1;
        s.runtime_in_flight_for_owner = 1;
        s.permits_available = 0;
        assert_eq!(
            decide_admission(
                s,
                interactive_probe(Some(5), 7, true, BarrierState::Held, PermitState::Missing)
            ),
            AdmissionAction::Start
        );
    }

    #[test]
    fn admission_decision_interactive_head_can_break_wait_cycle_with_foreign_waiting_runtimes() {
        // Deadlock breaker: all foreign runtime owners are also pending interactives.
        let mut s = snapshot();
        s.runtime_in_flight = 2;
        s.runtime_in_flight_for_owner = 1;
        s.pending_interactive_head_seq = Some(7);
        s.pending_interactive_min_seq = Some(7);
        s.foreign_runtime_owners_without_pending_interactive = 0;
        assert_eq!(
            decide_admission(
                s,
                interactive_probe(
                    Some(5),
                    7,
                    true,
                    BarrierState::Missing,
                    PermitState::Missing,
                )
            ),
            AdmissionAction::ClaimInteractiveBarrier
        );
    }

    #[test]
    fn admission_decision_interactive_head_must_not_break_without_owned_runtime_in_flight() {
        // Guard: only break cycles for owners that already hold an in-flight runtime slot.
        // Otherwise an interactive can claim the barrier too early and deadlock on permit order.
        let mut s = snapshot();
        s.runtime_in_flight = 2;
        s.runtime_in_flight_for_owner = 0;
        s.pending_interactive_head_seq = Some(7);
        s.pending_interactive_min_seq = Some(7);
        s.foreign_runtime_owners_without_pending_interactive = 0;
        assert_eq!(
            decide_admission(
                s,
                interactive_probe(
                    Some(5),
                    7,
                    true,
                    BarrierState::Missing,
                    PermitState::Missing,
                )
            ),
            AdmissionAction::WaitBarrier
        );
    }

    #[test]
    fn admission_decision_interactive_barrier_holder_bypasses_permit_head_ordering() {
        // Regression: permit FIFO by seq can conflict with interactive tie-break order.
        // Once an interactive owns the barrier, it must not be blocked by an earlier
        // permit queued for another owner.
        let mut s = snapshot();
        s.active_interactive_owner = Some(2);
        s.pending_permit_seq = Some(5);
        assert_eq!(
            decide_admission(
                s,
                interactive_probe(Some(2), 6, true, BarrierState::Held, PermitState::Missing)
            ),
            AdmissionAction::AcquirePermit
        );
    }

    #[test]
    fn admission_decision_drop_when_stopping_non_post_dep() {
        let mut s = snapshot();
        s.continue_on_error = false;
        s.stopping = true;
        assert_eq!(
            decide_admission(s, runtime_probe(None, 1, false, PermitState::Missing)),
            AdmissionAction::Drop
        );
    }

    #[test]
    fn admission_decision_non_runtime_starts_immediately() {
        let mut s = snapshot();
        s.active_interactive_owner = Some(9);
        s.pending_interactive_head_seq = Some(1);
        s.pending_interactive_min_seq = Some(1);
        s.permits_available = 0;
        assert_eq!(
            decide_admission(s, non_runtime_probe(99, true)),
            AdmissionAction::Start
        );
    }
}
