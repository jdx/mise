use clap::Args;
use eyre::{Result, WrapErr, eyre};
use std::path::PathBuf;

use crate::config::Settings;
use crate::dirs;
use crate::file;

/// Print the public recipient(s) for a given key file or the default key
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct KeysShow {
    /// Key file to show the public recipient for
    #[clap(long, value_hint = clap::ValueHint::FilePath)]
    key_file: Option<PathBuf>,
}

impl KeysShow {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age keys show")?;

        let key_path = self
            .key_file
            .or_else(|| {
                Settings::get()
                    .age
                    .key_file
                    .clone()
                    .map(crate::file::replace_path)
            })
            .unwrap_or_else(|| dirs::CONFIG.join("age.txt"));

        if !key_path.exists() {
            return Err(eyre!("Key file not found at {}", key_path.display()));
        }

        let content = file::read_to_string(&key_path)
            .wrap_err_with(|| format!("Failed to read key file at {}", key_path.display()))?;

        let mut found_keys = false;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("AGE-SECRET-KEY-") {
                if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                    println!("{}", identity.to_public());
                    found_keys = true;
                }
            }
        }

        if !found_keys {
            return Err(eyre!("No age identities found in {}", key_path.display()));
        }

        Ok(())
    }
}
