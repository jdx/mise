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
use crate::config::Config;
use crate::env::PATH_KEY;
use crate::env_diff::{EnvDiffOperation, EnvDiffPatches, EnvMap};
use crate::hash::hash_to_str;
use crate::shell::Shell;
use crate::{dirs, env, file, hooks, watch_files};

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
    // Can't exit early if directory changed
    if dir_change().is_some() {
        return false;
    }
    // Can't exit early if MISE_ env vars changed
    if have_mise_env_vars_been_modified() {
        return false;
    }
    // Check if any loaded config files have been modified
    for config_path in &PREV_SESSION.loaded_configs {
        if let Ok(metadata) = config_path.metadata() {
            if let Ok(modified) = metadata.modified()
                && mtime_to_millis(modified) > PREV_SESSION.latest_update {
                    return false;
                }
        } else if !config_path.exists() {
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
            && mtime_to_millis(modified) > PREV_SESSION.latest_update {
                return false;
            }
    // Check if any directory in the config search path has been modified
    // This catches new config files created anywhere in the hierarchy
    if let Some(cwd) = &*dirs::CWD
        && let Ok(ancestor_dirs) = file::all_dirs(cwd, &env::MISE_CEILING_PATHS)
    {
        // Config subdirectories that might contain config files
        let config_subdirs = ["", ".config/mise", ".mise", "mise", ".config"];
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
        hooks::schedule_hook(hooks::Hooks::Cd);
        hooks::schedule_hook(hooks::Hooks::Enter);
        hooks::schedule_hook(hooks::Hooks::Leave);
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
    loaded_tools: IndexSet<String>,
    watch_files: BTreeSet<WatchFilePattern>,
) -> Result<HookEnvSession> {
    let mut max_modtime = UNIX_EPOCH;
    for cf in get_watch_files(watch_files)? {
        if let Ok(Ok(modified)) = cf.metadata().map(|m| m.modified()) {
            max_modtime = std::cmp::max(modified, max_modtime);
        }
    }

    let config_paths = if let Ok(paths) = config.path_dirs().await {
        paths.iter().cloned().collect()
    } else {
        IndexSet::new()
    };

    Ok(HookEnvSession {
        dir: dirs::CWD.clone(),
        env_var_hash: get_mise_env_vars_hashed(),
        env,
        loaded_configs: config.config_files.keys().cloned().collect(),
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
