//! Daemon declarations, ports, presets, and the runtime live in `mise-daemons`; this keeps the
//! paths mise's other modules use and adds the pieces that drive mise's config and toolsets.

pub use mise_daemons::*;

pub mod glue;
#[cfg(test)]
mod glue_tests;
pub mod hook_env;
pub mod provider_commands;
pub mod tasks;
