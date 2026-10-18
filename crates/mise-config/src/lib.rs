//! The `mise.toml` declaration types: plain data parsed from the file, with no templating,
//! settings, or filesystem access. So far this covers `[bootstrap]`, the `[daemons]` sections,
//! `[doctor]`, `[dotfiles]`, `[env]`, `[dotfile_groups]`, `[history]`, `[oci]`, `[tool_config]`,
//! `[vars]`, `[watch_files]`, `[wrappers]`, `[deps]`, `[task_config]`, and the `[tasks]` and
//! `[task_templates]` declarations.
//!
//! mise's config layer stores these as parsed, and the system modules interpret them. Keeping
//! the plain data here, below both, is what lets those two layers be separate crates.

pub mod accounts;
pub mod bootstrap;
pub mod compose;
pub mod daemons;
pub mod de;
pub mod defaults;
pub mod deps;
pub mod doctor;
pub mod dotfiles;
pub mod env;
pub mod firewall;
pub mod history;
pub mod launchd;
pub mod login_items;
pub mod managed_files;
pub mod oci;
pub mod packages;
pub mod remote;
pub mod repos;
pub mod schema;
pub mod secrets;
pub mod services_common;
pub mod systemd;
pub mod task;
pub mod templating;
pub mod tool_config;
pub mod watch_files;
pub mod wrappers;
