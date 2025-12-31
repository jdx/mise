use std::sync::Arc;

use crate::config::{Config, Settings};
use crate::env;
use crate::file::replace_path;
use crate::{dirs, file, result};
use eyre::{WrapErr, eyre};
use rops::cryptography::cipher::AES256GCM;
use rops::cryptography::hasher::SHA512;
use rops::file::RopsFile;
use rops::file::state::EncryptedFile;
use tokio::sync::{Mutex, OnceCell};

pub async fn decrypt<PT, F>(
    config: &Arc<Config>,
    input: &str,
    mut parse_template: PT,
    format: &str,
) -> result::Result<String>
where
    PT: FnMut(String) -> result::Result<String>,
    F: rops::file::format::FileFormat,
{
    static AGE_KEY: OnceCell<Option<String>> = OnceCell::const_new();
    static AGE_KEY_FILE: OnceCell<Option<std::path::PathBuf>> = OnceCell::const_new();
    static MUTEX: Mutex<()> = Mutex::const_new(());

    let age = AGE_KEY
        .get_or_init(async || {
            // 1. Check mise-specific MISE_SOPS_AGE_KEY setting first (highest priority)
            if let Some(age_key) = &Settings::get().sops.age_key
                && !age_key.is_empty()
            {
                return Some(age_key.clone());
            }

            // 2. Check mise-specific MISE_SOPS_AGE_KEY_FILE setting
            if let Some(key_file) = &Settings::get().sops.age_key_file {
                let p = replace_path(
                    match parse_template(key_file.to_string_lossy().to_string()) {
                        Ok(p) => p,
                        Err(e) => {
                            warn!("failed to parse MISE_SOPS_AGE_KEY_FILE: {}", e);
                            return None;
                        }
                    },
                );
                if p.exists()
                    && let Ok(raw) = file::read_to_string(&p)
                {
                    let key = raw
                        .trim()
                        .lines()
                        .filter(|l| !l.starts_with('#'))
                        .collect::<String>();
                    if !key.trim().is_empty() {
                        // Store the path for later use by sops CLI
                        let _ = AGE_KEY_FILE.get_or_init(|| async { Some(p.clone()) }).await;
                        return Some(key);
                    }
                }
            }

            // 3. Check standard SOPS_AGE_KEY_FILE environment variable
            if let Ok(key_file_path) = env::var("SOPS_AGE_KEY_FILE") {
                let p = replace_path(match parse_template(key_file_path.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("failed to parse SOPS_AGE_KEY_FILE: {}", e);
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
                        // Store the path for later use by sops CLI
                        let _ = AGE_KEY_FILE.get_or_init(|| async { Some(p.clone()) }).await;
                        return Some(key);
                    }
                }
            }

            // 4. Check standard SOPS_AGE_KEY environment variable (direct key content)
            if let Ok(key) = env::var("SOPS_AGE_KEY")
                && !key.trim().is_empty()
            {
                return Some(key.trim().to_string());
            }

            // 5. Fall back to default path ~/.config/mise/age.txt
            let p = dirs::CONFIG.join("age.txt");
            let p = replace_path(match parse_template(p.to_string_lossy().to_string()) {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to parse default sops age key file: {}", e);
                    return None;
                }
            });
            if p.exists()
                && let Ok(raw) = file::read_to_string(p.clone())
            {
                let key = raw
                    .trim()
                    .lines()
                    .filter(|l| !l.starts_with('#'))
                    .collect::<String>();
                if !key.trim().is_empty() {
                    // Store the path for later use by sops CLI
                    let _ = AGE_KEY_FILE.get_or_init(|| async { Some(p.clone()) }).await;
                    return Some(key);
                }
            }
            None
        })
        .await;

    if age.is_none() && !Settings::get().sops.strict {
        debug!("age key not found, skipping decryption in non-strict mode");
        return Ok(String::new());
    }

    let _lock = MUTEX.lock().await; // prevent multiple threads from using the same age key
    let age_env_key = if Settings::get().sops.rops {
        "ROPS_AGE"
    } else {
        "SOPS_AGE_KEY"
    };
    let prev_age = env::var(age_env_key).ok();
    let prev_age_key_file = env::var("SOPS_AGE_KEY_FILE").ok();

    // Set SOPS_AGE_KEY_FILE with expanded path if we found one, so sops CLI can use it
    if let Some(expanded_path) = AGE_KEY_FILE.get().and_then(|f| f.as_ref()) {
        env::set_var(
            "SOPS_AGE_KEY_FILE",
            expanded_path.to_string_lossy().to_string(),
        );
    }

    if let Some(age) = &age {
        env::set_var(age_env_key, age.trim());
    }
    let output = if Settings::get().sops.rops {
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
        let mut ts = config
            .get_tool_request_set()
            .await
            .cloned()
            .unwrap_or_default()
            .filter_by_tool(["sops".into()].into())
            .into_toolset();
        Box::pin(ts.resolve(config)).await?;
        let sops_path = ts.which_bin(config, "sops").await;

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
