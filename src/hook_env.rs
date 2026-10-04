use std::io::prelude::*;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::BTreeSet, sync::Arc};

use base64::prelude::*;
use eyre::Result;
use flate2::Compression;
use flate2::write::{ZlibDecoder, ZlibEncoder};
use indexmap::IndexSet;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock as Lazy;

use crate::config::{Config, DEFAULT_CONFIG_FILENAMES, Settings, config_file};
use crate::env::PATH_KEY;
use crate::env_diff::{
    ENV_STATE_VERSION, EnvDiffOperation, EnvDiffPatches, EnvMap, hash_env_value,
    legacy_env_state_version,
};
use crate::hash::hash_to_str;
use crate::shell::Shell;
use crate::{dirs, duration, env, file, hooks, watch_files};

/// Why the shell hook ran: before a prompt, or after a directory change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, usage_rs::ValueEnum)]
#[usage(rename_all = "lowercase")]
pub enum HookReason {
    Precmd,
    Chpwd,
}

/// Directory to store per-directory last check timestamps.
/// Timestamps are stored per-directory (using a hash of CWD) so that
/// multiple shells in different directories don't interfere with each other.
static LAST_CHECK_DIR: Lazy<PathBuf> = Lazy::new(|| dirs::STATE.join("hook-env-checks"));
const LAST_UNTRUSTED_CONFIG_WARNING_KEY_ENV: &str = "__MISE_LAST_UNTRUSTED_CONFIG_WARNING_KEY";

/// Get the path to the last check file for a specific directory.
fn last_check_file_for_dir(dir: &Path) -> PathBuf {
    let hash = hash_to_str(&dir.to_string_lossy());
    LAST_CHECK_DIR.join(hash)
}

/// Read the last full check timestamp from the state file for the current directory.
fn read_last_full_check() -> u128 {
    let Some(cwd) = &*dirs::CWD else {
        return 0;
    };
    std::fs::read_to_string(last_check_file_for_dir(cwd))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Write the last full check timestamp to the state file for the current directory.
fn write_last_full_check(timestamp: u128) {
    let Some(cwd) = &*dirs::CWD else {
        return;
    };
    if let Err(e) = file::create_dir_all(&*LAST_CHECK_DIR) {
        trace!("failed to create last check dir: {e}");
        return;
    }
    if let Err(e) = std::fs::write(last_check_file_for_dir(cwd), timestamp.to_string()) {
        trace!("failed to write last check file: {e}");
    }
}

/// Set when [`should_exit_early_fast`] determines a full hook-env run is
/// required because something looks stale.
///
/// [`should_exit_early`] consults this so a run the fast path forced always
/// reaches [`build_session`], which is what rewrites `latest_update`. The two
/// checks are not identical — the fast path also compares config-search
/// directory mtimes, which the slow path has no equivalent for — so without
/// this the slow path could exit early on a run the fast path forced, leaving
/// `latest_update` stale and forcing another full run on the next prompt,
/// forever.
///
/// Both functions run in the same process for a given `mise hook-env`:
/// `should_exit_early_fast` from `cli::run` before config is loaded, and
/// `should_exit_early` from `cli::hook_env::HookEnv::run` after.
static FAST_PATH_FORCED_FULL_RUN: AtomicBool = AtomicBool::new(false);

/// Record that the fast path requires a full run and return `false`, so call
/// sites can `return force_full_run();` in place of a bare `return false`.
fn force_full_run() -> bool {
    FAST_PATH_FORCED_FULL_RUN.store(true, Ordering::Relaxed);
    false
}

/// Convert a SystemTime to milliseconds since Unix epoch
fn mtime_to_millis(mtime: SystemTime) -> u128 {
    mtime
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn should_show_untrusted_config_warning(config_paths: &[PathBuf]) -> bool {
    env::var(LAST_UNTRUSTED_CONFIG_WARNING_KEY_ENV).unwrap_or_default()
        != current_untrusted_warning_key(config_paths)
}

pub fn mark_untrusted_config_warning_seen(
    shell: &dyn Shell,
    config_paths: &[PathBuf],
) -> Result<()> {
    miseprint!(
        "{}",
        shell.set_env(
            LAST_UNTRUSTED_CONFIG_WARNING_KEY_ENV,
            &current_untrusted_warning_key(config_paths)
        )
    )?;
    Ok(())
}

pub fn clear_untrusted_config_warning(patches: &mut EnvDiffPatches) {
    if has_untrusted_config_warning_marker() {
        patches.push(EnvDiffOperation::Remove(
            LAST_UNTRUSTED_CONFIG_WARNING_KEY_ENV.into(),
        ));
    }
}

fn has_untrusted_config_warning_marker() -> bool {
    env::var(LAST_UNTRUSTED_CONFIG_WARNING_KEY_ENV).is_ok_and(|key| !key.is_empty())
}

/// Keyed on every skipped config, so a newly added or edited untrusted file in
/// the same directory warns again even when the first one is unchanged.
fn current_untrusted_warning_key(config_paths: &[PathBuf]) -> String {
    let cwd = dirs::CWD
        .as_ref()
        .map(|p| canonical_path_key(p))
        .unwrap_or_default();
    let configs = config_paths
        .iter()
        .map(|path| {
            let trust_root = canonical_path_key(&config_file::config_trust_root(path));
            let config_path = canonical_path_key(path);
            let mtime = config_path_mtime_millis(Path::new(&config_path));
            (trust_root, config_path, mtime)
        })
        .collect::<Vec<_>>();

    hash_to_str(&(cwd, configs))
}

fn canonical_path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn config_path_mtime_millis(path: &Path) -> u128 {
    path.metadata()
        .and_then(|m| m.modified())
        .map(mtime_to_millis)
        .unwrap_or_default()
}

pub static PREV_SESSION: Lazy<HookEnvSession> = Lazy::new(|| {
    env::var("__MISE_SESSION")
        .ok()
        .and_then(|s| {
            deserialize(s)
                .map_err(|err| {
                    warn!("error deserializing __MISE_SESSION: {err}");
                    err
                })
                .ok()
        })
        .unwrap_or_default()
});

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct WatchFilePattern {
    pub root: Option<PathBuf>,
    pub patterns: Vec<String>,
}

impl From<&Path> for WatchFilePattern {
    fn from(path: &Path) -> Self {
        Self {
            root: None,
            patterns: vec![path.to_string_lossy().to_string()],
        }
    }
}

impl From<PathBuf> for WatchFilePattern {
    fn from(path: PathBuf) -> Self {
        Self {
            patterns: vec![path.to_string_lossy().to_string()],
            root: Some(path),
        }
    }
}

/// Fast-path early exit check that can be called BEFORE loading config/tools.
/// This checks basic conditions using only the previous session data.
/// Returns true if we can definitely skip hook-env, false if we need to continue.
pub fn should_exit_early_fast() -> bool {
    // `main` asks before starting the async runtime, and `cli::run` asks again
    // when that answer was no; the second must not repeat the filesystem checks.
    static RESULT: OnceLock<bool> = OnceLock::new();
    *RESULT.get_or_init(check_exit_early_fast)
}

fn check_exit_early_fast() -> bool {
    let args = env::ARGS.read().unwrap();
    if args.len() < 2 || args[1] != "hook-env" {
        return false;
    }
    if has_preclap_logging_flag(&args) {
        return false;
    }
    // Can't exit early if no previous session
    // Check for dir being set as a proxy for "has valid session"
    // (loaded_configs can be empty if there are no config files)
    if PREV_SESSION.dir.is_none() {
        return false;
    }
    // Can't exit early if --force flag is present
    if args.iter().any(|a| a == "--force" || a == "-f") {
        return false;
    }
    if has_untrusted_config_warning_marker() {
        return false;
    }
    // Check if running from precmd for the first time
    // Handle both "--reason=precmd" and "--reason precmd" forms
    let is_precmd = args.iter().any(|a| a == "--reason=precmd")
        || args
            .windows(2)
            .any(|w| w[0] == "--reason" && w[1] == "precmd");
    if is_precmd && !*env::__MISE_ZSH_PRECMD_RUN {
        return false;
    }

    // hook_env.chpwd_only and hook_env.cache_ttl are the only settings this
    // check reads, and loading settings costs more than the rest of it. When
    // the full run that wrote the session found neither set, skip the load:
    // turning either on edits a config file or a MISE_* variable, which the
    // checks below catch, and the resulting full run refreshes the session.
    // Otherwise read them live, so turning one off applies on the next prompt,
    // as it does for a session written before this was recorded.
    let (chpwd_only, cache_ttl_ms) = if PREV_SESSION.hook_env_shortcuts_unset {
        (false, 0)
    } else {
        let settings = Settings::get();
        let cache_ttl_ms = duration::parse_duration(&settings.hook_env.cache_ttl)
            .map(|d| d.as_millis())
            .inspect_err(|e| warn!("invalid hook_env.cache_ttl setting: {e}"))
            .unwrap_or(0);
        (settings.hook_env.chpwd_only, cache_ttl_ms)
    };

    // Compute TTL window check only if cache_ttl is enabled (avoid unnecessary file read)
    let (now, within_ttl_window) = if cache_ttl_ms > 0 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let last_full_check = read_last_full_check();
        (now, now.saturating_sub(last_full_check) < cache_ttl_ms)
    } else {
        (0, false)
    };

    // Can't exit early if directory changed
    if dir_change().is_some() {
        return false;
    }
    // Can't exit early if MISE_ env vars changed (cheap in-memory hash comparison)
    if have_mise_env_vars_been_modified() {
        return false;
    }

    // chpwd_only mode: skip on precmd if directory hasn't changed
    // This significantly reduces stat operations on slow filesystems like NFS
    // Note: We check this AFTER env var check since that's cheap (no I/O)
    if chpwd_only && is_precmd {
        trace!("chpwd_only enabled, skipping precmd hook-env");
        return true;
    }

    // Cache TTL check: if within the TTL window, skip all stat operations
    // This is useful for slow filesystems like NFS where stat calls are expensive
    if within_ttl_window {
        trace!("within cache TTL, skipping filesystem checks");
        return true;
    }

    // Every staleness check below returns via force_full_run() so the slow path
    // knows this run must reach build_session and refresh the session.
    // Check if any loaded config files have been modified
    for config_path in &PREV_SESSION.loaded_configs {
        if let Ok(metadata) = config_path.metadata() {
            if let Ok(modified) = metadata.modified()
                && mtime_to_millis(modified) > PREV_SESSION.latest_update
            {
                return force_full_run();
            }
        } else if !config_path.exists() {
            return force_full_run();
        }
    }
    // Check if any files accessed by tera template functions have been modified
    for path in &PREV_SESSION.tera_files {
        if let Ok(metadata) = path.metadata() {
            if let Ok(modified) = metadata.modified()
                && mtime_to_millis(modified) > PREV_SESSION.latest_update
            {
                return force_full_run();
            }
        } else if !path.exists() {
            return force_full_run();
        }
    }
    // Check if any files from [[watch_files]] patterns have been modified
    for path in &PREV_SESSION.watch_files {
        if let Ok(metadata) = path.metadata() {
            if let Ok(modified) = metadata.modified()
                && mtime_to_millis(modified) > PREV_SESSION.latest_update
            {
                return force_full_run();
            }
        } else if !path.exists() {
            return force_full_run();
        }
    }
    if have_trust_state_dirs_been_modified() {
        return force_full_run();
    }
    // Check if data dir has been modified (new tools installed, etc.)
    // Also check if it's been deleted - this requires a full update
    if !dirs::DATA.exists() {
        return force_full_run();
    }
    if let Ok(metadata) = dirs::DATA.metadata()
        && let Ok(modified) = metadata.modified()
        && mtime_to_millis(modified) > PREV_SESSION.latest_update
    {
        return force_full_run();
    }
    // Check if any directory in the config search path has been modified
    // This catches new config files created anywhere in the hierarchy.
    // The slow path has no equivalent check, which is exactly why the
    // FAST_PATH_FORCED_FULL_RUN handshake exists.
    for modified in config_search_dir_mtimes() {
        if mtime_to_millis(modified) > PREV_SESSION.latest_update {
            return force_full_run();
        }
    }
    // Filesystem checks passed - update the last check timestamp so subsequent
    // prompts can benefit from the TTL cache without repeating these checks
    if cache_ttl_ms > 0 {
        write_last_full_check(now);
    }
    true
}

/// Check if hook-env can exit early after config is loaded.
/// This is called after the fast-path check and handles cases that need
/// the full config (watch_files, hook scheduling).
pub fn should_exit_early(
    watch_files: impl IntoIterator<Item = WatchFilePattern>,
    reason: Option<HookReason>,
) -> bool {
    // Force hook-env to run at least once from precmd after activation
    // This catches PATH modifications from shell initialization (e.g., path_helper in zsh)
    if reason == Some(HookReason::Precmd) && !*env::__MISE_ZSH_PRECMD_RUN {
        trace!("__MISE_ZSH_PRECMD_RUN=0 and reason=precmd, forcing hook-env to run");
        return false;
    }
    if has_untrusted_config_warning_marker() {
        return false;
    }
    // Schedule hooks on directory change (can't do this in fast-path)
    if schedule_dir_change_hooks() {
        return false;
    }
    // Check full watch_files list from config (may include more than config files)
    let watch_files = match get_watch_files(watch_files) {
        Ok(w) => w,
        Err(e) => {
            warn!("error getting watch files: {e}");
            return false;
        }
    };
    if have_files_been_modified(watch_files) {
        return false;
    }
    if have_mise_env_vars_been_modified() {
        return false;
    }
    // The fast path already decided this run is necessary. Check it only after
    // the slow-path checks above, since they also record modified watch files
    // and schedule hooks as side effects.
    if FAST_PATH_FORCED_FULL_RUN.load(Ordering::Relaxed) {
        trace!("fast-path forced a full run, not exiting early");
        return false;
    }
    trace!("early-exit");
    true
}

/// Schedules the leave, cd, and enter hooks when the directory differs from the
/// previous session's, including the first run after activation. Returns whether
/// it scheduled them.
pub fn schedule_dir_change_hooks() -> bool {
    if dir_change().is_none() {
        return false;
    }
    hooks::schedule_hook(hooks::Hooks::Leave);
    hooks::schedule_hook(hooks::Hooks::Cd);
    hooks::schedule_hook(hooks::Hooks::Enter);
    true
}

pub(crate) fn dir_change() -> Option<(Option<PathBuf>, PathBuf)> {
    match (&PREV_SESSION.dir, &*dirs::CWD) {
        (Some(old), Some(new)) if old != new => {
            trace!("dir change: {:?} -> {:?}", old, new);
            Some((Some(old.clone()), new.clone()))
        }
        (None, Some(new)) => {
            trace!("dir change: None -> {:?}", new);
            Some((None, new.clone()))
        }
        _ => None,
    }
}

fn have_files_been_modified(watch_files: BTreeSet<PathBuf>) -> bool {
    if let Some(p) = PREV_SESSION.loaded_configs.iter().find(|p| !p.exists()) {
        trace!("config deleted: {}", p.display());
        return true;
    }
    // check the files to see if they've been altered
    let mut modified = false;
    for fp in &watch_files {
        if let Ok(mtime) = fp.metadata().and_then(|m| m.modified()) {
            if mtime_to_millis(mtime) > PREV_SESSION.latest_update {
                trace!("file modified: {:?}", fp);
                modified = true;
                watch_files::add_modified_file(fp.clone());
            }
        } else if !fp.exists() {
            trace!("file deleted: {:?}", fp);
            modified = true;
            watch_files::add_modified_file(fp.clone());
        }
    }
    if !modified {
        trace!("watch files unmodified");
    }
    modified
}

fn have_trust_state_dirs_been_modified() -> bool {
    for path in [&*dirs::TRUSTED_CONFIGS, &*dirs::IGNORED_CONFIGS] {
        if PREV_SESSION.watch_files.iter().any(|p| p == path) {
            continue;
        }
        if let Ok(metadata) = path.metadata()
            && let Ok(modified) = metadata.modified()
            && mtime_to_millis(modified) > PREV_SESSION.latest_update
        {
            trace!("trust state dir modified: {:?}", path);
            return true;
        }
    }
    false
}

fn has_preclap_logging_flag(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(arg.as_str(), "-q" | "--quiet" | "--silent" | "--log-level")
            || arg.starts_with("--log-level=")
    })
}

fn have_mise_env_vars_been_modified() -> bool {
    get_mise_env_vars_hashed() != PREV_SESSION.env_var_hash
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HookEnvSession {
    /// See [`ENV_STATE_VERSION`]. Decides how `env` is compared.
    #[serde(default = "legacy_env_state_version")]
    pub v: u32,
    pub loaded_tools: IndexSet<String>,
    pub loaded_configs: IndexSet<PathBuf>,
    pub config_paths: IndexSet<PathBuf>,
    /// Env var name to [`hash_env_value`] digest (plaintext in a version 1 session). Values
    /// are only compared, so they are not stored.
    pub env: EnvMap,
    #[serde(default)]
    pub aliases: indexmap::IndexMap<String, String>,
    /// Files accessed by tera template functions (read_file, hash_file, etc.)
    /// that should be watched for changes.
    #[serde(default)]
    pub tera_files: Vec<PathBuf>,
    /// Resolved file paths from [[watch_files]] config patterns and env plugin watch_files.
    /// Stored so the fast-path can detect changes without loading config.
    #[serde(default)]
    pub watch_files: Vec<PathBuf>,
    dir: Option<PathBuf>,
    env_var_hash: String,
    latest_update: u128,
    /// Whether the full run that wrote this session found `hook_env.chpwd_only`
    /// off and `hook_env.cache_ttl` unset, which lets the fast path skip
    /// loading settings. False in sessions from older mise versions.
    #[serde(default)]
    hook_env_shortcuts_unset: bool,
}

pub fn serialize<T: serde::Serialize>(obj: &T) -> Result<String> {
    let mut gz = ZlibEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&rmp_serde::to_vec_named(obj)?)?;
    Ok(BASE64_STANDARD_NO_PAD.encode(gz.finish()?))
}

pub(crate) fn deserialize<T: serde::de::DeserializeOwned>(raw: String) -> Result<T> {
    let mut writer = Vec::new();
    let mut decoder = ZlibDecoder::new(writer);
    let bytes = BASE64_STANDARD_NO_PAD.decode(raw)?;
    decoder.write_all(&bytes[..])?;
    writer = decoder.finish()?;
    Ok(rmp_serde::from_slice(&writer[..])?)
}

/// Collect mtimes for config-search ancestor directories.
/// Used by both `should_exit_early_fast` and `build_session` to avoid divergence.
fn config_search_dir_mtimes() -> Vec<SystemTime> {
    let mut mtimes = Vec::new();
    if let Some(cwd) = &*dirs::CWD
        && let Ok(ancestor_dirs) = file::all_dirs(cwd, &env::MISE_CEILING_PATHS)
    {
        let config_subdirs = DEFAULT_CONFIG_FILENAMES
            .iter()
            .map(|f| Path::new(f).parent().and_then(|p| p.to_str()).unwrap_or(""))
            .unique()
            .collect::<Vec<_>>();
        for dir in ancestor_dirs {
            for subdir in &config_subdirs {
                // `conf.d/*` names the folder fragments: a file added to one
                // changes that folder's mtime, not conf.d's.
                let check_dirs = if subdir.contains('*') {
                    glob::glob(&dir.join(subdir).to_string_lossy())
                        .map(|paths| paths.flatten().collect())
                        .unwrap_or_default()
                } else if subdir.is_empty() {
                    vec![dir.clone()]
                } else {
                    vec![dir.join(subdir)]
                };
                for check_dir in check_dirs {
                    if let Ok(Ok(modified)) = check_dir.metadata().map(|m| m.modified()) {
                        mtimes.push(modified);
                    }
                }
            }
        }
    }
    mtimes
}

pub async fn build_session(
    config: &Arc<Config>,
    env: EnvMap,
    aliases: indexmap::IndexMap<String, String>,
    loaded_tools: IndexSet<String>,
    watch_files: BTreeSet<WatchFilePattern>,
    config_paths: IndexSet<PathBuf>,
) -> Result<HookEnvSession> {
    let mut max_modtime = UNIX_EPOCH;
    let resolved_watch_files = get_watch_files(watch_files)?;
    for cf in &resolved_watch_files {
        if let Ok(Ok(modified)) = cf.metadata().map(|m| m.modified()) {
            max_modtime = std::cmp::max(modified, max_modtime);
        }
    }

    // Include tera template files in max_modtime so latest_update reflects
    // their mtimes even when watch_files comes from env_cache
    for tf in &config.tera_files {
        if let Ok(Ok(modified)) = tf.metadata().map(|m| m.modified()) {
            max_modtime = std::cmp::max(modified, max_modtime);
        }
    }

    // Keep latest_update aligned with the fast-path checks so a full hook-env run
    // can stabilize subsequent prompts instead of repeatedly falling back.
    if let Ok(Ok(modified)) = dirs::DATA.metadata().map(|m| m.modified()) {
        max_modtime = std::cmp::max(modified, max_modtime);
    }
    for modified in config_search_dir_mtimes() {
        max_modtime = std::cmp::max(modified, max_modtime);
    }

    let included_paths: Vec<PathBuf> = config
        .config_files
        .values()
        .flat_map(|cf| cf.included_paths())
        .collect();
    // A remote include is stored in a cache file that is rewritten when it
    // refreshes. It is checked on every prompt like any loaded config, so
    // latest_update has to cover it or one refresh would make every later
    // prompt run in full.
    for path in &included_paths {
        if let Ok(Ok(modified)) = path.metadata().map(|m| m.modified()) {
            max_modtime = std::cmp::max(modified, max_modtime);
        }
    }
    let loaded_configs: IndexSet<PathBuf> = config
        .config_files
        .keys()
        .cloned()
        .chain(included_paths)
        .collect();

    let settings = Settings::get();
    let cache_ttl_ms = duration::parse_duration(&settings.hook_env.cache_ttl)
        .map(|d| d.as_millis())
        .inspect_err(|e| warn!("invalid hook_env.cache_ttl setting: {e}"))
        .unwrap_or(0);
    // Update the last full check timestamp (only if cache_ttl feature is enabled)
    if cache_ttl_ms > 0 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        write_last_full_check(now);
    }

    Ok(HookEnvSession {
        v: ENV_STATE_VERSION,
        dir: dirs::CWD.clone(),
        env_var_hash: get_mise_env_vars_hashed(),
        env: env
            .into_iter()
            .map(|(k, v)| (k, hash_env_value(&v)))
            .collect(),
        aliases,
        tera_files: config.tera_files.clone(),
        watch_files: resolved_watch_files.into_iter().collect(),
        loaded_configs,
        loaded_tools,
        config_paths,
        latest_update: mtime_to_millis(max_modtime),
        hook_env_shortcuts_unset: !settings.hook_env.chpwd_only && cache_ttl_ms == 0,
    })
}

pub(crate) fn get_watch_files(
    watch_files: impl IntoIterator<Item = WatchFilePattern>,
) -> Result<BTreeSet<PathBuf>> {
    let mut watches = BTreeSet::new();
    if dirs::DATA.exists() {
        watches.insert(dirs::DATA.to_path_buf());
    }
    if dirs::TRUSTED_CONFIGS.exists() {
        watches.insert(dirs::TRUSTED_CONFIGS.to_path_buf());
    }
    if dirs::IGNORED_CONFIGS.exists() {
        watches.insert(dirs::IGNORED_CONFIGS.to_path_buf());
    }
    for (root, patterns) in &watch_files.into_iter().chunk_by(|wfp| wfp.root.clone()) {
        if let Some(root) = root {
            let patterns = patterns.flat_map(|wfp| wfp.patterns).collect::<Vec<_>>();
            watches.extend(watch_files::glob(&root, &patterns)?);
        } else {
            watches.extend(patterns.flat_map(|wfp| wfp.patterns).map(PathBuf::from));
        }
    }

    Ok(watches)
}

/// gets a hash of all MISE_ environment variables
fn get_mise_env_vars_hashed() -> String {
    let env_vars: Vec<(&String, &String)> = env::PRISTINE_ENV
        .deref()
        .iter()
        .filter(|(k, _)| k.starts_with("MISE_"))
        .sorted()
        .collect();
    hash_to_str(&env_vars)
}

pub fn clear_old_env_patches(shell: &dyn Shell) -> EnvDiffPatches {
    let mut patches = env::__MISE_DIFF.reverse().to_patches();

    // For fish shell, filter out PATH operations from the reversed diff because
    // fish has its own PATH management that conflicts with ours.
    if shell.to_string() == "fish" {
        patches.retain(|p| match p {
            EnvDiffOperation::Add(k, _)
            | EnvDiffOperation::Change(k, _)
            | EnvDiffOperation::Remove(k) => k != &*PATH_KEY,
        });
        // Fish also needs PATH restored during deactivation
        let new_path = compute_deactivated_path();
        patches.push(EnvDiffOperation::Change(PATH_KEY.to_string(), new_path));
    } else {
        // For non-fish shells, we need to preserve user-added paths while removing mise paths
        let new_path = compute_deactivated_path();
        patches.push(EnvDiffOperation::Change(PATH_KEY.to_string(), new_path));
    }
    patches
}

pub(crate) fn clear_old_env(shell: &dyn Shell) -> String {
    build_env_commands(shell, &clear_old_env_patches(shell))
}

/// Clear all aliases from the previous session. Called only during deactivation.
pub(crate) fn clear_aliases(shell: &dyn Shell) -> String {
    let mut output = String::new();
    for name in PREV_SESSION.aliases.keys() {
        output.push_str(&shell.unset_alias(name));
    }
    output
}

/// Compute PATH after deactivation, preserving user additions
fn compute_deactivated_path() -> String {
    // Get current PATH (may include user additions since last hook-env)
    let current_path = env::var("PATH").unwrap_or_default();

    // Get the PATH that mise set during the last hook-env
    let mise_paths = &env::__MISE_DIFF.path;

    // The activation prelude may establish a mise-managed shim boundary before
    // the first hook runs. Prefer the shell's explicit pre-activation snapshot
    // so deactivation does not preserve that boundary as a user-owned path.
    let pristine_path = env::__MISE_ORIG_PATH.clone().unwrap_or_else(|| {
        env::PRISTINE_ENV
            .deref()
            .get(&*PATH_KEY)
            .map(|s| s.to_string())
            .unwrap_or_default()
    });
    let pristine_path = crate::windows_posix::orig_path_for_windows(&pristine_path).into_owned();

    if current_path.is_empty() || mise_paths.is_empty() {
        // If no current PATH or no mise PATH, just return pristine
        return pristine_path;
    }

    // Parse paths
    let current_paths: Vec<PathBuf> = env::split_paths(&current_path).collect();
    let mise_paths_vec = mise_paths.clone();

    // Count occurrences of each path in current_path, mise_paths, and pristine_path
    let pristine_paths: Vec<PathBuf> = env::split_paths(&pristine_path).collect();

    let mut current_counts: std::collections::HashMap<PathBuf, usize> =
        std::collections::HashMap::new();
    for path in &current_paths {
        *current_counts.entry(path.clone()).or_insert(0) += 1;
    }

    let mut mise_counts: std::collections::HashMap<PathBuf, usize> =
        std::collections::HashMap::new();
    for path in &mise_paths_vec {
        *mise_counts.entry(path.clone()).or_insert(0) += 1;
    }

    let mut pristine_counts: std::collections::HashMap<PathBuf, usize> =
        std::collections::HashMap::new();
    for path in &pristine_paths {
        *pristine_counts.entry(path.clone()).or_insert(0) += 1;
    }

    // Determine how many copies of each path we should keep: user additions plus pristine entries
    use std::collections::HashMap;

    let mut target_counts: HashMap<PathBuf, usize> = HashMap::new();
    for (path, current_count) in current_counts.iter() {
        let removal_count = *mise_counts.get(path).unwrap_or(&0);
        let pristine_count = *pristine_counts.get(path).unwrap_or(&0);
        let user_and_pristine = if file::is_mise_shims_dir(path) {
            // Activation owns the shim boundary even though it is not part of
            // EnvDiff::path. Preserve only copies that existed before activation.
            pristine_count
        } else {
            current_count
                .saturating_sub(removal_count)
                .max(pristine_count)
        };
        target_counts.insert(path.clone(), user_and_pristine);
    }

    for (path, pristine_count) in pristine_counts.iter() {
        target_counts
            .entry(path.clone())
            .and_modify(|count| *count = (*count).max(*pristine_count))
            .or_insert(*pristine_count);
    }

    let mut kept_counts: HashMap<PathBuf, usize> = HashMap::new();
    let mut final_paths: Vec<PathBuf> = Vec::new();

    for path in &current_paths {
        if let Some(target) = target_counts.get(path) {
            let kept = kept_counts.entry(path.clone()).or_insert(0);
            if *kept < *target {
                final_paths.push(path.clone());
                *kept += 1;
            }
        }
    }

    for path in pristine_paths {
        let target = target_counts.get(&path).copied().unwrap_or(0);
        let kept = kept_counts.entry(path.clone()).or_insert(0);
        while *kept < target {
            final_paths.push(path.clone());
            *kept += 1;
        }
    }

    env::join_paths(final_paths.iter())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(pristine_path)
}

pub fn build_env_commands(shell: &dyn Shell, patches: &EnvDiffPatches) -> String {
    let mut output = String::new();

    for patch in patches.iter() {
        match patch {
            EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                output.push_str(&shell.set_env(k, v));
            }
            EnvDiffOperation::Remove(k) => {
                output.push_str(&shell.unset_env(k));
            }
        }
    }

    output
}

/// Build shell alias commands based on the difference between old and new aliases
pub fn build_alias_commands(
    shell: &dyn Shell,
    old_aliases: &indexmap::IndexMap<String, String>,
    new_aliases: &indexmap::IndexMap<String, String>,
) -> String {
    let mut output = String::new();

    // Remove aliases that no longer exist or have changed
    for (name, old_cmd) in old_aliases {
        match new_aliases.get(name) {
            Some(new_cmd) if new_cmd != old_cmd => {
                // Alias changed, unset then set new
                output.push_str(&shell.unset_alias(name));
                output.push_str(&shell.set_alias(name, new_cmd));
            }
            None => {
                // Alias removed
                output.push_str(&shell.unset_alias(name));
            }
            _ => {
                // Alias unchanged, do nothing
            }
        }
    }

    // Add new aliases
    for (name, cmd) in new_aliases {
        if !old_aliases.contains_key(name) {
            output.push_str(&shell.set_alias(name, cmd));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{FAST_PATH_FORCED_FULL_RUN, force_full_run, has_preclap_logging_flag};
    use std::sync::atomic::Ordering;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn session_version_decides_how_env_is_compared() {
        use super::{HookEnvSession, deserialize, serialize};
        use crate::env_diff::{ENV_STATE_VERSION, env_value_matches, hash_env_value};

        // what the previous mise wrote: no version field, plaintext values
        #[derive(serde::Serialize)]
        struct Legacy {
            loaded_tools: Vec<String>,
            loaded_configs: Vec<String>,
            config_paths: Vec<String>,
            env: std::collections::BTreeMap<String, String>,
            env_var_hash: String,
            latest_update: u64,
        }
        let legacy = serialize(&Legacy {
            loaded_tools: vec![],
            loaded_configs: vec![],
            config_paths: vec![],
            env: [("K".to_string(), "secret".to_string())].into(),
            env_var_hash: String::new(),
            latest_update: 0,
        })
        .unwrap();
        let session: HookEnvSession = deserialize(legacy).unwrap();
        assert_eq!(session.v, 1);
        assert!(env_value_matches(session.v, &session.env["K"], "secret"));

        let current = HookEnvSession {
            v: ENV_STATE_VERSION,
            env: [("K".to_string(), hash_env_value("secret"))].into(),
            ..Default::default()
        };
        let current: HookEnvSession = deserialize(serialize(&current).unwrap()).unwrap();
        assert_eq!(current.v, ENV_STATE_VERSION);
        assert!(env_value_matches(current.v, &current.env["K"], "secret"));
        assert!(!env_value_matches(current.v, &current.env["K"], "other"));
    }

    #[test]
    fn force_full_run_records_the_decision_and_reports_not_exiting_early() {
        let prev = FAST_PATH_FORCED_FULL_RUN.swap(false, Ordering::Relaxed);

        // Returns false so callers can `return force_full_run();` directly, and
        // leaves the flag set for should_exit_early to observe.
        assert!(!force_full_run());
        assert!(FAST_PATH_FORCED_FULL_RUN.load(Ordering::Relaxed));

        FAST_PATH_FORCED_FULL_RUN.store(prev, Ordering::Relaxed);
    }

    #[test]
    fn detects_logging_flags_that_need_clap_before_fast_exit() {
        assert!(has_preclap_logging_flag(&args(&[
            "mise", "hook-env", "-s", "bash", "--quiet"
        ])));
        assert!(has_preclap_logging_flag(&args(&["mise", "hook-env", "-q"])));
        assert!(has_preclap_logging_flag(&args(&[
            "mise", "hook-env", "--silent"
        ])));
        assert!(has_preclap_logging_flag(&args(&[
            "mise",
            "hook-env",
            "--log-level",
            "error"
        ])));
        assert!(has_preclap_logging_flag(&args(&[
            "mise",
            "hook-env",
            "--log-level=error"
        ])));
    }

    #[test]
    fn ignores_logging_flags_that_do_not_suppress_warnings() {
        assert!(!has_preclap_logging_flag(&args(&[
            "mise", "hook-env", "-s", "bash"
        ])));
        assert!(!has_preclap_logging_flag(&args(&[
            "mise", "hook-env", "--trace"
        ])));
        assert!(!has_preclap_logging_flag(&args(&[
            "mise", "hook-env", "--debug"
        ])));
        assert!(!has_preclap_logging_flag(&args(&[
            "mise",
            "hook-env",
            "--verbose"
        ])));
    }
}
