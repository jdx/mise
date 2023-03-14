use std::io::prelude::*;
use std::ops::Deref;
use std::path::PathBuf;
use std::time::SystemTime;

use base64::prelude::*;
use color_eyre::eyre::Result;
use flate2::write::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};

use crate::env_diff::{EnvDiffOperation, EnvDiffPatches};
use crate::hash::hash_to_str;
use crate::shell::Shell;
use crate::{dirs, env};

/// this function will early-exit the application if hook-env is being
/// called and it does not need to be
pub fn should_exit_early(config_filenames: &[PathBuf]) -> bool {
    if env::ARGS.len() < 2 || env::ARGS[1] != "hook-env" {
        return false;
    }
    let watch_files = get_watch_files(config_filenames);
    match env::var("__RTX_WATCH") {
        Ok(raw) => {
            match deserialize_watches(raw) {
                Ok(watches) => {
                    if have_config_files_been_modified(&watches, watch_files) {
                        return false;
                    }
                    if have_rtx_env_vars_been_modified(&watches) {
                        return false;
                    }
                }
                Err(e) => {
                    debug!("error deserializing watches: {:?}", e);
                    return false;
                }
            };
        }
        Err(_) => {
            // __RTX_WATCH is not set
            return false;
        }
    };
    trace!("early-exit");
    true
}

fn have_config_files_been_modified(
    watches: &HookEnvWatches,
    watch_files: IndexSet<PathBuf>,
) -> bool {
    // make sure they have exactly the same config filenames
    let watch_keys = watches.files.keys().cloned().collect::<IndexSet<_>>();
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

fn have_rtx_env_vars_been_modified(watches: &HookEnvWatches) -> bool {
    if get_rtx_env_vars_hashed() != watches.env_var_hash {
        return true;
    }
    false
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HookEnvWatches {
    files: IndexMap<PathBuf, SystemTime>,
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

pub fn build_watches(config_filenames: &[PathBuf]) -> Result<HookEnvWatches> {
    let mut watches = IndexMap::new();
    for cf in get_watch_files(config_filenames) {
        watches.insert(cf.clone(), cf.metadata()?.modified()?);
    }

    Ok(HookEnvWatches {
        files: watches,
        env_var_hash: get_rtx_env_vars_hashed(),
    })
}

pub fn get_watch_files(config_filenames: &[PathBuf]) -> IndexSet<PathBuf> {
    let mut watches = IndexSet::new();
    if dirs::ROOT.exists() {
        watches.insert(dirs::ROOT.clone());
    }
    for cf in config_filenames {
        watches.insert(cf.clone());
    }

    watches
}

/// gets a hash of all RTX_ environment variables
fn get_rtx_env_vars_hashed() -> String {
    let env_vars: Vec<(&String, &String)> = env::PRISTINE_ENV
        .deref()
        .iter()
        .filter(|(k, _)| k.starts_with("RTX_"))
        .sorted()
        .collect();
    hash_to_str(&env_vars)
}

pub fn clear_old_env(shell: &dyn Shell) -> String {
    let mut patches = env::__RTX_DIFF.reverse().to_patches();
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
    use std::time::UNIX_EPOCH;

    use pretty_assertions::assert_str_eq;

    use crate::dirs;

    use super::*;

    #[test]
    fn test_have_config_files_been_modified() {
        let files = IndexSet::new();
        let watches = HookEnvWatches {
            files: IndexMap::new(),
            env_var_hash: "".into(),
        };
        assert!(!have_config_files_been_modified(&watches, files));

        let fp = dirs::CURRENT.join(".test-tool-versions");
        let watches = HookEnvWatches {
            files: IndexMap::from([(fp.clone(), UNIX_EPOCH)]),
            env_var_hash: "".into(),
        };
        let files = IndexSet::from([fp.clone()]);
        assert!(have_config_files_been_modified(&watches, files));

        let modtime = fp.metadata().unwrap().modified().unwrap();
        let watches = HookEnvWatches {
            files: IndexMap::from([(fp.clone(), modtime)]),
            env_var_hash: "".into(),
        };
        let files = IndexSet::from([fp]);
        assert!(!have_config_files_been_modified(&watches, files));
    }

    #[test]
    fn test_serialize_watches_empty() {
        let watches = HookEnvWatches {
            files: IndexMap::new(),
            env_var_hash: "".into(),
        };
        let serialized = serialize_watches(&watches).unwrap();
        let deserialized = deserialize_watches(serialized).unwrap();
        assert_eq!(deserialized.files.len(), 0);
    }

    #[test]
    fn test_serialize_watches() {
        let serialized = serialize_watches(&HookEnvWatches {
            files: IndexMap::from([("foo".into(), UNIX_EPOCH)]),
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
