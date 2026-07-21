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

pub async fn decrypt<PT, F>(
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

    let (age, age_key_file) = resolve_age_key(exec_env, &mut parse_template);

    if age.is_none() && !Settings::get().sops.strict {
        debug!("age key not found, skipping decryption in non-strict mode");
        return Ok(String::new());
    }

    let _lock = MUTEX.lock().await; // prevent multiple threads from using the same age key
    let age_env_key = if use_rops { "ROPS_AGE" } else { "SOPS_AGE_KEY" };
    let prev_age = env::var(age_env_key).ok();
    let prev_age_key_file = env::var("SOPS_AGE_KEY_FILE").ok();

    // Set SOPS_AGE_KEY_FILE with expanded path if we found one, so sops CLI can use it
    if let Some(expanded_path) = &age_key_file {
        env::set_var(
            "SOPS_AGE_KEY_FILE",
            expanded_path.to_string_lossy().to_string(),
        );
    }

    if let Some(age) = &age {
        env::set_var(age_env_key, age.trim());
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
                // TODO: this obviously won't work on windows
                match cmd!(
                    sops,
                    "--input-type",
                    format,
                    "--output-type",
                    format,
                    "-d",
                    "/dev/stdin"
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

fn resolve_age_key<PT>(
    env: &EnvMap,
    parse_template: &mut PT,
) -> (Option<String>, Option<std::path::PathBuf>)
where
    PT: FnMut(String) -> result::Result<String>,
{
    // 1. Check mise-specific MISE_SOPS_AGE_KEY setting first (highest priority)
    if let Some(age_key) = &Settings::get().sops.age_key
        && !age_key.is_empty()
    {
        return (Some(age_key.clone()), None);
    }

    // 2. Check mise-specific MISE_SOPS_AGE_KEY_FILE setting
    if let Some(key_file) = &Settings::get().sops.age_key_file
        && let Some((key, path)) = read_age_key_file(
            key_file.to_string_lossy().to_string(),
            parse_template,
            "MISE_SOPS_AGE_KEY_FILE",
        )
    {
        return (Some(key), Some(path));
    }

    // 3. Check ordered env directives that have already been resolved
    if let Some(age_key) = env.get("MISE_SOPS_AGE_KEY").filter(|key| !key.is_empty()) {
        return (Some(age_key.clone()), None);
    }

    if let Some(key_file) = env.get("MISE_SOPS_AGE_KEY_FILE")
        && let Some((key, path)) =
            read_age_key_file(key_file.clone(), parse_template, "MISE_SOPS_AGE_KEY_FILE")
    {
        return (Some(key), Some(path));
    }

    if let Some(key_file) = env.get("SOPS_AGE_KEY_FILE")
        && let Some((key, path)) =
            read_age_key_file(key_file.clone(), parse_template, "SOPS_AGE_KEY_FILE")
    {
        return (Some(key), Some(path));
    }

    // 4. Check standard SOPS environment variables
    if let Ok(key_file_path) = env::var("SOPS_AGE_KEY_FILE")
        && let Some((key, path)) =
            read_age_key_file(key_file_path, parse_template, "SOPS_AGE_KEY_FILE")
    {
        return (Some(key), Some(path));
    }

    if let Some(age_key) = env.get("SOPS_AGE_KEY").filter(|key| !key.trim().is_empty()) {
        return (Some(age_key.trim().to_string()), None);
    }

    if let Ok(key) = env::var("SOPS_AGE_KEY")
        && !key.trim().is_empty()
    {
        return (Some(key.trim().to_string()), None);
    }

    // 5. Fall back to default path ~/.config/mise/age.txt
    if let Some((key, path)) = read_age_key_file(
        dirs::CONFIG.join("age.txt").to_string_lossy().to_string(),
        parse_template,
        "default sops age key file",
    ) {
        return (Some(key), Some(path));
    }

    (None, None)
}

fn read_age_key_file<PT>(
    key_file_path: String,
    parse_template: &mut PT,
    source: &str,
) -> Option<(String, std::path::PathBuf)>
where
    PT: FnMut(String) -> result::Result<String>,
{
    let p = replace_path(match parse_template(key_file_path) {
        Ok(p) => p,
        Err(e) => {
            warn!("failed to parse {source}: {e}");
            return None;
        }
    });
    if p.exists()
        && let Ok(raw) = file::read_to_string(&p)
    {
        let key = raw
            .trim()
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<String>();
        if !key.trim().is_empty() {
            return Some((key, p));
        }
    }
    None
}
