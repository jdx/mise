use clap::Args;
use eyre::{Result, WrapErr, eyre};
use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::dirs;
use crate::file;

/// Add recipients to a config-managed set
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct RecipientsAdd {
    /// Age recipients to add (age1...)
    #[clap(long = "age")]
    age_recipients: Vec<String>,

    /// SSH recipients to add (file path or public key)
    #[clap(long = "ssh")]
    ssh_recipients: Vec<String>,

    /// Add to global config instead of settings
    #[clap(long, short = 'g')]
    global: bool,

    /// Add to local config (.mise.toml in current directory)
    #[clap(long, short = 'l', conflicts_with = "global")]
    local: bool,
}

impl RecipientsAdd {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age recipients add")?;

        if self.age_recipients.is_empty() && self.ssh_recipients.is_empty() {
            return Err(eyre!("No recipients specified. Use --age or --ssh"));
        }

        // Validate age recipients
        for recipient in &self.age_recipients {
            if !recipient.starts_with("age1") {
                return Err(eyre!("Invalid age recipient: {}", recipient));
            }
            // Validate it can be parsed
            recipient
                .parse::<age::x25519::Recipient>()
                .map_err(|e| eyre!("Invalid age recipient {}: {}", recipient, e))?;
        }

        // Process SSH recipients
        let mut processed_ssh = Vec::new();
        for recipient in &self.ssh_recipients {
            let processed = self.process_ssh_recipient(recipient).await?;
            processed_ssh.push(processed);
        }

        // Determine config file to update
        let config_path = if self.global {
            dirs::CONFIG.join("config.toml")
        } else if self.local {
            PathBuf::from(".mise.toml")
        } else {
            // Default to settings
            dirs::CONFIG.join("settings.toml")
        };

        // Update config file
        self.update_config(&config_path, &self.age_recipients, &processed_ssh)
            .await?;

        eprintln!("Added {} age recipient(s)", self.age_recipients.len());
        eprintln!("Added {} ssh recipient(s)", processed_ssh.len());
        eprintln!("Updated config: {}", config_path.display());

        Ok(())
    }

    async fn process_ssh_recipient(&self, recipient: &str) -> Result<String> {
        // Check if it's already a public key
        if recipient.starts_with("ssh-") {
            // Validate it
            recipient
                .parse::<age::ssh::Recipient>()
                .map_err(|e| eyre!("Invalid SSH public key: {:?}", e))?;
            return Ok(recipient.to_string());
        }

        // Try as file path
        let path = PathBuf::from(recipient);
        if path.exists() {
            let content = file::read_to_string(&path)
                .wrap_err_with(|| format!("Failed to read SSH key from {}", path.display()))?;

            let trimmed = content.trim();
            if trimmed.starts_with("ssh-") {
                trimmed
                    .parse::<age::ssh::Recipient>()
                    .map_err(|e| eyre!("Invalid SSH public key in {}: {:?}", path.display(), e))?;
                return Ok(trimmed.to_string());
            }

            // Try .pub file if it's a private key
            let pub_path = path.with_extension("pub");
            if pub_path.exists() {
                let content = file::read_to_string(&pub_path)?;
                let trimmed = content.trim();
                if trimmed.starts_with("ssh-") {
                    trimmed.parse::<age::ssh::Recipient>().map_err(|e| {
                        eyre!("Invalid SSH public key in {}: {:?}", pub_path.display(), e)
                    })?;
                    return Ok(trimmed.to_string());
                }
            }
        }

        Err(eyre!(
            "Invalid SSH recipient: {} (not a valid public key or file path)",
            recipient
        ))
    }

    async fn update_config(
        &self,
        path: &Path,
        age_recipients: &[String],
        ssh_recipients: &[String],
    ) -> Result<()> {
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

        let age_table = doc["age"]
            .as_table_mut()
            .ok_or_else(|| eyre!("Failed to access [age] section"))?;

        // Add age recipients
        if !age_recipients.is_empty() {
            if !age_table.contains_key("recipients") {
                age_table["recipients"] = toml_edit::Item::Value(Value::Array(Array::new()));
            }

            let recipients_array = age_table["recipients"]
                .as_array_mut()
                .ok_or_else(|| eyre!("Failed to access age.recipients array"))?;

            for recipient in age_recipients {
                let already_exists = recipients_array
                    .iter()
                    .any(|v| v.as_str() == Some(recipient));

                if !already_exists {
                    recipients_array.push(recipient.as_str());
                }
            }
        }

        // Add SSH recipients
        if !ssh_recipients.is_empty() {
            if !age_table.contains_key("ssh_recipients") {
                age_table["ssh_recipients"] = toml_edit::Item::Value(Value::Array(Array::new()));
            }

            let ssh_array = age_table["ssh_recipients"]
                .as_array_mut()
                .ok_or_else(|| eyre!("Failed to access age.ssh_recipients array"))?;

            for recipient in ssh_recipients {
                let already_exists = ssh_array.iter().any(|v| v.as_str() == Some(recipient));

                if !already_exists {
                    ssh_array.push(recipient.as_str());
                }
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
