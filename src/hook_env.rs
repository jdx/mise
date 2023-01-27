use std::collections::HashMap;
use std::io::prelude::*;
use std::path::PathBuf;
use std::time::SystemTime;

use base64::prelude::*;
use color_eyre::eyre::Result;
use flate2::write::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::{dirs, env};

/// this function will early-exit the application if hook-env is being
/// called and it does not need to be
pub fn should_exit_early(current_env: HashMap<String, String>) -> bool {
    if env::ARGS.len() < 2 || env::ARGS[1] != "hook-env" {
        return false;
    }
    if has_rf_path_changed(&current_env) {
        return false;
    }
    if has_watch_file_been_modified(&current_env) {
        return false;
    }
    true
}

fn has_rf_path_changed(env: &HashMap<String, String>) -> bool {
    if let Some(prev) = env.get("__RTX_DIR").map(PathBuf::from) {
        if prev == dirs::CURRENT.as_path() {
            return false;
        }
    }
    true
}

fn has_watch_file_been_modified(env: &HashMap<String, String>) -> bool {
    if let Some(prev) = env.get("__RTX_WATCH") {
        let watches = deserialize_watches(prev.to_string()).unwrap();
        for (fp, prev_modtime) in watches {
            if !fp.exists() {
                return true;
            }
            if let Ok(modtime) = fp.metadata().unwrap().modified() {
                if modtime != prev_modtime {
                    return true;
                }
            }
        }
        return false;
    }
    true
}

pub type HookEnvWatches = HashMap<PathBuf, SystemTime>;

pub fn serialize_watches(watches: &HookEnvWatches) -> Result<String> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&rmp_serde::to_vec_named(watches)?)?;
    Ok(BASE64_STANDARD_NO_PAD.encode(gz.finish()?))
}

pub fn deserialize_watches(raw: String) -> Result<HookEnvWatches> {
    let mut writer = Vec::new();
    let mut decoder = GzDecoder::new(writer);
    let bytes = BASE64_STANDARD_NO_PAD.decode(raw)?;
    decoder.write_all(&bytes[..])?;
    writer = decoder.finish()?;
    Ok(rmp_serde::from_slice(&writer[..])?)
}

#[cfg(test)]
mod test {
    use std::time::UNIX_EPOCH;

    use super::*;

    #[test]
    fn test_has_rf_path_changed() {
        let mut env = HashMap::new();
        assert!(has_rf_path_changed(&env));
        env.insert("__RTX_DIR".into(), dirs::CURRENT.to_string_lossy().into());
        assert!(!has_rf_path_changed(&env));
        env.insert("__RTX_DIR".into(), dirs::HOME.to_string_lossy().into());
        assert!(has_rf_path_changed(&env));
    }

    #[test]
    fn test_has_watch_file_been_modified() {
        let mut env = HashMap::new();
        assert!(has_watch_file_been_modified(&env));
        let fp = dirs::CURRENT.join(".tool-versions");
        env.insert(
            "__RTX_WATCH".into(),
            serialize_watches(&HookEnvWatches::from([(fp.clone(), UNIX_EPOCH)])).unwrap(),
        );
        assert!(has_watch_file_been_modified(&env));
        let modtime = fp.metadata().unwrap().modified().unwrap();
        env.insert(
            "__RTX_WATCH".into(),
            serialize_watches(&HookEnvWatches::from([(fp, modtime)])).unwrap(),
        );
        assert!(!has_watch_file_been_modified(&env));
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
