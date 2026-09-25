//! Environment variables mise reads, and the directories derived from them.

use crate::env_diff::{EnvDiff, EnvMap};
use crate::file::replace_path;
use indexmap::IndexMap;
use itertools::Itertools;
use log::LevelFilter;
use std::collections::{HashMap, HashSet};
pub use std::env::*;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::LazyLock as Lazy;
use std::sync::{Mutex, RwLock};

pub static ARGS: RwLock<Vec<String>> = RwLock::new(vec![]);

pub const MISE_INSTALL_VERSION_ENV_VAR: &str = "MISE_INSTALL_VERSION";

pub const MISE_TOOL_VERSION_ENV_VAR: &str = "MISE_TOOL_VERSION";

pub const NON_TOOL_VERSION_ENV_VARS: &[&str] =
    &[MISE_INSTALL_VERSION_ENV_VAR, MISE_TOOL_VERSION_ENV_VAR];

#[cfg(unix)]
pub static SHELL: Lazy<String> = Lazy::new(|| var("SHELL").unwrap_or_else(|_| "sh".into()));

#[cfg(windows)]
pub static SHELL: Lazy<String> = Lazy::new(|| var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));

#[cfg(unix)]
pub static SHELL_COMMAND_FLAG: &str = "-c";

#[cfg(windows)]
pub static SHELL_COMMAND_FLAG: &str = "/c";

pub static HOME: Lazy<PathBuf> = Lazy::new(|| {
    if let Some(home) = crate::testing::home() {
        return home.to_path_buf();
    }
    homedir::my_home()
        .ok()
        .flatten()
        .unwrap_or_else(|| PathBuf::from("/"))
});

pub static EDITOR: Lazy<String> = Lazy::new(|| {
    var("VISUAL")
        .or_else(|_| var("EDITOR"))
        .unwrap_or_else(|_| DEFAULT_EDITOR.to_string())
});

/// The editor to fall back on when neither `VISUAL` nor `EDITOR` is set.
///
/// `nano` everywhere but Windows, which ships none of it — not `nano`, `vi`, `vim`, or anything
/// else POSIX — so the shared default left `mise tasks edit` there with nothing to run at all.
/// `notepad` is the one editor Windows can be relied on to have.
///
/// It also has to *wait*, because `mise dot edit --apply` converges the target as soon as the
/// editor returns. Measured on Windows 11 26200, where `System32\notepad.exe` no longer exists and
/// `notepad` resolves to a zero-byte app-execution alias under `WindowsApps`: spawned the way
/// `Command::status` does it, the parent was still waiting five seconds later, so the alias hands
/// back the real process rather than detaching from it.
#[cfg(windows)]
const DEFAULT_EDITOR: &str = "notepad";

#[cfg(not(windows))]
const DEFAULT_EDITOR: &str = "nano";

#[cfg(target_os = "macos")]
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CACHE_HOME").unwrap_or_else(|| HOME.join("Library/Caches")));

#[cfg(windows)]
pub static XDG_CACHE_HOME: Lazy<PathBuf> = Lazy::new(|| {
    var_path("XDG_CACHE_HOME")
        .or_else(|| var_path("TEMP"))
        .unwrap_or_else(temp_dir)
});

#[cfg(all(not(windows), not(target_os = "macos")))]
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CACHE_HOME").unwrap_or_else(|| HOME.join(".cache")));

pub static XDG_CONFIG_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CONFIG_HOME").unwrap_or_else(|| HOME.join(".config")));

#[cfg(unix)]
pub static XDG_DATA_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_DATA_HOME").unwrap_or_else(|| HOME.join(".local").join("share")));

#[cfg(windows)]
pub static XDG_DATA_HOME: Lazy<PathBuf> = Lazy::new(|| {
    var_path("XDG_DATA_HOME")
        .or(var_path("LOCALAPPDATA"))
        .unwrap_or_else(|| HOME.join("AppData").join("Local"))
});

pub static XDG_STATE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_STATE_HOME").unwrap_or_else(|| HOME.join(".local").join("state")));

/// `%LOCALAPPDATA%`. What the `adrg/xdg` Go package resolves `XDG_CONFIG_HOME` to on Windows,
/// so it is where CLIs built on it — `glab` among them — keep their config.
///
/// Roaming `%APPDATA%` deliberately has no counterpart here. Its only consumer is the `gh`
/// lookup, which has to reproduce go-gh's literal `os.Getenv("AppData")` test — including
/// falling through to `~/.config/gh` when the variable is unset — so a synthesized default
/// would make mise look somewhere gh never would.
///
/// An empty `%LOCALAPPDATA%` falls back rather than resolving to an empty path — see
/// [`var_path`] — which is also what `adrg/xdg` does (`dir != "" && filepath.IsAbs(dir)`).
#[cfg(windows)]
pub static LOCAL_APPDATA: Lazy<PathBuf> =
    Lazy::new(|| var_path("LOCALAPPDATA").unwrap_or_else(|| HOME.join("AppData").join("Local")));

/// control display of "friendly" errors - defaults to release mode behavior unless overridden
pub static MISE_FRIENDLY_ERROR: Lazy<bool> = Lazy::new(|| {
    if var_is_true("MISE_FRIENDLY_ERROR") {
        true
    } else if var_is_false("MISE_FRIENDLY_ERROR") {
        false
    } else {
        // default behavior: friendly in release mode unless debug logging
        !cfg!(debug_assertions) && log::max_level() < log::LevelFilter::Debug
    }
});

pub static MISE_TOOL_STUB: Lazy<bool> =
    Lazy::new(|| ARGS.read().unwrap().get(1).map(|s| s.as_str()) == Some("tool-stub"));

pub static MISE_NO_CONFIG: Lazy<bool> = Lazy::new(|| var_is_true("MISE_NO_CONFIG"));

pub static MISE_NO_ENV: Lazy<bool> = Lazy::new(|| var_is_true("MISE_NO_ENV"));

pub static MISE_NO_HOOKS: Lazy<bool> = Lazy::new(|| var_is_true("MISE_NO_HOOKS"));

pub static MISE_PROGRESS_TRACE: Lazy<bool> = Lazy::new(|| var_is_true("MISE_PROGRESS_TRACE"));

pub static MISE_CACHE_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_CACHE_DIR").unwrap_or_else(|| XDG_CACHE_HOME.join("mise")));

pub static MISE_CONFIG_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_CONFIG_DIR").unwrap_or_else(|| XDG_CONFIG_HOME.join("mise")));

/// The default config directory location (XDG_CONFIG_HOME/mise), used to filter out
/// configs from this location when MISE_CONFIG_DIR is set to a different path
pub static MISE_DEFAULT_CONFIG_DIR: Lazy<PathBuf> = Lazy::new(|| XDG_CONFIG_HOME.join("mise"));

/// True if MISE_CONFIG_DIR was explicitly set to a non-default location
pub static MISE_CONFIG_DIR_OVERRIDDEN: Lazy<bool> = Lazy::new(|| {
    var_path("MISE_CONFIG_DIR").is_some() && *MISE_CONFIG_DIR != *MISE_DEFAULT_CONFIG_DIR
});

pub static MISE_DATA_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_DATA_DIR").unwrap_or_else(|| XDG_DATA_HOME.join("mise")));

pub static MISE_STATE_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_STATE_DIR").unwrap_or_else(|| XDG_STATE_HOME.join("mise")));

pub static MISE_TMP_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_TMP_DIR").unwrap_or_else(|| temp_dir().join("mise")));

pub static MISE_SYSTEM_CONFIG_DIR: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_SYSTEM_CONFIG_DIR")
        .or_else(|| var_path("MISE_SYSTEM_DIR"))
        .unwrap_or_else(|| PathBuf::from("/etc/mise"))
});

pub static MISE_INSTALLS_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_INSTALLS_DIR").unwrap_or_else(|| MISE_DATA_DIR.join("installs")));

pub static MISE_DOWNLOADS_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_DOWNLOADS_DIR").unwrap_or_else(|| MISE_DATA_DIR.join("downloads")));

pub static MISE_PLUGINS_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_PLUGINS_DIR").unwrap_or_else(|| MISE_DATA_DIR.join("plugins")));

pub static MISE_SHIMS_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_SHIMS_DIR").unwrap_or_else(|| MISE_DATA_DIR.join("shims")));

/// System-level data directory (like MISE_DATA_DIR but for system-wide tools).
pub static MISE_SYSTEM_DATA_DIR: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_SYSTEM_DATA_DIR").unwrap_or_else(|| PathBuf::from("/usr/local/share/mise"))
});

/// System-level installs directory, derived from MISE_SYSTEM_DATA_DIR.
pub static MISE_SYSTEM_INSTALLS_DIR: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_SYSTEM_INSTALLS_DIR").unwrap_or_else(|| MISE_SYSTEM_DATA_DIR.join("installs"))
});

/// Extra shared install directories parsed from the environment variable.
/// This is the early/fallback source; prefer `shared_install_dirs()` which also
/// reads from Settings (config files) when available.
pub static MISE_SHARED_INSTALL_DIRS_ENV: Lazy<Vec<PathBuf>> = Lazy::new(|| {
    var_os("MISE_SHARED_INSTALL_DIRS")
        .map(|v| {
            std::env::split_paths(&v)
                .filter(|p| !p.as_os_str().is_empty())
                .map(replace_path)
                .collect()
        })
        .unwrap_or_default()
});

/// Early-boot variant used by install_state::init_tools() before Settings is loaded.
pub fn shared_install_dirs_early() -> Vec<PathBuf> {
    let system = &*MISE_SYSTEM_INSTALLS_DIR;
    let mut result = Vec::new();
    if system.is_dir() && *system != *MISE_INSTALLS_DIR {
        result.push(system.clone());
    }
    result.extend(MISE_SHARED_INSTALL_DIRS_ENV.iter().cloned());
    result
}

pub static MISE_GLOBAL_CONFIG_FILE: Lazy<Option<PathBuf>> =
    Lazy::new(|| var_path("MISE_GLOBAL_CONFIG_FILE").or_else(|| var_path("MISE_CONFIG_FILE")));

pub static MISE_GLOBAL_CONFIG_ROOT: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_GLOBAL_CONFIG_ROOT").unwrap_or_else(|| HOME.to_path_buf()));

pub static MISE_SYSTEM_CONFIG_FILE: Lazy<Option<PathBuf>> =
    Lazy::new(|| var_path("MISE_SYSTEM_CONFIG_FILE"));

pub static MISE_USE_TOML: Lazy<bool> = Lazy::new(|| !var_is_false("MISE_USE_TOML"));

pub static MISE_LIST_ALL_VERSIONS: Lazy<bool> = Lazy::new(|| var_is_true("MISE_LIST_ALL_VERSIONS"));

pub static ARGV0: Lazy<String> = Lazy::new(|| ARGS.read().unwrap()[0].to_string());

pub static MISE_BIN_NAME: Lazy<&str> = Lazy::new(|| filename(&ARGV0));

pub static MISE_LOG_FILE: Lazy<Option<PathBuf>> = Lazy::new(|| var_path("MISE_LOG_FILE"));

pub static MISE_LOG_FILE_LEVEL: Lazy<Option<LevelFilter>> = Lazy::new(log_file_level);

pub fn find_in_tree(base: &Path, rels: &[&[&str]]) -> Option<PathBuf> {
    for rel in rels {
        let mut p = base.to_path_buf();
        for part in *rel {
            p = p.join(part);
        }
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub fn mise_install_base() -> Option<PathBuf> {
    std::fs::canonicalize(&*MISE_BIN)
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
}

pub static MISE_SELF_UPDATE_INSTRUCTIONS: Lazy<Option<PathBuf>> = Lazy::new(|| {
    if let Some(p) = var_path("MISE_SELF_UPDATE_INSTRUCTIONS") {
        return Some(p);
    }
    let base = mise_install_base()?;
    // search lib/, lib/mise/, lib64/mise/
    find_in_tree(
        &base,
        &[
            &["lib", "mise-self-update-instructions.toml"],
            &["lib", "mise", "mise-self-update-instructions.toml"],
            &["lib64", "mise", "mise-self-update-instructions.toml"],
        ],
    )
});

pub static MISE_LOG_HTTP: Lazy<bool> = Lazy::new(|| var_is_true("MISE_LOG_HTTP"));

pub static MISE_LOG_VERBOSE_DEPS: Lazy<bool> = Lazy::new(|| var_is_true("MISE_LOG_VERBOSE_DEPS"));

pub static __USAGE: Lazy<Option<String>> = Lazy::new(|| var("__USAGE").ok());

pub static __MISE_SHIM: Lazy<bool> = Lazy::new(|| var_is_true("__MISE_SHIM"));

/// Absolute path of the shim that delegated to mise. Unlike `MISE_SHIMS_DIR`,
/// this remains reliable when a parent process preserves PATH but filters out
/// mise's directory configuration variables.
pub const MISE_SHIM_PATH_ENV: &str = "__MISE_SHIM_PATH";

pub static MISE_SHIM_PATH: Lazy<RwLock<Option<PathBuf>>> =
    Lazy::new(|| RwLock::new(var_path(MISE_SHIM_PATH_ENV)));

pub static IS_RUNNING_AS_SHIM: Lazy<bool> = Lazy::new(|| {
    // When running tests, always treat as direct mise invocation
    // to avoid interfering with test expectations
    if crate::testing::active() {
        return false;
    }

    // Check if running as tool stub
    if *MISE_TOOL_STUB {
        return true;
    }

    let bin_name = *MISE_BIN_NAME;
    !is_mise_binary(bin_name)
});

/// Returns true if the given binary name refers to mise itself (not a shim).
/// Handles "mise", "mise.exe", "mise.bat", "mise.cmd", "mise-doctor", etc.
///
/// The comparison ignores case on Windows only. Its filesystem does too, so `MISE.EXE` starts the
/// same file as `mise.exe` and reaches `argv[0]` with whatever casing the caller wrote — and a
/// case-sensitive test then sent mise through `handle_shim` against itself. On unix
/// they are two different files, so a shim genuinely named `MISE` has to stay a shim.
pub fn is_mise_binary(bin_name: &str) -> bool {
    let is_mise = |s: &str| {
        if cfg!(windows) {
            s.eq_ignore_ascii_case("mise")
        } else {
            s == "mise"
        }
    };
    // Equivalent to matching "mise" plus the "mise." and "mise-" prefixes, with the case rule
    // applied in one place rather than three.
    is_mise(bin_name)
        || bin_name
            .split_once(['.', '-'])
            .is_some_and(|(stem, _)| is_mise(stem))
}

/// The suffixes `self-replace` gives the copies it makes of the running executable on Windows.
/// `get_temp_executable_name` builds `.{exe stem}.{32 random}{suffix}`.
///
/// This and the predicates below are compiled off Windows too, like
/// `unix_path_to_windows`, so they stay unit-tested everywhere rather than only
/// on the platform that runs them. Their callers are all `#[cfg(windows)]`, hence the allow.
pub const SELF_REPLACE_SUFFIXES: [&str; 2] = [".__selfdelete__.exe", ".__relocated__.exe"];

/// Whether `bin_name` is a copy of *this* executable that `self-replace` made while updating.
///
/// It is mise under a generated name, not a shim — but [`is_mise_binary`] cannot tell, because the
/// name **begins with a `.`**, so `split_once(['.', '-'])` hands back an empty stem and `is_mise("")`
/// is false. mise then walks into `handle_shim` and reports the name as a broken shim, advising the
/// user to reinstall a tool that was never uninstalled.
///
/// Takes the stem rather than reading `current_exe` so it stays pure, and requires it so that a
/// *different* application's orphan — `.othertool.….__selfdelete__.exe` — is not claimed as ours.
///
/// The random segment is checked too, because the caller that acts on this **deletes** the file:
/// anything short of the generated shape is somebody else's and stays where it is.
pub fn is_self_replace_helper(bin_name: &str, exe_stem: &str) -> bool {
    let prefix = format!(".{exe_stem}.");
    // Case-insensitively on Windows, and only for the stem. Measured: `current_exe()` hands back
    // whatever casing the caller used to start the process — `CASEPROBE.EXE` gives a stem of
    // `CASEPROBE` — so an update launched as `MISE.EXE` writes `.MISE.….__selfdelete__.exe`, and
    // the next one launched as `mise.exe` would fail to recognise its own orphan and leave it
    // there for good. The suffix and the random segment are generated by the crate and are always
    // lowercase, so only the stem can vary. Same rule, same reason, as [`is_mise_binary`].
    let matches = bin_name.get(..prefix.len()).is_some_and(|head| {
        if cfg!(windows) {
            head.eq_ignore_ascii_case(&prefix)
        } else {
            head == prefix
        }
    });
    if !matches {
        return false;
    }
    let rest = &bin_name[prefix.len()..];
    SELF_REPLACE_SUFFIXES.iter().any(|suffix| {
        rest.strip_suffix(suffix)
            .is_some_and(is_self_replace_random_segment)
    })
}

/// `self-replace` fills this many characters from `fastrand`'s `lowercase()`:
/// `for _ in 0..32 { file_name.push(rng.lowercase()) }` in `get_temp_executable_name`.
pub const SELF_REPLACE_RANDOM_LEN: usize = 32;

/// Exactly the segment that generator produces — nothing shorter, longer, or outside `a-z`.
///
/// Compares bytes rather than chars on purpose: ASCII bytes cannot appear inside a multi-byte
/// character, so this cannot mis-read a name that is not ASCII to begin with.
fn is_self_replace_random_segment(s: &str) -> bool {
    s.len() == SELF_REPLACE_RANDOM_LEN && s.bytes().all(|b| b.is_ascii_lowercase())
}

/// Whether *this* process is one of those copies, read straight from the OS.
///
/// Deliberately does not go through `ARGS`/`MISE_BIN_NAME`: this answers a question `main` asks
/// before the runtime, logging or config exist, in the same shape as
/// early executable-dispatch paths.
///
/// The stem is not checked, unlike [`is_self_replace_helper`], and it cannot be: the original stem
/// is *inside* the generated name, so a process running under one has nothing left to compare it
/// against. That costs nothing here — a running binary named `.x.….__selfdelete__.exe` was copied
/// from whatever spawned it, and the process asking is mise. Everything around the stem is still
/// required: a leading `.`, then some stem, then the generated random segment and the suffix.
#[cfg(windows)]
pub fn invoked_as_self_replace_helper() -> bool {
    let Some(invoked) = std::env::args_os().next() else {
        return false;
    };
    let Some(name) = Path::new(&invoked).file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with('.')
        && SELF_REPLACE_SUFFIXES.iter().any(|suffix| {
            name.strip_suffix(suffix).is_some_and(|head| {
                head.rsplit_once('.').is_some_and(|(stem, random)| {
                    !stem.is_empty() && is_self_replace_random_segment(random)
                })
            })
        })
}

/// Explicit terminal-width override: `MISE_TERM_WIDTH` takes precedence, then the
/// conventional `COLUMNS`. Lets tables/lists render sanely in CI where terminal
/// size detection returns 0. Honored exactly (no 80 floor) so a narrow width can
/// be forced on purpose. See discussion #4109.
///
/// `None` under mise's unit tests (like `TERM_WIDTH`) so tests that build a table don't pick up
/// a stray `COLUMNS`/`MISE_TERM_WIDTH` from the env.
pub static TERM_WIDTH_OVERRIDE: Lazy<Option<usize>> = Lazy::new(|| {
    if crate::testing::active() {
        return None;
    }
    for key in ["MISE_TERM_WIDTH", "COLUMNS"] {
        if let Some(w) = var(key)
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|w| *w > 0)
        {
            // COLUMNS is maintained by the shell and can leak in unintentionally,
            // so leave a breadcrumb when it (rather than MISE_TERM_WIDTH) is used.
            if key == "COLUMNS" {
                debug!(
                    "overriding terminal width with COLUMNS={w}; set MISE_TERM_WIDTH to control this explicitly"
                );
            }
            return Some(w);
        }
    }
    None
});

pub static TERM_WIDTH: Lazy<usize> = Lazy::new(|| {
    if crate::testing::active() {
        return 80;
    }
    if let Some(w) = *TERM_WIDTH_OVERRIDE {
        return w;
    }
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
        .max(80)
});

/// true if inside a script like bin/exec-env or bin/install
/// used to prevent infinite loops
pub static MISE_BIN: Lazy<PathBuf> = Lazy::new(|| {
    var_path("__MISE_BIN")
        .or_else(|| current_exe().ok())
        .unwrap_or_else(|| "mise".into())
});

pub static MISE_TIMINGS: Lazy<u8> = Lazy::new(|| var_u8("MISE_TIMINGS"));

pub static MISE_PID: Lazy<String> = Lazy::new(|| process::id().to_string());

pub static MISE_JOBS: Lazy<Option<usize>> =
    Lazy::new(|| var("MISE_JOBS").ok().and_then(|v| v.parse::<usize>().ok()));

pub static __MISE_SCRIPT: Lazy<bool> = Lazy::new(|| var_is_true("__MISE_SCRIPT"));

pub static __MISE_ORIG_PATH: Lazy<Option<String>> = Lazy::new(|| var("__MISE_ORIG_PATH").ok());

pub static __MISE_ZSH_PRECMD_RUN: Lazy<bool> = Lazy::new(|| !var_is_false("__MISE_ZSH_PRECMD_RUN"));

pub static OFFLINE: Lazy<bool> = Lazy::new(|| offline(&ARGS.read().unwrap()));

pub static WARN_ON_MISSING_REQUIRED_ENV: Lazy<bool> =
    Lazy::new(|| warn_on_missing_required_env(&ARGS.read().unwrap()));

pub static PATH_KEY: Lazy<String> =
    Lazy::new(|| path_key_from_env(vars_os().filter_map(|(k, _)| k.into_string().ok())));

#[cfg(unix)]
fn path_key_from_env(_keys: impl IntoIterator<Item = String>) -> String {
    "PATH".into()
}

#[cfg(windows)]
fn path_key_from_env(keys: impl IntoIterator<Item = String>) -> String {
    keys.into_iter()
        .find(|k| k.eq_ignore_ascii_case("PATH"))
        .unwrap_or("PATH".into())
}

/// Whether `key` names PATH.
///
/// Windows environment variable names are case-insensitive, so every spelling is the same
/// variable there — including `Path`, which is how Windows itself writes it. On unix only the
/// exact [`PATH_KEY`] is PATH, and a `Path` beside it is a variable of its own.
pub fn is_path_key(key: &str) -> bool {
    if cfg!(windows) {
        key.eq_ignore_ascii_case(&PATH_KEY)
    } else {
        key == *PATH_KEY
    }
}

/// Fold any spelling of PATH onto [`PATH_KEY`], leaving every other name alone.
///
/// mise owns PATH: it writes its own value under `PATH_KEY` after everything else has been
/// collected. A key that means PATH but is spelled differently would survive that write as a
/// second entry, and the two would then both be applied — so it has to be folded before it is
/// stored, not filtered afterwards. The identity on unix, where the only spelling that is PATH
/// is `PATH_KEY` already.
pub fn normalize_path_key(key: String) -> String {
    if is_path_key(&key) {
        PATH_KEY.to_string()
    } else {
        key
    }
}

pub static PATH_NON_PRISTINE: Lazy<Vec<PathBuf>> = Lazy::new(|| match var(&*PATH_KEY) {
    Ok(ref path) => split_paths(path).collect(),
    Err(_) => vec![],
});

pub static DIRENV_DIFF: Lazy<Option<String>> = Lazy::new(|| var("DIRENV_DIFF").ok());

/// GitHub token resolved from environment variables ONLY
/// (`MISE_GITHUB_TOKEN`, `GITHUB_API_TOKEN`, `GITHUB_TOKEN`).
///
/// Intended for subprocess env-var plumbing — passing a token to child processes such as
/// `cargo install` or `ruby-build` that read it themselves.
///
/// **Do not use for mise's own HTTP or sigstore calls.** Use
/// mise's `github::resolve_token_for_api_url` (which walks env vars,
/// `credential_command`, `github_tokens.toml`, gh CLI, and git credentials) or the
/// `github::sigstore` wrapper (which calls it internally). Passing this static
/// to attestation verification is the original cause of the lock-time rate-limit bug.
pub static GITHUB_TOKEN: Lazy<Option<String>> =
    Lazy::new(|| get_token(&["MISE_GITHUB_TOKEN", "GITHUB_API_TOKEN", "GITHUB_TOKEN"]));

pub static MISE_GITHUB_ENTERPRISE_TOKEN: Lazy<Option<String>> =
    Lazy::new(|| get_token(&["MISE_GITHUB_ENTERPRISE_TOKEN"]));

pub static GITLAB_TOKEN: Lazy<Option<String>> =
    Lazy::new(|| get_token(&["MISE_GITLAB_TOKEN", "GITLAB_TOKEN"]));

pub static MISE_GITLAB_ENTERPRISE_TOKEN: Lazy<Option<String>> =
    Lazy::new(|| get_token(&["MISE_GITLAB_ENTERPRISE_TOKEN"]));

pub static TEST_TRANCHE: Lazy<usize> = Lazy::new(|| var_u8("TEST_TRANCHE") as usize);

pub static TEST_TRANCHE_COUNT: Lazy<usize> = Lazy::new(|| var_u8("TEST_TRANCHE_COUNT") as usize);

pub static CLICOLOR_FORCE: Lazy<Option<bool>> =
    Lazy::new(|| var("CLICOLOR_FORCE").ok().map(|v| v != "0"));

pub static CLICOLOR: Lazy<Option<bool>> = Lazy::new(|| {
    if *CLICOLOR_FORCE == Some(true) {
        Some(true)
    } else if *NO_COLOR || var_is_false("MISE_COLOR") {
        Some(false)
    } else if let Ok(v) = var("CLICOLOR") {
        Some(v != "0")
    } else {
        None
    }
});

/// Disable color output - https://no-color.org/
pub static NO_COLOR: Lazy<bool> = Lazy::new(|| var("NO_COLOR").is_ok_and(|v| !v.is_empty()));

/// Force progress bars even in non-TTY (for debugging)
pub static MISE_FORCE_PROGRESS: Lazy<bool> = Lazy::new(|| var_is_true("MISE_FORCE_PROGRESS"));

/// Whether an AI coding agent is driving this process.
pub static AI_AGENT: Lazy<bool> = Lazy::new(mise_agent_env::is_agent);

pub static PYENV_ROOT: Lazy<PathBuf> =
    Lazy::new(|| var_path("PYENV_ROOT").unwrap_or_else(|| HOME.join(".pyenv")));

pub static UV_PYTHON_INSTALL_DIR: Lazy<PathBuf> = Lazy::new(|| {
    var_path("UV_PYTHON_INSTALL_DIR").unwrap_or_else(|| XDG_DATA_HOME.join("uv").join("python"))
});

fn var_u8(key: &str) -> u8 {
    var(key)
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or_default()
}

pub fn var_is_true(key: &str) -> bool {
    match var(key) {
        Ok(v) => {
            let v = v.to_lowercase();
            v == "y" || v == "yes" || v == "true" || v == "1" || v == "on"
        }
        Err(_) => false,
    }
}

pub fn var_is_false(key: &str) -> bool {
    match var(key) {
        Ok(v) => {
            let v = v.to_lowercase();
            v == "n" || v == "no" || v == "false" || v == "0" || v == "off"
        }
        Err(_) => false,
    }
}

pub fn in_home_dir() -> bool {
    current_dir().is_ok_and(|d| d == *HOME)
}

/// The value of `key` as a path, or `None` when it is unset **or empty**.
///
/// An empty value is not a directory. Without this it would yield an empty `PathBuf`, and every
/// caller joins onto the result — producing a *relative* path that gets resolved against the
/// current working directory. `XDG_CONFIG_HOME=` would make `MISE_CONFIG_DIR` the relative
/// `mise`, and a forge CLI lookup read `gh/hosts.yml` out of whatever directory mise happened to
/// be run from. Treating empty as unset is also what the tools mise mirrors here do: go-gh
/// (`os.Getenv(x) != ""`) and `adrg/xdg` (`dir != "" && filepath.IsAbs(dir)`) both fall through.
pub fn var_path(key: &str) -> Option<PathBuf> {
    var_os(key)
        .map(PathBuf::from)
        .map(replace_path)
        .filter(|p| !p.as_os_str().is_empty())
}

fn offline(args: &[String]) -> bool {
    if var_is_true("MISE_OFFLINE") {
        return true;
    }

    args.iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--offline")
}

/// returns true if missing required env vars should produce warnings instead of errors
fn warn_on_missing_required_env(args: &[String]) -> bool {
    // Check if we're running in a command that should warn instead of error
    args.iter()
        .take_while(|a| *a != "--")
        .filter(|a| !a.starts_with('-'))
        .nth(1)
        .map(|a| {
            [
                "hook-env", // Shell activation should not break the shell
            ]
            .contains(&a.as_str())
        })
        .unwrap_or_default()
}

fn log_file_level() -> Option<LevelFilter> {
    let log_level = var("MISE_LOG_FILE_LEVEL").unwrap_or_default();
    log_level.parse::<LevelFilter>().ok()
}

/// The basename of `path` by the host's path grammar, which on Windows means either separator.
///
/// `argv[0]` does not always arrive with `MAIN_SEPARATOR_STR`. libuv hands a Windows process a
/// forward-slash path, which is how Neovim's `jobstart` spawns mise, and splitting on the platform
/// separator left the whole path in [`MISE_BIN_NAME`]. [`is_mise_binary`] then said no and mise ran
/// itself as a shim named after its own path (discussion #11423).
///
/// Deferring to [`Path`] rather than splitting on both separators unconditionally is deliberate:
/// `\` is an ordinary filename character on unix, and splitting there would resolve a shim to a
/// different tool than the one invoked.
fn filename(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

fn get_token(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| var(key).ok())
        .filter(|v| !v.trim().is_empty())
}

pub fn is_activated() -> bool {
    var("__MISE_DIFF").is_ok()
}

pub fn set_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    static MUTEX: Mutex<()> = Mutex::new(());
    let _mutex = MUTEX.lock().unwrap();
    unsafe {
        std::env::set_var(key, value);
    }
}

pub fn remove_var<K: AsRef<OsStr>>(key: K) {
    static MUTEX: Mutex<()> = Mutex::new(());
    let _mutex = MUTEX.lock().unwrap();
    unsafe {
        std::env::remove_var(key);
    }
}

/// Remove the env cache encryption key to force fresh env computation
pub fn reset_env_cache_key() {
    remove_var("__MISE_ENV_CACHE_KEY");
}

/// Safe wrapper around std::env::vars() that handles invalid UTF-8 gracefully.
/// This function uses vars_os() and converts OsString to String, skipping any
/// environment variables that contain invalid UTF-8 sequences.
pub fn vars_safe() -> impl Iterator<Item = (String, String)> {
    vars_os().filter_map(|(k, v)| {
        let k_str = k.to_str()?;
        let v_str = v.to_str()?;
        Some((k_str.to_string(), v_str.to_string()))
    })
}

/// The raw `CMDCMDLINE` a generated Windows `.cmd` launcher was invoked through.
///
/// cmd.exe parses its whole command line before a batch file's `%*` expands, so an argument
/// containing `& ^ | " < >` or `%VAR%` never reaches `%*` intact. The original text does survive
/// in cmd's `CMDCMDLINE` pseudo-variable, but that is not part of the environment a child
/// inherits — measured — so the launcher copies it into this real variable.
pub const LAUNCHER_RAW_CMDLINE_ENV: &str = "__MISE_RAW_CMDLINE";

/// The launcher's own path, so [`recover_launcher_args`] can find where its arguments begin.
pub const LAUNCHER_PATH_ENV: &str = "__MISE_LAUNCHER";

/// Separates the launcher's own command from the caller's arguments.
///
/// The launcher always passes `%*` after this, so a run where the raw line cannot be trusted
/// still gets the arguments cmd managed to deliver rather than none at all.
pub const LAUNCHER_ARGS_SENTINEL: &str = "__MISE_LAUNCHER_ARGS__";

/// Arguments recovered from the launcher's raw command line, or `None` when there are none to
/// recover or the line cannot be shown to be this launcher's.
///
/// Read once and removed from the environment straight away: a task mise runs inherits this
/// process's environment, and a launcher or shim invoked *by* that task would otherwise recover
/// the outer invocation's arguments as its own.
static RECOVERED_LAUNCHER_ARGS: Lazy<Option<Vec<String>>> = Lazy::new(|| {
    let raw = var(LAUNCHER_RAW_CMDLINE_ENV).ok();
    let launcher = var(LAUNCHER_PATH_ENV).ok();
    remove_var(LAUNCHER_RAW_CMDLINE_ENV);
    remove_var(LAUNCHER_PATH_ENV);
    recover_launcher_args(&raw?, &launcher?)
});

/// The argument text `launcher` was called with, taken out of cmd's raw command line.
///
/// Deliberately strict about the shape. Only a line where cmd was spawned *for* this launcher is
/// accepted — `<cmd.exe> /c "" <launcher> " <args>"`, which is what a shell building a native
/// invocation produces. A line that merely mentions the launcher somewhere (a `call` from another
/// batch file, a `cmd /c "<launcher> a & b"` chain typed by hand, an interactive prompt) is
/// declined, because there the arguments were split by the shell before anything mise wrote ran
/// and `%*` is already as good as it gets.
pub fn recover_launcher_args(raw: &str, launcher: &str) -> Option<Vec<String>> {
    let (_, after) = raw.split_once(" /c ").or_else(|| raw.split_once(" /C "))?;
    // cmd's own `/c` argument, then the launcher path quoted inside it.
    let inner = after.strip_prefix('"')?.strip_suffix('"')?;
    let tail = inner
        .strip_prefix('"')?
        .strip_prefix(launcher)?
        .strip_prefix('"')?;
    Some(split_command_line(tail))
}

/// Split the argument section of a Windows command line the way a native program's runtime does.
///
/// The rules are the ones `CommandLineToArgvW` applies past the program name: arguments are
/// separated by whitespace, `"` toggles a quoted run in which whitespace is literal, `2n`
/// backslashes before a `"` are `n` backslashes and a toggle, and `2n+1` are `n` backslashes and
/// a literal `"`. Written out rather than calling the Win32 function so it is testable on every
/// platform — the end-to-end check that it agrees with a real Windows program lives in
/// `e2e-win/task_stub_native_launcher.Tests.ps1`.
pub fn split_command_line(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut started = false;
    let mut backslashes = 0usize;

    fn flush(current: &mut String, backslashes: &mut usize) {
        for _ in 0..*backslashes {
            current.push('\\');
        }
        *backslashes = 0;
    }

    for c in line.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                started = true;
            }
            '"' => {
                for _ in 0..backslashes / 2 {
                    current.push('\\');
                }
                if backslashes % 2 == 1 {
                    current.push('"');
                } else {
                    in_quotes = !in_quotes;
                }
                backslashes = 0;
                // An empty quoted run is still an argument: `""` is one, not none.
                started = true;
            }
            ' ' | '\t' if !in_quotes => {
                flush(&mut current, &mut backslashes);
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                flush(&mut current, &mut backslashes);
                current.push(c);
                started = true;
            }
        }
    }
    flush(&mut current, &mut backslashes);
    if started {
        args.push(current);
    }
    args
}

/// Replace what a Windows launcher forwarded with what it was actually called with.
///
/// Everything up to [`LAUNCHER_ARGS_SENTINEL`] is the launcher's own command and is kept. What
/// follows is cmd's `%*`, used as-is unless the raw command line could be recovered, in which case
/// the recovered arguments take its place.
fn replace_after_sentinel(args: Vec<String>, recovered: Option<&Vec<String>>) -> Vec<String> {
    let Some(at) = args.iter().position(|a| a == LAUNCHER_ARGS_SENTINEL) else {
        return args;
    };
    let mut out = args[..at].to_vec();
    match recovered {
        Some(recovered) => out.extend(recovered.iter().cloned()),
        None => out.extend_from_slice(&args[at + 1..]),
    }
    out
}

fn apply_launcher_args(args: Vec<String>) -> Vec<String> {
    // Forced whatever argv looks like, so the environment variables never outlive this process
    // even when mise was not started by a launcher.
    let recovered = RECOVERED_LAUNCHER_ARGS.as_ref();
    replace_after_sentinel(args, recovered)
}

/// Safe wrapper around std::env::args() that handles invalid UTF-8 gracefully.
/// std::env::args() panics if any argument contains invalid UTF-8; this uses
/// args_os() and lossily converts each argument (invalid sequences become U+FFFD).
/// Unlike vars_safe() the conversion is lossy rather than skipping, so argument
/// positions are preserved and a malformed argv yields a normal "unknown command"
/// error instead of crashing.
pub fn args_safe() -> Vec<String> {
    apply_launcher_args(args_os().map(|a| a.to_string_lossy().to_string()).collect())
}

pub static __MISE_DIFF: Lazy<EnvDiff> = Lazy::new(get_env_diff);

/// essentially, this is whether we show spinners or build output on runtime install
pub static PRISTINE_ENV: Lazy<EnvMap> =
    Lazy::new(|| get_pristine_env(&__MISE_DIFF, vars_safe().collect()));

pub static PATH: Lazy<Vec<PathBuf>> = Lazy::new(|| match PRISTINE_ENV.get(&*PATH_KEY) {
    Some(path) => split_paths(path).collect(),
    None => vec![],
});

fn get_env_diff() -> EnvDiff {
    let env = vars_safe().collect::<HashMap<_, _>>();
    match env.get("__MISE_DIFF") {
        Some(raw) => EnvDiff::deserialize(raw).unwrap_or_else(|err| {
            warn!("Failed to deserialize __MISE_DIFF: {:#}", err);
            EnvDiff::default()
        }),
        None => EnvDiff::default(),
    }
}

/// this returns the environment as if __MISE_DIFF was reversed.
/// putting the shell back into a state before hook-env was run
fn get_pristine_env(mise_diff: &EnvDiff, orig_env: EnvMap) -> EnvMap {
    let mut env = reverse_diff_preserving_overrides(mise_diff, orig_env);

    // get the current path as a vector
    let path = match env.get(&*PATH_KEY) {
        Some(path) => split_paths(path).collect(),
        None => vec![],
    };
    // get the paths that were removed by mise as a hashset
    let mut to_remove = mise_diff.path.iter().collect::<HashSet<_>>();

    // remove those paths that were added by mise, but only once (the first time)
    let path = path
        .into_iter()
        .filter(|p| !to_remove.remove(p))
        .collect_vec();

    // put the pristine PATH back into the environment
    env.insert(
        PATH_KEY.to_string(),
        join_paths(path).unwrap().to_string_lossy().to_string(),
    );
    env
}

/// Reverse values that are still in the state mise recorded, while preserving
/// values changed or removed by the caller after mise applied the environment.
fn reverse_diff_preserving_overrides(mise_diff: &EnvDiff, mut env: EnvMap) -> EnvMap {
    for (key, old_value) in &mise_diff.old {
        match env_diff_get(&mise_diff.new, key) {
            Some(new_value) if env_map_get(&env, key) == Some(new_value) => {
                let key = env_map_key(&env, key)
                    .cloned()
                    .unwrap_or_else(|| key.clone());
                env.insert(key, old_value.clone());
            }
            None if env_map_get(&env, key).is_none() => {
                env.insert(key.clone(), old_value.clone());
            }
            _ => {}
        }
    }

    for (key, new_value) in &mise_diff.new {
        if env_diff_get(&mise_diff.old, key).is_none()
            && env_map_get(&env, key) == Some(new_value)
            && let Some(key) = env_map_key(&env, key).cloned()
        {
            env.remove(&key);
        }
    }

    env
}

#[cfg(not(windows))]
fn env_map_key<'a>(env: &'a EnvMap, key: &str) -> Option<&'a String> {
    env.get_key_value(key).map(|(key, _)| key)
}

#[cfg(windows)]
fn env_map_key<'a>(env: &'a EnvMap, key: &str) -> Option<&'a String> {
    env.keys()
        .find(|candidate| windows_env_key_eq(candidate, key))
}

fn env_map_get<'a>(env: &'a EnvMap, key: &str) -> Option<&'a String> {
    env_map_key(env, key).and_then(|key| env.get(key))
}

#[cfg(not(windows))]
fn env_diff_get<'a>(env: &'a IndexMap<String, String>, key: &str) -> Option<&'a String> {
    env.get(key)
}

#[cfg(windows)]
fn env_diff_get<'a>(env: &'a IndexMap<String, String>, key: &str) -> Option<&'a String> {
    env.iter()
        .find(|(candidate, _)| windows_env_key_eq(candidate, key))
        .map(|(_, value)| value)
}

#[cfg(windows)]
fn windows_env_key_eq(left: &str, right: &str) -> bool {
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    let left = left.encode_utf16().collect::<Vec<_>>();
    let right = right.encode_utf16().collect::<Vec<_>>();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };

    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == CSTR_EQUAL
    }
}

/// Deliberately not `#[cfg(windows)]`: the code under test is pure string handling, and the whole
/// point of writing the splitter out rather than calling `CommandLineToArgvW` was that it can be
/// checked on every platform CI runs.
#[cfg(test)]
mod launcher_args_tests {
    use super::*;

    /// A command line shaped the way a shell builds one when it spawns cmd to run `launcher`.
    fn cmd_line(launcher: &str, tail: &str) -> String {
        format!("C:\\WINDOWS\\system32\\cmd.exe /c \"\"{launcher}\"{tail}\"")
    }

    const LAUNCHER: &str = "C:\\proj\\bin\\hello.cmd";

    #[test]
    fn splits_the_way_a_native_program_would() {
        assert_eq!(split_command_line(" a b c"), ["a", "b", "c"]);
        assert_eq!(split_command_line("  a   b  "), ["a", "b"]);
        assert_eq!(split_command_line(""), Vec::<String>::new());
        assert_eq!(split_command_line("   "), Vec::<String>::new());
        // Whitespace is literal inside quotes, and the quotes themselves are not part of it.
        assert_eq!(split_command_line(" \"m n\""), ["m n"]);
        assert_eq!(split_command_line(" a\"b c\"d"), ["ab cd"]);
        // An empty quoted run is an argument, not nothing.
        assert_eq!(split_command_line(" \"\""), [""]);
        // A tab separates like a space.
        assert_eq!(split_command_line(" a\tb"), ["a", "b"]);
    }

    #[test]
    fn applies_the_backslash_rules() {
        // Backslashes are only special immediately before a quote, which is why a Windows path
        // full of them survives untouched.
        assert_eq!(split_command_line(" C:\\a\\b"), ["C:\\a\\b"]);
        assert_eq!(split_command_line(" trail\\"), ["trail\\"]);
        // `2n` backslashes then `"`: n backslashes, and the quote toggles.
        assert_eq!(split_command_line(" \"a\\\\\"b"), ["a\\b"]);
        // `2n+1`: n backslashes and a literal quote.
        assert_eq!(split_command_line(" q\\\"r"), ["q\"r"]);
        assert_eq!(split_command_line(" q\\\\\\\"r"), ["q\\\"r"]);
    }

    #[test]
    fn recovers_the_arguments_cmd_destroyed() {
        // Every shape measured to reach the task differently through a `%*` launcher.
        for (tail, expected) in [
            (" c&d", vec!["c&d"]),
            (" i^j", vec!["i^j"]),
            (" e%OS%f", vec!["e%OS%f"]),
            (" a>b", vec!["a>b"]),
            (" a<b", vec!["a<b"]),
            (" ^caret", vec!["^caret"]),
            (" a!b", vec!["a!b"]),
            (" \"x y&z\"", vec!["x y&z"]),
            (" a \"b c\" d", vec!["a", "b c", "d"]),
            ("", Vec::<&str>::new()),
        ] {
            let raw = cmd_line(LAUNCHER, tail);
            assert_eq!(
                recover_launcher_args(&raw, LAUNCHER).unwrap(),
                expected,
                "{raw:?}"
            );
        }
    }

    #[test]
    fn declines_a_line_that_is_not_this_launchers_own() {
        // The controls. Accepting any of these would either take arguments that were never meant
        // as one -- the shell had already split them, exactly as it would for a native program --
        // or, worse, let the `exit` in the launcher close a shell mise was not spawned by.
        for raw in [
            // An interactive prompt: no `/c` at all.
            "\"C:\\WINDOWS\\system32\\cmd.exe\"".to_string(),
            // `call` from another batch file: the line is the outer script's.
            "\"cmd.exe\" /c \"C:\\proj\\outer.cmd\"".to_string(),
            // Typed by hand to run the launcher and then something else: the launcher path is not
            // quoted, so it is not the sole thing cmd was given.
            format!("\"cmd.exe\" /c \"{LAUNCHER} foo & echo done\""),
            // A different launcher's line.
            cmd_line("C:\\proj\\bin\\other.cmd", " a"),
            // Truncated or malformed.
            format!("\"cmd.exe\" /c \"\"{LAUNCHER}\" a"),
            format!("\"cmd.exe\" /c {LAUNCHER} a"),
            String::new(),
        ] {
            assert!(recover_launcher_args(&raw, LAUNCHER).is_none(), "{raw:?}");
        }
    }

    #[test]
    fn the_sentinel_marks_where_the_callers_arguments_begin() {
        let argv = |extra: &[&str]| {
            let mut v = vec!["mise".to_string(), "run".to_string(), "hello".to_string()];
            v.push(LAUNCHER_ARGS_SENTINEL.to_string());
            v.extend(extra.iter().map(|s| s.to_string()));
            v
        };
        // Nothing recovered: what cmd delivered is used, and the sentinel is not passed on.
        assert_eq!(
            replace_after_sentinel(argv(&["c"]), None),
            ["mise", "run", "hello", "c"]
        );
        // Recovered: it replaces what cmd delivered rather than adding to it.
        assert_eq!(
            replace_after_sentinel(argv(&["c"]), Some(&vec!["c&d".to_string()])),
            ["mise", "run", "hello", "c&d"]
        );
        // No sentinel at all -- an ordinary mise invocation -- is left exactly as it is.
        let plain = vec!["mise".to_string(), "run".to_string(), "hello".to_string()];
        assert_eq!(
            replace_after_sentinel(plain.clone(), Some(&vec!["nope".to_string()])),
            plain
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_path_key_from_env_uses_uppercase_path_on_unix() {
        assert_eq!(
            path_key_from_env(vec!["path".into(), "HOME".into()]),
            "PATH"
        );
        assert_eq!(
            path_key_from_env(vec!["Path".into(), "HOME".into()]),
            "PATH"
        );
    }
    #[cfg(windows)]
    #[test]
    fn test_path_key_from_env_preserves_windows_path_casing() {
        assert_eq!(
            path_key_from_env(vec!["Path".into(), "TEMP".into()]),
            "Path"
        );
        assert_eq!(
            path_key_from_env(vec!["TEMP".into(), "PATH".into()]),
            "PATH"
        );
        assert_eq!(path_key_from_env(vec!["TEMP".into()]), "PATH");
    }
    #[test]
    fn test_token_overwrite() {
        // Clean up any existing environment variables that might interfere
        remove_var("MISE_GITHUB_TOKEN");
        remove_var("GITHUB_TOKEN");
        remove_var("GITHUB_API_TOKEN");

        set_var("MISE_GITHUB_TOKEN", "");
        set_var("GITHUB_TOKEN", "invalid_token");
        assert_eq!(
            get_token(&["MISE_GITHUB_TOKEN", "GITHUB_TOKEN"]),
            None,
            "Empty token should overwrite other tokens"
        );
        assert_eq!(
            get_token(&["GITHUB_API_TOKEN", "GITHUB_TOKEN"]),
            Some("invalid_token".into()),
            "Unset token should not overwrite other tokens"
        );
        remove_var("MISE_GITHUB_TOKEN");
        remove_var("GITHUB_TOKEN");
        remove_var("GITHUB_API_TOKEN");
    }
    #[test]
    fn test_filename_takes_the_basename() {
        // The reported case: libuv gives a Windows process a forward-slash argv[0], and splitting
        // on MAIN_SEPARATOR_STR left the whole path behind. `/` separates on every platform, so
        // the case the fix is for is pinned everywhere.
        assert_eq!(filename("C:/Users/alice/.cargo/bin/mise.EXE"), "mise.EXE");
        // A unix path was equally unhandled on Windows, since the separator there is `\`.
        assert_eq!(filename("/usr/local/bin/mise"), "mise");
        // A trailing separator used to yield an empty name.
        assert_eq!(filename("/usr/local/bin/mise/"), "mise");
        // Bare names pass through, which is the common case.
        assert_eq!(filename("mise"), "mise");
        assert_eq!(filename("mise.exe"), "mise.exe");
    }
    /// `\` separates path components only on Windows, so the two tests below are a deliberate
    /// platform split rather than duplicated coverage.
    ///
    /// This half is the regression guard: the spelling that always worked has to keep working.
    #[cfg(windows)]
    #[test]
    fn test_filename_splits_on_backslash_on_windows() {
        assert_eq!(filename(r"C:\Users\alice\.cargo\bin\mise.EXE"), "mise.EXE");
        assert_eq!(filename(r"C:\tools\mise\"), "mise");
    }
    /// The other half. On unix `\` is an ordinary filename character, so splitting on it would
    /// resolve a shim to a different tool than the one invoked. `filename` defers to the host
    /// path grammar to avoid that, and this pins the choice.
    #[cfg(unix)]
    #[test]
    fn test_filename_keeps_backslashes_on_unix() {
        assert_eq!(
            filename(r"C:\Users\alice\.cargo\bin\mise.EXE"),
            r"C:\Users\alice\.cargo\bin\mise.EXE"
        );
        assert_eq!(filename(r"/opt/odd/weird\name"), r"weird\name");
    }
    #[test]
    fn test_a_full_path_argv0_is_recognised_as_mise_itself() {
        // What the fix is actually for: `filename` is only interesting because its result feeds
        // `is_mise_binary`, and a false there sends mise into shim mode against its own path.
        for argv0 in [
            "C:/Users/alice/.cargo/bin/mise.EXE",
            "/usr/local/bin/mise",
            "mise",
        ] {
            assert!(
                is_mise_binary(filename(argv0)),
                "argv[0] {argv0:?} should be recognised as mise, not a shim"
            );
        }
        // The backslash spelling only resolves where `\` is a separator; see the pair of
        // `filename` tests above.
        #[cfg(windows)]
        assert!(
            is_mise_binary(filename(r"C:\Users\alice\.cargo\bin\mise.exe")),
            "a backslash argv[0] should be recognised as mise on Windows"
        );
        // The control: a real shim invocation must still be treated as one, or this "fix" would
        // be mise refusing to act as a shim at all.
        for argv0 in ["/home/alice/.local/share/mise/shims/node", "node.exe"] {
            assert!(
                !is_mise_binary(filename(argv0)),
                "argv[0] {argv0:?} should still be a shim"
            );
        }
    }

    #[test]
    fn test_reverse_diff_preserves_runtime_overrides() {
        let diff = EnvDiff {
            old: [
                ("CHANGED".into(), "before".into()),
                ("REMOVED".into(), "before".into()),
            ]
            .into(),
            new: [
                ("ADDED".into(), "managed".into()),
                ("CHANGED".into(), "managed".into()),
            ]
            .into(),
            ..Default::default()
        };
        let current = [
            ("ADDED".into(), "override".into()),
            ("CHANGED".into(), "override".into()),
            ("REMOVED".into(), "override".into()),
        ]
        .into();

        assert_eq!(
            reverse_diff_preserving_overrides(&diff, current),
            [
                ("ADDED".into(), "override".into()),
                ("CHANGED".into(), "override".into()),
                ("REMOVED".into(), "override".into()),
            ]
            .into()
        );
    }
    #[test]
    fn test_reverse_diff_restores_unchanged_managed_values() {
        let diff = EnvDiff {
            old: [
                ("CHANGED".into(), "before".into()),
                ("REMOVED".into(), "before".into()),
            ]
            .into(),
            new: [
                ("ADDED".into(), "managed".into()),
                ("CHANGED".into(), "managed".into()),
            ]
            .into(),
            ..Default::default()
        };
        let current = [
            ("ADDED".into(), "managed".into()),
            ("CHANGED".into(), "managed".into()),
        ]
        .into();

        assert_eq!(
            reverse_diff_preserving_overrides(&diff, current),
            [
                ("CHANGED".into(), "before".into()),
                ("REMOVED".into(), "before".into()),
            ]
            .into()
        );
    }
    #[test]
    fn test_reverse_diff_preserves_runtime_removals() {
        let diff = EnvDiff {
            old: [("CHANGED".into(), "before".into())].into(),
            new: [
                ("ADDED".into(), "managed".into()),
                ("CHANGED".into(), "managed".into()),
            ]
            .into(),
            ..Default::default()
        };

        assert_eq!(
            reverse_diff_preserving_overrides(&diff, EnvMap::new()),
            EnvMap::new()
        );
    }
    #[cfg(windows)]
    #[test]
    fn test_reverse_diff_matches_environment_keys_case_insensitively_on_windows() {
        let diff = EnvDiff {
            old: [
                ("Changed".into(), "before".into()),
                ("MÎSE_FOO".into(), "before-unicode".into()),
            ]
            .into(),
            new: [
                ("Added".into(), "managed".into()),
                ("Changed".into(), "managed".into()),
                ("MÎSE_FOO".into(), "managed-unicode".into()),
            ]
            .into(),
            ..Default::default()
        };
        let current = [
            ("ADDED".into(), "managed".into()),
            ("CHANGED".into(), "managed".into()),
            ("mîse_foo".into(), "managed-unicode".into()),
        ]
        .into();

        assert_eq!(
            reverse_diff_preserving_overrides(&diff, current),
            [
                ("CHANGED".into(), "before".into()),
                ("mîse_foo".into(), "before-unicode".into()),
            ]
            .into()
        );
    }
}
