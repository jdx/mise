use clap::Args;
use eyre::Result;
use serde_json::json;

use crate::config::Settings;
use crate::file;

/// List effective identities from settings
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct KeysLs {
    /// Output in JSON format
    #[clap(long)]
    json: bool,
}

impl KeysLs {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age keys ls")?;

        let identities = self.collect_identities().await?;

        if self.json {
            let output = json!(identities);
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            if identities.is_empty() {
                eprintln!("No age identities configured");
            } else {
                for identity in identities {
                    println!("Source: {}", identity.source);
                    println!("  Public: {}", identity.public_key);
                    if let Some(comment) = identity.comment {
                        println!("  Comment: {}", comment);
                    }
                    println!();
                }
            }
        }

        Ok(())
    }

    async fn collect_identities(&self) -> Result<Vec<IdentityInfo>> {
        let mut identities = Vec::new();

        // Check MISE_AGE_KEY environment variable
        if let Ok(age_key) = std::env::var("MISE_AGE_KEY") {
            if !age_key.is_empty() {
                for line in age_key.lines() {
                    let line = line.trim();
                    if line.starts_with("AGE-SECRET-KEY-") {
                        if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                            identities.push(IdentityInfo {
                                source: "env:MISE_AGE_KEY".to_string(),
                                public_key: identity.to_public().to_string(),
                                comment: None,
                            });
                        }
                    }
                }
            }
        }

        // Check settings.age.key_file
        if let Some(key_file) = &Settings::get().age.key_file {
            let path = crate::file::replace_path(key_file.clone());
            if path.exists() {
                self.load_identities_from_file(&path, &mut identities)?;
            }
        }

        // Check settings.age.identity_files
        if let Some(identity_files) = &Settings::get().age.identity_files {
            for file in identity_files {
                let path = crate::file::replace_path(file.clone());
                if path.exists() {
                    self.load_identities_from_file(&path, &mut identities)?;
                }
            }
        }

        // Check default age.txt
        let default_age = crate::dirs::CONFIG.join("age.txt");
        if default_age.exists() {
            self.load_identities_from_file(&default_age, &mut identities)?;
        }

        // Check SSH identities from settings.age.ssh_identity_files
        if let Some(ssh_files) = &Settings::get().age.ssh_identity_files {
            for file in ssh_files {
                let path = crate::file::replace_path(file.clone());
                if path.exists() {
                    // Try to get the public key for SSH identity
                    let pub_path = path.with_extension("pub");
                    if pub_path.exists() {
                        if let Ok(content) = file::read_to_string(&pub_path) {
                            let trimmed = content.trim();
                            if trimmed.starts_with("ssh-") {
                                identities.push(IdentityInfo {
                                    source: format!("ssh:{}", path.display()),
                                    public_key: trimmed.to_string(),
                                    comment: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(identities)
    }

    fn load_identities_from_file(
        &self,
        path: &std::path::Path,
        identities: &mut Vec<IdentityInfo>,
    ) -> Result<()> {
        let content = file::read_to_string(path)?;
        let mut comment = None;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("# public key:") {
                // Extract comment from age file
                comment = Some(
                    line.strip_prefix("# public key:")
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                );
            } else if line.starts_with("AGE-SECRET-KEY-") {
                if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                    identities.push(IdentityInfo {
                        source: format!("file:{}", path.display()),
                        public_key: identity.to_public().to_string(),
                        comment: comment.clone(),
                    });
                    comment = None;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, serde::Serialize)]
struct IdentityInfo {
    source: String,
    public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
}
