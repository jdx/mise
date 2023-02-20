use std::collections::{HashMap, HashSet};
use std::io::prelude::*;
use std::path::PathBuf;
use std::time::SystemTime;

use base64::prelude::*;
use color_eyre::eyre::Result;
use flate2::write::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::config::Config;
use crate::{dirs, env};

/// this function will early-exit the application if hook-env is being
/// called and it does not need to be
pub fn should_exit_early(config: &Config) -> bool {
    if env::ARGS.len() < 2 || env::ARGS[1] != "hook-env" {
        return false;
    }
    let watch_files = get_watch_files(config);
    if have_config_files_been_modified(&env::vars().collect(), watch_files) {
        return false;
    }
    trace!("early-exit");
    true
}

fn have_config_files_been_modified(
    env: &HashMap<String, String>,
    watch_files: HashSet<PathBuf>,
) -> bool {
    match env.get("__RTX_WATCH") {
        Some(prev) => {
            let watches = match deserialize_watches(prev.to_string()) {
                Ok(watches) => watches,
                Err(e) => {
                    debug!("error deserializing watches: {:?}", e);
                    return true;
                }
            };

            // make sure they have exactly the same config filenames
            let watch_keys = watches.keys().cloned().collect::<HashSet<_>>();
            if watch_keys != watch_files {
                trace!(
                    "config files do not match {:?}",
                    watch_keys.symmetric_difference(&watch_files)
                );
                return true;
            }

            // check the files to see if they've been altered
            for (fp, prev_modtime) in watches {
                if let Ok(modtime) = fp
                    .metadata()
                    .expect("accessing config file modtime")
                    .modified()
                {
                    if modtime != prev_modtime {
                        trace!("config file modified: {:?}", fp);
                        return true;
                    }
                }
            }
            trace!("config files unmodified");
            false
        }
        _ => true, // no previous watch data, we say they have been modified, so we don't exit early
    }
}

pub type HookEnvWatches = HashMap<PathBuf, SystemTime>;

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

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use crate::dirs;

    use super::*;

    #[test]
    fn test_have_config_files_been_modified() {
        let mut env = HashMap::new();
        let files = HashSet::new();
        assert!(have_config_files_been_modified(&env, files));

        let fp = dirs::CURRENT.join(".tool-versions");
        env.insert(
            "__RTX_WATCH".into(),
            serialize_watches(&HookEnvWatches::from([(fp.clone(), UNIX_EPOCH)])).unwrap(),
        );
        let files = HashSet::from([fp.clone()]);
        assert!(have_config_files_been_modified(&env, files));

        let modtime = fp.metadata().unwrap().modified().unwrap();
        env.insert(
            "__RTX_WATCH".into(),
            serialize_watches(&HookEnvWatches::from([(fp.clone(), modtime)])).unwrap(),
        );
        let files = HashSet::from([fp]);
        assert!(!have_config_files_been_modified(&env, files));
    }

    #[test]
    fn test_serialize_watches_empty() {
        let serialized = serialize_watches(&HookEnvWatches::new()).unwrap();
        let deserialized = deserialize_watches(serialized).unwrap();
        assert_eq!(deserialized.len(), 0);
    }

    #[test]
    fn test_serialize_watches() {
        let serialized =
            serialize_watches(&HookEnvWatches::from([("foo".into(), UNIX_EPOCH)])).unwrap();
        let deserialized = deserialize_watches(serialized).unwrap();
        assert_eq!(deserialized.len(), 1);
        assert_eq!(
            deserialized.get(PathBuf::from("foo").as_path()).unwrap(),
            &UNIX_EPOCH
        );
    }
}

pub fn build_watches(config: &Config) -> Result<HookEnvWatches> {
    let mut watches = HookEnvWatches::new();
    for cf in get_watch_files(config) {
        watches.insert(cf.clone(), cf.metadata()?.modified()?);
    }

    Ok(watches)
}

pub fn get_watch_files(config: &Config) -> HashSet<PathBuf> {
    let mut watches = HashSet::new();
    if dirs::ROOT.exists() {
        watches.insert(dirs::ROOT.clone());
    }
    for cf in &config.config_files {
        watches.insert(cf.clone());
    }

    watches
}
