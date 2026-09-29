//! Resource planning for declarative mise bootstrap.
//!
//! Resource models and graph validation are independent of host inspection;
//! bootstrap-specific ordering rules live in `policy`.

mod plan;
mod policy;
mod resource;
mod state;

pub use plan::{BootstrapPlan, BootstrapPlanOutput, PlanSummary};
pub use resource::{ResourceAction, ResourceId, ResourceOrigin, ResourcePlan, serialize_path};
pub use state::{AccountState, ManagedFilePhase, ManagedState};
