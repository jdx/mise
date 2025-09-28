use clap::Args;
use eyre::{Result, WrapErr, eyre};
use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::dirs;
use crate::file;

/// Convert SSH public keys into age recipients and persist to config
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct ImportSsh {
    /// SSH public key files to import
    #[clap(required = true, value_hint = clap::ValueHint::FilePath)]
    ssh_keys: Vec<PathBuf>,

    /// Add to global config instead of local
    #[clap(long, short = 'g')]
    global: bool,

    /// Add to local config (.mise.toml in current directory)
    #[clap(long, short = 'l', conflicts_with = "global")]
    local: bool,
}

impl ImportSsh {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age import-ssh")?;

        let mut recipients = Vec::new();

        for key_path in &self.ssh_keys {
            let recipient = self.import_ssh_key(key_path).await?;
            recipients.push(recipient);
        }

        if recipients.is_empty() {
            return Err(eyre!("No valid SSH recipients found"));
        }

        // Determine config file to update
        let config_path = if self.global {
            dirs::CONFIG.join("config.toml")
        } else if self.local {
            PathBuf::from(".mise.toml")
        } else {
            // Default to global settings
            dirs::CONFIG.join("settings.toml")
        };

        // Update config file
        self.update_config(&config_path, &recipients).await?;

        for recipient in &recipients {
            eprintln!("Imported SSH recipient: {}", recipient);
        }
        eprintln!("Updated config: {}", config_path.display());

        Ok(())
    }

    async fn import_ssh_key(&self, path: &Path) -> Result<String> {
        let content = file::read_to_string(path)
            .wrap_err_with(|| format!("Failed to read SSH key from {}", path.display()))?;

        let trimmed = content.trim();

        // Validate it's a proper SSH public key
        if !trimmed.starts_with("ssh-") {
            return Err(eyre!(
                "File {} does not contain a valid SSH public key",
                path.display()
            ));
        }

        // Verify it can be parsed as an age SSH recipient
        match trimmed.parse::<age::ssh::Recipient>() {
            Ok(_) => Ok(trimmed.to_string()),
            Err(e) => Err(eyre!(
                "Invalid SSH public key in {}: {:?}",
                path.display(),
                e
            )),
        }
    }

    async fn update_config(&self, path: &Path, recipients: &[String]) -> Result<()> {
        use toml_edit::{Array, DocumentMut, Value};

        let content = if path.exists() {
            file::read_to_string(path)?
        } else {
            String::new()
        };

        let mut doc = content
            .parse::<DocumentMut>()
            .wrap_err_with(|| format!("Failed to parse config at {}", path.display()))?;

        // Ensure [age] section exists
        if !doc.contains_key("age") {
            doc["age"] = toml_edit::table();
        }

        // Get or create ssh_recipients array
        let age_table = doc["age"]
            .as_table_mut()
            .ok_or_else(|| eyre!("Failed to access [age] section"))?;

        if !age_table.contains_key("ssh_recipients") {
            age_table["ssh_recipients"] = toml_edit::Item::Value(Value::Array(Array::new()));
        }

        let ssh_recipients = age_table["ssh_recipients"]
            .as_array_mut()
            .ok_or_else(|| eyre!("Failed to access age.ssh_recipients array"))?;

        // Add new recipients (avoiding duplicates)
        for recipient in recipients {
            let already_exists = ssh_recipients.iter().any(|v| v.as_str() == Some(recipient));

            if !already_exists {
                ssh_recipients.push(recipient.as_str());
            }
        }

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            file::create_dir_all(parent)?;
        }

        // Write back the updated config
        file::write(path, doc.to_string())
            .wrap_err_with(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }
}
