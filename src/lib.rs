//! mise's core: config, toolsets, backends, tasks and everything else the
//! command line is built on. The `mise` binary (`src/main.rs` and `src/cli/`)
//! is the only intended consumer; this is not a stable public API.

#![allow(unknown_lints)]
#![deny(unreachable_pub)]

use crate::config::SettingsExt;

#[cfg(test)]
#[macro_use]
mod test;

#[cfg(test)]
#[path = "../build/lockfile_rollout.rs"]
mod lockfile_rollout;
#[path = "../build/registry_url.rs"]
mod registry_url;

#[macro_use]
pub mod output;

#[macro_use]
pub mod hint;

#[macro_use]
pub mod timings;

pub mod otel;

#[macro_use]
pub mod cmd;
mod inline_command;

pub mod agecrypt;
pub mod aqua;
pub mod args;
pub mod backend;
pub mod build_time;
pub mod cache;
pub mod config;
pub mod daemons;
pub mod deps;
pub(crate) mod deps_graph;
pub mod direnv;
pub mod dirs;
pub mod duration;
pub mod env;
pub mod env_diff;
pub mod errors;
pub mod exit;
#[cfg_attr(windows, path = "fake_asdf_windows.rs")]
mod fake_asdf;
pub mod file;
pub mod forgejo;
pub mod frontend;
pub mod fuzzy;
pub mod git;
pub mod github;
pub mod github_relay;
pub mod gitlab;
mod gpg;
pub mod hash;
pub mod hook_env;
pub mod hooks;
pub mod http;
pub mod install_before;
pub mod install_context;
pub mod jobs;
pub mod lock_file;
pub mod lockfile;
pub mod logger;
pub(crate) mod maplit;
pub mod migrate;
pub mod minisign;
pub mod oci;
pub mod packslip;
pub mod packslip_pins;
mod packslip_requirements;
mod packslip_stamps;
pub mod parallel;
pub mod path;
pub mod path_env;
pub mod platform;
pub mod plugins;
mod rand;
mod redactions;
pub mod registry;
mod remote_source;
pub(crate) mod result;
pub mod runtime_symlinks;
pub mod sandbox;
pub mod semver;
pub mod shell;
pub mod shims;
mod shorthands;
mod sops;
mod sysconfig;
pub mod system;
#[cfg(unix)]
pub mod system_install;
pub mod task;
pub mod tera;
#[doc(hidden)]
pub mod testing;
pub(crate) mod timeout;
pub mod tokens;
pub mod toml;
pub mod tool_catalog;
pub mod tool_purgatory;
pub mod tool_stub;
pub mod toolset;
pub mod ui;
pub mod upgrade_hint;
mod uv;
pub mod version;
mod versions_host;
pub mod watch_files;
pub mod wildcard;
pub mod windows_console;
#[cfg(windows)]
mod windows_job;
pub mod windows_posix;

pub use crate::exit::request as request_exit;
pub use crate::result::Result;

/// Register what mise's lower crates need from mise itself (its settings loader,
/// build identity, version and config-layer lookups) before anything uses them.
/// Runs first in `main` and in the test harness constructor. The binary also
/// registers its [`frontend`] there.
pub fn register_util_hooks() {
    config::settings::register_loader();
    cache::register_base_cache_keys();
    let shell = env::MISE_SHELL.map(|s| s.to_string()).unwrap_or_default();
    mise_util::user_agent::set(
        format!("mise/{} {shell}", *version::VERSION)
            .trim()
            .to_string(),
    );
    mise_util::deprecation::set_version(env!("CARGO_PKG_VERSION"));
    mise_util::env::set_mise_env(|| env::MISE_ENV.as_slice());
    mise_util::shells::set_implicit_inline_shell(|| {
        config::Settings::get().implicit_inline_shell()
    });
}
