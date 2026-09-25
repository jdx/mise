//! Low-level helpers that mise's other crates build on: the environment
//! variables mise reads, the directories derived from them, and path helpers
//! that depend only on those.
//!
//! Nothing here may depend on mise's config system, so any crate can use it.

#[macro_use]
extern crate log;

pub mod dirs;
pub mod env;
pub mod file;
pub mod testing;
