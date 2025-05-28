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

use crate::config::Config;
use crate::env::PATH_KEY;
use crate::env_diff::{EnvDiffOperation, EnvDiffPatches, EnvMap};
use crate::hash::hash_to_str;
use crate::shell::Shell;
use crate::{dirs, env, hooks, watch_files};

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

/// this function will early-exit the application if hook-env is being
/// called and it does not need to be
pub fn should_exit_early(watch_files: impl IntoIterator<Item = WatchFilePattern>) -> bool {
    let args = env::ARGS.read().unwrap();
    if args.len() < 2 || args[1] != "hook-env" {
        return false;
    }
    if dir_change().is_some() {
        hooks::schedule_hook(hooks::Hooks::Cd);
        hooks::schedule_hook(hooks::Hooks::Enter);
        hooks::schedule_hook(hooks::Hooks::Leave);
        return false;
    }
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
        if let Ok(modtime) = fp.metadata().and_then(|m| m.modified()) {
            let modtime = modtime
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            if modtime > PREV_SESSION.latest_update {
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
    if get_mise_env_vars_hashed() != PREV_SESSION.env_var_hash {
        return true;
    }
    false
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
        latest_update: max_modtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
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
    if let Some(path) = env::PRISTINE_ENV.deref().get(&*PATH_KEY) {
        patches.push(EnvDiffOperation::Change(
            PATH_KEY.to_string(),
            path.to_string(),
        ));
    }
    build_env_commands(shell, &patches)
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
