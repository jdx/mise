//! Low-level helpers that mise's other crates build on: the environment
//! variables mise reads, the directories derived from them, and path helpers
//! that depend only on those.
//!
//! Nothing here may depend on mise's config system, so any crate can use it.

#[macro_use]
extern crate log;

#[doc(hidden)]
pub static WARNED_ONCE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(Default::default);

/// Warn once per `id` that something is deprecated, from `warn_at` until it is
/// removed in `remove_at`. See [`deprecation`].
macro_rules! deprecated_at {
    ($warn_at:tt, $remove_at:tt, $id:tt, $($arg:tt)*) => {{
        use versions::Versioning;
        let warn_version = Versioning::new($warn_at).expect("invalid warn_at version in deprecated_at!");
        let remove_version = Versioning::new($remove_at).expect("invalid remove_at version in deprecated_at!");
        let current = $crate::deprecation::version();
        debug_assert!(
            *current < remove_version,
            "Deprecated code [{}] should have been removed in version {}. Please remove this deprecated functionality.",
            $id, $remove_at
        );
        if *current >= warn_version && $crate::deprecation::DEPRECATED.lock().unwrap().insert($id) {
            warn!("deprecated [{}]: {} This will be removed in mise {}.", $id, format!($($arg)*), $remove_at);
        }
    }};
}

/// `warn!`, but only the first time a given message is logged by this crate.
macro_rules! warn_once {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        if $crate::WARNED_ONCE.lock().unwrap().insert(msg.clone()) {
            warn!("{}", msg);
        }
    }};
}

#[macro_use]
pub mod cmd;
pub mod agecrypt;
pub mod args;
pub mod cache;
pub mod cancel;
pub mod deprecation;
pub mod deps_graph;
pub mod dirs;
pub mod duration;
pub mod env;
pub mod env_diff;
pub mod env_value;
pub mod errors;
pub mod exit;
pub mod file;
pub mod forgejo;
pub mod fuzzy;
pub mod git;
pub mod github;
pub mod github_relay;
pub mod gitlab;
pub mod gpg;
pub mod hash;
pub mod http;
pub mod inline_command;
pub mod jobs;
pub mod lock_file;
pub mod netrc;
pub mod network;
pub mod packslip_pins;
pub mod packslip_requirements;
pub mod parallel;
pub mod path;
pub mod path_env;
pub mod platform;
pub mod progress;
pub mod rand;
pub mod redactions;
pub mod remote_source;
pub mod resolve_progress;
pub mod sandbox;
pub mod semver;
pub mod shells;
pub mod style;
pub mod sysconfig;
pub mod tera;
pub mod testing;
pub mod time;
pub mod timeout;
pub mod tokens;
pub mod user_agent;
pub mod versions_host;
pub mod wildcard;
pub mod windows_console;
pub mod windows_posix;
