use std::sync::Arc;

use crate::config::{Config, Settings};
use crate::env;
use crate::file::replace_path;
use crate::{dirs, file, result};
use eyre::WrapErr;
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
    static MUTEX: Mutex<()> = Mutex::const_new(());
    let age = AGE_KEY
        .get_or_init(async || {
            let p = Settings::get()
                .sops
                .age_key_file
                .clone()
                .unwrap_or(dirs::CONFIG.join("age.txt"));
            let p = replace_path(match parse_template(p.to_string_lossy().to_string()) {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to parse sops age key file: {}", e);
                    return None;
                }
            });
            if let Some(age_key) = &Settings::get().sops.age_key {
                if !age_key.is_empty() {
                    return Some(age_key.clone());
                }
            }
            if p.exists() {
                if let Ok(raw) = file::read_to_string(p) {
                    let key = raw
                        .trim()
                        .lines()
                        .filter(|l| !l.starts_with('#'))
                        .collect::<String>();
                    if !key.trim().is_empty() {
                        return Some(key);
                    }
                }
            }
            None
        })
        .await;
    let _lock = MUTEX.lock().await; // prevent multiple threads from using the same age key
    let age_env_key = if Settings::get().sops.rops {
        "ROPS_AGE"
    } else {
        "SOPS_AGE_KEY"
    };
    let prev_age = env::var(age_env_key).ok();
    if let Some(age) = &age {
        env::set_var(age_env_key, age.trim());
    }
    let output = if Settings::get().sops.rops {
        input
            .parse::<RopsFile<EncryptedFile<AES256GCM, SHA512>, F>>()
            .wrap_err("failed to parse sops file")?
            .decrypt::<F>()
            .wrap_err("failed to decrypt sops file")?
            .to_string()
    } else {
        let mut ts = config
            .get_tool_request_set()
            .await
            .cloned()
            .unwrap_or_default()
            .filter_by_tool(["sops".into()].into())
            .into_toolset();
        Box::pin(ts.resolve(config)).await?;
        let sops = ts
            .which_bin(config, "sops")
            .await
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or("sops".into());
        // TODO: this obviously won't work on windows
        cmd!(
            sops,
            "--input-type",
            format,
            "--output-type",
            format,
            "-d",
            "/dev/stdin"
        )
        .stdin_bytes(input.as_bytes())
        .read()?
    };

    if let Some(age) = prev_age {
        env::set_var(age_env_key, age);
    } else {
        env::remove_var(age_env_key);
    }
    Ok(output)
}
