//! Low-level helpers that mise's other crates build on: the environment
//! variables mise reads, the directories derived from them, and path helpers
//! that depend only on those.
//!
//! Nothing here may depend on mise's config system, so any crate can use it.

#[macro_use]
extern crate log;

#[macro_use]
pub mod cmd;
pub mod dirs;
pub mod env;
pub mod env_diff;
pub mod env_value;
pub mod errors;
pub mod file;
pub mod git;
pub mod hash;
pub mod inline_command;
pub mod lock_file;
pub mod netrc;
pub mod path;
pub mod path_env;
pub mod progress;
pub mod redactions;
pub mod sandbox;
pub mod style;
pub mod testing;
