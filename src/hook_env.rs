use std::io::prelude::*;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::BTreeSet, sync::Arc};

use base64::prelude::*;
use eyre::Result;
use flate2::Compression;
use flate2::write::{ZlibDecoder, ZlibEncoder};
use indexmap::IndexSet;
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::sync::LazyLock as Lazy;

use crate::cli::HookReason;
use crate::config::{Config, DEFAULT_CONFIG_FILENAMES, Settings};
use crate::env::PATH_KEY;
use crate::env_diff::{EnvDiffOperation, EnvDiffPatches, EnvMap};
use crate::hash::hash_to_str;
use crate::shell::Shell;
use crate::{dirs, duration, env, file, hooks, watch_files};

/// Directory to store per-directory last check timestamps.
/// Timestamps are stored per-directory (using a hash of CWD) so that
/// multiple shells in different directories don't interfere with each other.
static LAST_CHECK_DIR: Lazy<PathBuf> = Lazy::new(|| dirs::STATE.join("hook-env-checks"));

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

/// Convert a SystemTime to milliseconds since Unix epoch
fn mtime_to_millis(mtime: SystemTime) -> u128 {
    mtime
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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
    let args = env::ARGS.read().unwrap();
    if args.len() < 2 || args[1] != "hook-env" {
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
    // Check if running from precmd for the first time
    // Handle both "--reason=precmd" and "--reason precmd" forms
    let is_precmd = args.iter().any(|a| a == "--reason=precmd")
        || args
            .windows(2)
            .any(|w| w[0] == "--reason" && w[1] == "precmd");
    if is_precmd && !*env::__MISE_ZSH_PRECMD_RUN {
        return false;
    }

    // Get settings for cache_ttl and chpwd_only
    let settings = Settings::get();
    let cache_ttl_ms = duration::parse_duration(&settings.hook_env.cache_ttl)
        .map(|d| d.as_millis())
        .inspect_err(|e| warn!("invalid hook_env.cache_ttl setting: {e}"))
        .unwrap_or(0);

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
    if settings.hook_env.chpwd_only && is_precmd {
        trace!("chpwd_only enabled, skipping precmd hook-env");
        return true;
    }

    // Cache TTL check: if within the TTL window, skip all stat operations
    // This is useful for slow filesystems like NFS where stat calls are expensive
    if within_ttl_window {
        trace!("within cache TTL, skipping filesystem checks");
        return true;
    }

    // Check if any loaded config files have been modified
    for config_path in &PREV_SESSION.loaded_configs {
        if let Ok(metadata) = config_path.metadata() {
            if let Ok(modified) = metadata.modified()
                && mtime_to_millis(modified) > PREV_SESSION.latest_update
            {
                return false;
            }
        } else if !config_path.exists() {
            return false;
        }
    }
    // Check if any files accessed by tera template functions have been modified
    for path in &PREV_SESSION.tera_files {
        if let Ok(metadata) = path.metadata() {
            if let Ok(modified) = metadata.modified()
                && mtime_to_millis(modified) > PREV_SESSION.latest_update
            {
                return false;
            }
        } else if !path.exists() {
            return false;
        }
    }
    // Check if data dir has been modified (new tools installed, etc.)
    // Also check if it's been deleted - this requires a full update
    if !dirs::DATA.exists() {
        return false;
    }
    if let Ok(metadata) = dirs::DATA.metadata()
        && let Ok(modified) = metadata.modified()
        && mtime_to_millis(modified) > PREV_SESSION.latest_update
    {
        return false;
    }
    // Check if any directory in the config search path has been modified
    // This catches new config files created anywhere in the hierarchy
    if let Some(cwd) = &*dirs::CWD
        && let Ok(ancestor_dirs) = file::all_dirs(cwd, &env::MISE_CEILING_PATHS)
    {
        // Config subdirectories that might contain config files
        let config_subdirs = DEFAULT_CONFIG_FILENAMES
            .iter()
            .map(|f| Path::new(f).parent().and_then(|p| p.to_str()).unwrap_or(""))
            .unique()
            .collect::<Vec<_>>();
        for dir in ancestor_dirs {
            for subdir in &config_subdirs {
                let check_dir = if subdir.is_empty() {
                    dir.clone()
                } else {
                    dir.join(subdir)
                };
                if let Ok(metadata) = check_dir.metadata()
                    && let Ok(modified) = metadata.modified()
                    && mtime_to_millis(modified) > PREV_SESSION.latest_update
                {
                    return false;
                }
            }
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
    // Schedule hooks on directory change (can't do this in fast-path)
    if dir_change().is_some() {
        hooks::schedule_hook(hooks::Hooks::Leave);
        hooks::schedule_hook(hooks::Hooks::Cd);
        hooks::schedule_hook(hooks::Hooks::Enter);
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
    trace!("early-exit");
    true
}

pub fn dir_change() -> Option<(Option<PathBuf>, PathBuf)> {
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

fn have_mise_env_vars_been_modified() -> bool {
    get_mise_env_vars_hashed() != PREV_SESSION.env_var_hash
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HookEnvSession {
    pub loaded_tools: IndexSet<String>,
    pub loaded_configs: IndexSet<PathBuf>,
    pub config_paths: IndexSet<PathBuf>,
    pub env: EnvMap,
    #[serde(default)]
    pub aliases: indexmap::IndexMap<String, String>,
    /// Files accessed by tera template functions (read_file, hash_file, etc.)
    /// that should be watched for changes.
    #[serde(default)]
    pub tera_files: Vec<PathBuf>,
    dir: Option<PathBuf>,
    env_var_hash: String,
    latest_update: u128,
}

pub fn serialize<T: serde::Serialize>(obj: &T) -> Result<String> {
    let mut gz = ZlibEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&rmp_serde::to_vec_named(obj)?)?;
    Ok(BASE64_STANDARD_NO_PAD.encode(gz.finish()?))
}

pub fn deserialize<T: serde::de::DeserializeOwned>(raw: String) -> Result<T> {
    let mut writer = Vec::new();
    let mut decoder = ZlibDecoder::new(writer);
    let bytes = BASE64_STANDARD_NO_PAD.decode(raw)?;
    decoder.write_all(&bytes[..])?;
    writer = decoder.finish()?;
    Ok(rmp_serde::from_slice(&writer[..])?)
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
    for cf in get_watch_files(watch_files)? {
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

    let loaded_configs: IndexSet<PathBuf> = config.config_files.keys().cloned().collect();

    // Update the last full check timestamp (only if cache_ttl feature is enabled)
    let settings = Settings::get();
    if duration::parse_duration(&settings.hook_env.cache_ttl)
        .map(|d| d.as_millis() > 0)
        .unwrap_or(false)
    {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        write_last_full_check(now);
    }

    Ok(HookEnvSession {
        dir: dirs::CWD.clone(),
        env_var_hash: get_mise_env_vars_hashed(),
        env,
        aliases,
        tera_files: config.tera_files.clone(),
        loaded_configs,
        loaded_tools,
        config_paths,
        latest_update: mtime_to_millis(max_modtime),
    })
}

pub fn get_watch_files(
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

pub fn clear_old_env(shell: &dyn Shell) -> String {
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
    build_env_commands(shell, &patches)
}

/// Clear all aliases from the previous session. Called only during deactivation.
pub fn clear_aliases(shell: &dyn Shell) -> String {
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

    // Get pristine PATH (from before mise activation)
    let pristine_path = env::PRISTINE_ENV
        .deref()
        .get(&*PATH_KEY)
        .map(|s| s.to_string())
        .unwrap_or_default();

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
        let user_and_pristine = current_count
            .saturating_sub(removal_count)
            .max(pristine_count);
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
