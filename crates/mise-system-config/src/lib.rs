//! The `mise.toml` declaration types for `[bootstrap]`, `[dotfiles]`, `[dotfile_groups]`, and
//! `[history]`.
//!
//! mise's config layer stores these as parsed, and the system modules interpret them. Keeping
//! the plain data here, below both, is what lets those two layers be separate crates.

pub mod dotfiles;
pub mod history;
pub mod secrets;
