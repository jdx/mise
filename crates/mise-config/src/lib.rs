//! The `mise.toml` declaration types: plain data parsed from the file, with no templating,
//! settings, or filesystem access. So far this covers `[bootstrap]`, `[dotfiles]`,
//! `[dotfile_groups]`, and `[history]`.
//!
//! mise's config layer stores these as parsed, and the system modules interpret them. Keeping
//! the plain data here, below both, is what lets those two layers be separate crates.

pub mod accounts;
pub mod bootstrap;
pub mod compose;
pub mod defaults;
pub mod dotfiles;
pub mod firewall;
pub mod history;
pub mod launchd;
pub mod login_items;
pub mod managed_files;
pub mod packages;
pub mod remote;
pub mod repos;
pub mod schema;
pub mod secrets;
pub mod services_common;
pub mod systemd;
pub mod templating;
