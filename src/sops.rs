use crate::config::SETTINGS;
use crate::file::replace_path;
use crate::{dirs, file, result};
use eyre::WrapErr;
use rops::cryptography::cipher::AES256GCM;
use rops::cryptography::hasher::SHA512;
use rops::file::state::EncryptedFile;
use rops::file::RopsFile;
use std::env;

pub fn decrypt<PT, F>(input: &str, parse_template: PT) -> result::Result<String>
where
    PT: Fn(String) -> result::Result<String>,
    F: rops::file::format::FileFormat,
{
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let p = SETTINGS
            .sops
            .age_key_file
            .clone()
            .unwrap_or(dirs::CONFIG.join("age.txt"));
        let p = replace_path(match parse_template(p.to_string_lossy().to_string()) {
            Ok(p) => p,
            Err(e) => {
                warn!("failed to parse sops age key file: {}", e);
                return;
            }
        });
        if p.exists() {
            if let Ok(raw) = file::read_to_string(p) {
                let key = raw
                    .trim()
                    .lines()
                    .filter(|l| !l.starts_with('#'))
                    .collect::<String>();
                env::set_var("ROPS_AGE", key);
            }
        }
        if let Some(age_key) = &SETTINGS.sops.age_key {
            if !age_key.is_empty() {
                env::set_var("ROPS_AGE", age_key);
            }
        }
    });
    let f = input
        .parse::<RopsFile<EncryptedFile<AES256GCM, SHA512>, F>>()
        .wrap_err("failed to parse sops file")?;
    Ok(f.decrypt::<F>()
        .wrap_err("failed to decrypt sops file")?
        .to_string())
}
