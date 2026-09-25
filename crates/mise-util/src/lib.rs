//! Low-level helpers that mise's other crates build on: the environment
//! variables mise reads, the directories derived from them, and path helpers
//! that depend only on those.
//!
//! Nothing here may depend on mise's config system, so any crate can use it.

#[macro_use]
extern crate log;

#[macro_use]
pub mod cmd;
pub mod deps_graph;
pub mod dirs;
pub mod duration;
pub mod env;
pub mod env_diff;
pub mod env_value;
pub mod errors;
pub mod file;
pub mod fuzzy;
pub mod git;
pub mod hash;
pub mod inline_command;
pub mod jobs;
pub mod lock_file;
pub mod netrc;
pub mod packslip_pins;
pub mod path;
pub mod path_env;
pub mod platform;
pub mod progress;
pub mod rand;
pub mod redactions;
pub mod resolve_progress;
pub mod sandbox;
pub mod semver;
pub mod style;
pub mod sysconfig;
pub mod testing;
pub mod time;
pub mod timeout;
pub mod wildcard;
pub mod windows_console;
pub mod windows_posix;
