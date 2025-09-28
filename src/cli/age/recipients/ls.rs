use clap::Args;
use eyre::Result;
use serde_json::json;

use crate::config::Settings;
use crate::file;

/// Show effective recipients resolved from CLI, settings, and defaults
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct RecipientsLs {
    /// Output in JSON format
    #[clap(long)]
    json: bool,

    /// Expand SSH recipients to show full public keys
    #[clap(long)]
    expand_ssh: bool,
}

impl RecipientsLs {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age recipients ls")?;

        let recipients = self.collect_recipients().await?;

        if self.json {
            let output = json!(recipients);
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            if recipients.is_empty() {
                eprintln!("No age recipients configured");
            } else {
                for recipient in recipients {
                    match recipient.recipient_type.as_str() {
                        "age" => println!("age: {}", recipient.value),
                        "ssh" => {
                            if self.expand_ssh {
                                println!("ssh: {}", recipient.value);
                            } else {
                                // Truncate SSH keys for display
                                let truncated = if recipient.value.len() > 50 {
                                    format!("{}...", &recipient.value[..50])
                                } else {
                                    recipient.value.clone()
                                };
                                println!("ssh: {}", truncated);
                            }
                        }
                        "ssh-file" => println!("ssh-file: {}", recipient.value),
                        _ => println!("{}: {}", recipient.recipient_type, recipient.value),
                    }
                }
            }
        }

        Ok(())
    }

    async fn collect_recipients(&self) -> Result<Vec<RecipientInfo>> {
        let mut recipients = Vec::new();

        // Note: Recipients are not stored in settings currently
        // They are derived from identity files or specified at encryption time

        // Load from settings.age.ssh_identity_files (public keys)
        if let Some(ssh_files) = &Settings::get().age.ssh_identity_files {
            for file in ssh_files {
                let path = crate::file::replace_path(file.clone());
                let pub_path = path.with_extension("pub");
                if pub_path.exists() {
                    if let Ok(content) = file::read_to_string(&pub_path) {
                        let trimmed = content.trim();
                        if trimmed.starts_with("ssh-") {
                            recipients.push(RecipientInfo {
                                source: format!("ssh_identity_file:{}", path.display()),
                                recipient_type: "ssh-file".to_string(),
                                value: if self.expand_ssh {
                                    trimmed.to_string()
                                } else {
                                    path.display().to_string()
                                },
                            });
                        }
                    }
                }
            }
        }

        // Try to load default recipients from age key file
        let default_age_paths = vec![
            Settings::get()
                .age
                .key_file
                .clone()
                .map(crate::file::replace_path),
            Some(crate::dirs::CONFIG.join("age.txt")),
        ];

        for path_opt in default_age_paths {
            if let Some(path) = path_opt {
                if path.exists() {
                    if let Ok(content) = file::read_to_string(&path) {
                        for line in content.lines() {
                            let line = line.trim();
                            if line.starts_with("AGE-SECRET-KEY-") {
                                if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                                    recipients.push(RecipientInfo {
                                        source: format!("key_file:{}", path.display()),
                                        recipient_type: "age".to_string(),
                                        value: identity.to_public().to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(recipients)
    }
}

#[derive(Debug, serde::Serialize)]
struct RecipientInfo {
    source: String,
    #[serde(rename = "type")]
    recipient_type: String,
    value: String,
}
