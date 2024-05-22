use std::collections::{BTreeMap, BTreeSet};
use std::io::prelude::*;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use base64::prelude::*;
use eyre::Result;
use flate2::write::{ZlibDecoder, ZlibEncoder};
use flate2::Compression;
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};

use crate::env_diff::{EnvDiffOperation, EnvDiffPatches};
use crate::hash::hash_to_str;
use crate::shell::Shell;
use crate::{dirs, env};

/// this function will early-exit the application if hook-env is being
/// called and it does not need to be
pub fn should_exit_early(watch_files: impl IntoIterator<Item = impl AsRef<Path>>) -> bool {
    let args = env::ARGS.read().unwrap();
    if args.len() < 2 || args[1] != "hook-env" {
        return false;
    }
    let watch_files = get_watch_files(watch_files);
    match &*env::__MISE_WATCH {
        Some(watches) => {
            if have_config_files_been_modified(watches, watch_files) {
                return false;
            }
            if have_mise_env_vars_been_modified(watches) {
                return false;
            }
        }
        None => {
            return false;
        }
    };
    trace!("early-exit");
    true
}

fn have_config_files_been_modified(
    watches: &HookEnvWatches,
    watch_files: BTreeSet<PathBuf>,
) -> bool {
    // make sure they have exactly the same config filenames
    let watch_keys = watches.files.keys().cloned().collect::<BTreeSet<_>>();
    if watch_keys != watch_files {
        trace!(
            "config files do not match {:?}",
            watch_keys.symmetric_difference(&watch_files)
        );
        return true;
    }

    // check the files to see if they've been altered
    for (fp, prev_modtime) in &watches.files {
        if let Ok(modtime) = fp
            .metadata()
            .expect("accessing config file modtime")
            .modified()
        {
            if &modtime != prev_modtime {
                trace!("config file modified: {:?}", fp);
                return true;
            }
        }
    }
    trace!("config files unmodified");
    false
}

fn have_mise_env_vars_been_modified(watches: &HookEnvWatches) -> bool {
    if get_mise_env_vars_hashed() != watches.env_var_hash {
        return true;
    }
    false
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HookEnvWatches {
    files: BTreeMap<PathBuf, SystemTime>,
    env_var_hash: String,
}

pub fn serialize_watches(watches: &HookEnvWatches) -> Result<String> {
    let mut gz = ZlibEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&rmp_serde::to_vec_named(watches)?)?;
    Ok(BASE64_STANDARD_NO_PAD.encode(gz.finish()?))
}

pub fn deserialize_watches(raw: String) -> Result<HookEnvWatches> {
    let mut writer = Vec::new();
    let mut decoder = ZlibDecoder::new(writer);
    let bytes = BASE64_STANDARD_NO_PAD.decode(raw)?;
    decoder.write_all(&bytes[..])?;
    writer = decoder.finish()?;
    Ok(rmp_serde::from_slice(&writer[..])?)
}

pub fn build_watches(
    watch_files: impl IntoIterator<Item = impl AsRef<Path>>,
) -> Result<HookEnvWatches> {
    let mut watches = BTreeMap::new();
    for cf in get_watch_files(watch_files) {
        watches.insert(cf.clone(), cf.metadata()?.modified()?);
    }

    Ok(HookEnvWatches {
        files: watches,
        env_var_hash: get_mise_env_vars_hashed(),
    })
}

pub fn get_watch_files(
    watch_files: impl IntoIterator<Item = impl AsRef<Path>>,
) -> BTreeSet<PathBuf> {
    let mut watches = BTreeSet::new();
    if dirs::DATA.exists() {
        watches.insert(dirs::DATA.to_path_buf());
    }
    for cf in watch_files {
        watches.insert(cf.as_ref().to_path_buf());
    }

    watches
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
    if let Some(path) = env::PRISTINE_ENV.deref().get("PATH") {
        patches.push(EnvDiffOperation::Change("PATH".into(), path.to_string()));
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

#[cfg(test)]
mod tests {
    use crate::test::reset;
    use std::time::UNIX_EPOCH;

    use super::*;
    use test_log::test;

    #[test(tokio::test)]
    async fn test_have_config_files_been_modified() {
        reset().await;
        let files = BTreeSet::new();
        let watches = HookEnvWatches {
            files: BTreeMap::new(),
            env_var_hash: "".into(),
        };
        assert!(!have_config_files_been_modified(&watches, files));

        let fp = env::current_dir().unwrap().join(".test-tool-versions");
        let watches = HookEnvWatches {
            files: BTreeMap::from([(fp.clone(), UNIX_EPOCH)]),
            env_var_hash: "".into(),
        };
        let files = BTreeSet::from([fp.clone()]);
        assert!(have_config_files_been_modified(&watches, files));

        let modtime = fp.metadata().unwrap().modified().unwrap();
        let watches = HookEnvWatches {
            files: BTreeMap::from([(fp.clone(), modtime)]),
            env_var_hash: "".into(),
        };
        let files = BTreeSet::from([fp]);
        assert!(!have_config_files_been_modified(&watches, files));
    }

    #[test(tokio::test)]
    async fn test_serialize_watches_empty() {
        reset().await;
        let watches = HookEnvWatches {
            files: BTreeMap::new(),
            env_var_hash: "".into(),
        };
        let serialized = serialize_watches(&watches).unwrap();
        let deserialized = deserialize_watches(serialized).unwrap();
        assert_eq!(deserialized.files.len(), 0);
    }

    #[test(tokio::test)]
    async fn test_serialize_watches() {
        reset().await;
        let serialized = serialize_watches(&HookEnvWatches {
            files: BTreeMap::from([("foo".into(), UNIX_EPOCH)]),
            env_var_hash: "testing-123".into(),
        })
        .unwrap();
        let deserialized = deserialize_watches(serialized).unwrap();
        assert_eq!(deserialized.files.len(), 1);
        assert_str_eq!(deserialized.env_var_hash, "testing-123");
        assert_eq!(
            deserialized
                .files
                .get(PathBuf::from("foo").as_path())
                .unwrap(),
            &UNIX_EPOCH
        );
    }
}
