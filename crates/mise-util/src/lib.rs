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
pub mod file;
pub mod hash;
pub mod lock_file;
pub mod path;
pub mod progress;
pub mod style;
pub mod testing;
