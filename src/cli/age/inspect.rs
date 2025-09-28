use clap::Args;
use eyre::{Result, WrapErr, eyre};
use serde_json::json;
use std::path::PathBuf;

use crate::config::Settings;
use crate::file;

/// Show metadata of ciphertext: recipient types, armor, file count
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct Inspect {
    /// File to inspect
    #[clap(value_hint = clap::ValueHint::FilePath)]
    file: PathBuf,

    /// Output in JSON format
    #[clap(long)]
    json: bool,

    /// Show detailed information
    #[clap(long)]
    detailed: bool,
}

impl Inspect {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age inspect")?;

        if !self.file.exists() {
            return Err(eyre!("File not found: {}", self.file.display()));
        }

        let content = file::read(&self.file)
            .wrap_err_with(|| format!("Failed to read {}", self.file.display()))?;

        let metadata = self.analyze_ciphertext(&content)?;

        if self.json {
            let output = json!(metadata);
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            self.print_metadata(&metadata);
        }

        Ok(())
    }

    fn analyze_ciphertext(&self, content: &[u8]) -> Result<CiphertextMetadata> {
        // Check if it's ASCII armor or binary
        let is_armor = content.iter().take(100).all(|&b| b.is_ascii());

        let mut metadata = CiphertextMetadata {
            file: self.file.display().to_string(),
            size: content.len(),
            is_armor,
            recipients: Vec::new(),
            format: None,
        };

        // Try to parse as age format
        if is_armor {
            let text = String::from_utf8_lossy(content);
            if text.starts_with("-----BEGIN AGE ENCRYPTED FILE-----") {
                metadata.format = Some("age-armor".to_string());

                // Parse recipients from armor header
                for line in text.lines().skip(1) {
                    if line.starts_with("---") {
                        break;
                    }
                    if line.starts_with("-> X25519 ") {
                        metadata.recipients.push(RecipientInfo {
                            recipient_type: "X25519".to_string(),
                            value: line.strip_prefix("-> X25519 ").unwrap_or("").to_string(),
                        });
                    } else if line.starts_with("-> ssh-") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            metadata.recipients.push(RecipientInfo {
                                recipient_type: "SSH".to_string(),
                                value: parts[1].to_string(),
                            });
                        }
                    } else if line.starts_with("-> scrypt ") {
                        metadata.recipients.push(RecipientInfo {
                            recipient_type: "scrypt".to_string(),
                            value: "(passphrase)".to_string(),
                        });
                    }
                }
            } else if text.starts_with("age64:") {
                metadata.format = Some("mise-age-encoded".to_string());
                // This is a mise-specific format
                if text.starts_with("age64:zstd:v1:") {
                    metadata.format = Some("mise-age-compressed".to_string());
                } else if text.starts_with("age64:v1:") {
                    metadata.format = Some("mise-age-uncompressed".to_string());
                }
            }
        } else {
            // Try to parse binary age format
            use age::Decryptor;

            match Decryptor::new(&content[..]) {
                Ok(_decryptor) => {
                    metadata.format = Some("age-binary".to_string());

                    // Note: The age crate doesn't expose recipient metadata directly
                    // in the current API version
                    metadata.recipients.push(RecipientInfo {
                        recipient_type: "unknown".to_string(),
                        value: "(encrypted)".to_string(),
                    });
                }
                Err(_) => {
                    metadata.format = Some("unknown".to_string());
                }
            }
        }

        Ok(metadata)
    }

    fn print_metadata(&self, metadata: &CiphertextMetadata) {
        println!("File: {}", metadata.file);
        println!("Size: {} bytes", metadata.size);
        println!(
            "Format: {}",
            metadata.format.as_ref().unwrap_or(&"unknown".to_string())
        );

        if metadata.is_armor {
            println!("Armor: yes");
        } else {
            println!("Armor: no");
        }

        if !metadata.recipients.is_empty() {
            println!("Recipients:");
            for recipient in &metadata.recipients {
                if self.detailed {
                    println!("  - {}: {}", recipient.recipient_type, recipient.value);
                } else {
                    let display = if recipient.value.len() > 50 {
                        format!("{}...", &recipient.value[..50.min(recipient.value.len())])
                    } else {
                        recipient.value.clone()
                    };
                    println!("  - {}: {}", recipient.recipient_type, display);
                }
            }
        } else {
            println!("Recipients: unable to determine");
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct CiphertextMetadata {
    file: String,
    size: usize,
    is_armor: bool,
    format: Option<String>,
    recipients: Vec<RecipientInfo>,
}

#[derive(Debug, serde::Serialize)]
struct RecipientInfo {
    #[serde(rename = "type")]
    recipient_type: String,
    value: String,
}
