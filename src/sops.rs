use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::configured_toolset_or_path_which;
use crate::config::{Config, Settings};
use crate::env;
use crate::env_diff::EnvMap;
use crate::file::replace_path;
use crate::{dirs, file, result};
use eyre::{WrapErr, eyre};
use rops::cryptography::cipher::AES256GCM;
use rops::cryptography::hasher::SHA512;
use rops::file::RopsFile;
use rops::file::state::EncryptedFile;
use tokio::sync::Mutex;

#[derive(Debug, PartialEq)]
enum ResolvedAgeKey {
    Direct(String),
    File {
        identities: Vec<String>,
        path: PathBuf,
    },
}

impl ResolvedAgeKey {
    fn env_value(&self, use_rops: bool) -> String {
        match self {
            Self::Direct(key) => key.clone(),
            Self::File { identities, .. } if use_rops => identities.join(","),
            Self::File { identities, .. } => identities.join("\n"),
        }
    }

    fn file_path(&self) -> Option<&Path> {
        match self {
            Self::Direct(_) => None,
            Self::File { path, .. } => Some(path.as_path()),
        }
    }
}

pub(crate) async fn decrypt<PT, F>(
    config: &Arc<Config>,
    exec_env: &EnvMap,
    input: &str,
    mut parse_template: PT,
    format: &str,
) -> result::Result<String>
where
    PT: FnMut(String) -> result::Result<String>,
    F: rops::file::format::FileFormat,
{
    static MUTEX: Mutex<()> = Mutex::const_new(());

    let use_rops = Settings::get().sops.rops;
    if !use_rops && format == "toml" {
        return Err(eyre!(
            "sops.rops=false is not supported for TOML SOPS files because the sops CLI does not support TOML; set sops.rops=true or use a JSON/YAML SOPS file"
        ));
    }

    let age = resolve_age_key(exec_env, &mut parse_template);

    if use_rops && age.is_none() && !Settings::get().sops.strict {
        debug!("age key not found, skipping decryption in non-strict mode");
        return Ok(String::new());
    }

    let _lock = MUTEX.lock().await; // prevent multiple threads from using the same age key
    let age_env_key = if use_rops { "ROPS_AGE" } else { "SOPS_AGE_KEY" };
    let prev_age = env::var(age_env_key).ok();
    let prev_age_key_file = env::var("SOPS_AGE_KEY_FILE").ok();

    // Set SOPS_AGE_KEY_FILE with expanded path if we found one, so sops CLI can use it
    if let Some(expanded_path) = age.as_ref().and_then(ResolvedAgeKey::file_path) {
        env::set_var(
            "SOPS_AGE_KEY_FILE",
            expanded_path.to_string_lossy().to_string(),
        );
    }

    if let Some(age) = &age {
        env::set_var(age_env_key, age.env_value(use_rops).trim());
    }
    let output = if use_rops {
        match input
            .parse::<RopsFile<EncryptedFile<AES256GCM, SHA512>, F>>()
            .wrap_err("failed to parse sops file")
            .and_then(|file| file.decrypt::<F>().wrap_err("failed to decrypt sops file"))
        {
            Ok(decrypted) => Some(decrypted.to_string()),
            Err(e) => {
                if Settings::get().sops.strict {
                    if let Some(age) = prev_age {
                        env::set_var(age_env_key, age);
                    } else {
                        env::remove_var(age_env_key);
                    }
                    if let Some(age_key_file) = prev_age_key_file {
                        env::set_var("SOPS_AGE_KEY_FILE", age_key_file);
                    } else {
                        env::remove_var("SOPS_AGE_KEY_FILE");
                    }
                    return Err(e);
                } else {
                    debug!(
                        "sops decryption failed but continuing in non-strict mode: {}",
                        e
                    );
                    None
                }
            }
        }
    } else {
        let sops_path =
            configured_toolset_or_path_which(config, ["sops".to_string()], "sops").await?;

        match sops_path {
            None => {
                if Settings::get().sops.strict {
                    if let Some(age) = prev_age {
                        env::set_var(age_env_key, age);
                    } else {
                        env::remove_var(age_env_key);
                    }
                    if let Some(age_key_file) = prev_age_key_file {
                        env::set_var("SOPS_AGE_KEY_FILE", age_key_file);
                    } else {
                        env::remove_var("SOPS_AGE_KEY_FILE");
                    }
                    return Err(eyre!("sops command not found"));
                } else {
                    debug!("sops command not found, skipping decryption in non-strict mode");
                    None
                }
            }
            Some(sops_path) => {
                let sops = sops_path.to_string_lossy().to_string();
                // sops reads stdin when no input filename is provided.
                match cmd!(
                    sops,
                    "decrypt",
                    "--input-type",
                    format,
                    "--output-type",
                    format
                )
                .stdin_bytes(input.as_bytes())
                .read()
                {
                    Ok(output) => Some(output),
                    Err(e) => {
                        if Settings::get().sops.strict {
                            if let Some(age) = prev_age {
                                env::set_var(age_env_key, age);
                            } else {
                                env::remove_var(age_env_key);
                            }
                            if let Some(age_key_file) = prev_age_key_file {
                                env::set_var("SOPS_AGE_KEY_FILE", age_key_file);
                            } else {
                                env::remove_var("SOPS_AGE_KEY_FILE");
                            }
                            return Err(e.into());
                        } else {
                            debug!(
                                "sops decryption failed but continuing in non-strict mode: {}",
                                e
                            );
                            None
                        }
                    }
                }
            }
        }
    };

    if let Some(age) = prev_age {
        env::set_var(age_env_key, age);
    } else {
        env::remove_var(age_env_key);
    }
    if let Some(age_key_file) = prev_age_key_file {
        env::set_var("SOPS_AGE_KEY_FILE", age_key_file);
    } else {
        env::remove_var("SOPS_AGE_KEY_FILE");
    }
    Ok(output.unwrap_or_default())
}

fn resolve_age_key<PT>(env: &EnvMap, parse_template: &mut PT) -> Option<ResolvedAgeKey>
where
    PT: FnMut(String) -> result::Result<String>,
{
    // 1. Check mise-specific MISE_SOPS_AGE_KEY setting first (highest priority)
    if let Some(age_key) = &Settings::get().sops.age_key
        && !age_key.is_empty()
    {
        return Some(ResolvedAgeKey::Direct(age_key.clone()));
    }

    // 2. Check mise-specific MISE_SOPS_AGE_KEY_FILE setting. Only a value written in a config
    // file is a template. One from the environment is a path: it was rendered by `[env]` if it
    // was declared there, and otherwise it may be text from an env file.
    if let Some(key_file) = &Settings::get().sops.age_key_file {
        let key_file = key_file.to_string_lossy().to_string();
        let key_file = if env::var_os("MISE_SOPS_AGE_KEY_FILE").is_some() {
            Some(key_file)
        } else {
            parse_template(key_file)
                .inspect_err(|e| warn!("failed to parse sops.age_key_file: {e}"))
                .ok()
        };
        if let Some(key) = key_file.and_then(read_age_key_file) {
            return Some(key);
        }
    }

    // 3. Check ordered env directives that have already been resolved. Rendering these values
    // again would run a template that came from an env file or from a directory name.
    if let Some(age_key) = env.get("MISE_SOPS_AGE_KEY").filter(|key| !key.is_empty()) {
        return Some(ResolvedAgeKey::Direct(age_key.clone()));
    }

    if let Some(key_file) = env.get("MISE_SOPS_AGE_KEY_FILE")
        && let Some(key) = read_age_key_file(key_file)
    {
        return Some(key);
    }

    if let Some(key_file) = env.get("SOPS_AGE_KEY_FILE")
        && let Some(key) = read_age_key_file(key_file)
    {
        return Some(key);
    }

    // 4. Check standard SOPS environment variables
    if let Ok(key_file_path) = env::var("SOPS_AGE_KEY_FILE")
        && let Some(key) = read_age_key_file(key_file_path)
    {
        return Some(key);
    }

    if let Some(age_key) = env.get("SOPS_AGE_KEY").filter(|key| !key.trim().is_empty()) {
        return Some(ResolvedAgeKey::Direct(age_key.trim().to_string()));
    }

    if let Ok(key) = env::var("SOPS_AGE_KEY")
        && !key.trim().is_empty()
    {
        return Some(ResolvedAgeKey::Direct(key.trim().to_string()));
    }

    // 5. Fall back to default path ~/.config/mise/age.txt
    if let Some(key) = read_age_key_file(dirs::CONFIG.join("age.txt")) {
        return Some(key);
    }

    None
}

fn read_age_key_file(key_file_path: impl AsRef<Path>) -> Option<ResolvedAgeKey> {
    let p = replace_path(key_file_path);
    if p.exists()
        && let Ok(raw) = file::read_to_string(&p)
    {
        let identities = raw
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            })
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !identities.is_empty() {
            return Some(ResolvedAgeKey::File {
                identities,
                path: p,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_key_file(contents: &str) -> Option<ResolvedAgeKey> {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("keys.txt");
        file::write(&path, contents).unwrap();
        read_age_key_file(path)
    }

    #[test]
    fn reads_multiple_age_keys_in_order() {
        let key =
            read_key_file("# first key is unrelated\r\nKEY-1\r\n\r\n# matching key\r\nKEY-2\r\n")
                .unwrap();
        let ResolvedAgeKey::File { identities, .. } = &key else {
            panic!("expected key file");
        };
        assert_eq!(identities, &["KEY-1", "KEY-2"]);
        assert_eq!(key.env_value(true), "KEY-1,KEY-2");
        assert_eq!(key.env_value(false), "KEY-1\nKEY-2");
    }

    #[test]
    fn preserves_invalid_non_comment_lines() {
        let key = read_key_file("not-an-age-key\n").unwrap();
        let ResolvedAgeKey::File { identities, .. } = key else {
            panic!("expected key file");
        };
        assert_eq!(identities, &["not-an-age-key"]);
    }

    #[test]
    fn ignores_empty_and_comment_only_key_files() {
        assert_eq!(
            read_key_file("\n  \t\r\n# no identities\r\n  # indented comment\r\n"),
            None
        );
    }
}
