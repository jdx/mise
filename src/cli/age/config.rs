use clap::Args;
use eyre::{Result, eyre};
use serde_json::json;

use crate::config::Settings;

/// Print effective age configuration merged from settings/env/CLI
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct Config {
    /// Output in JSON format
    #[clap(long)]
    json: bool,

    /// Show specific configuration path
    #[clap(long)]
    path: Option<String>,
}

impl Config {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age config")?;

        let settings = Settings::get();

        if let Some(ref path) = self.path {
            let value = self.get_config_value(&settings, path)?;
            println!("{}", value);
        } else if self.json {
            let config = json!({
                "key_file": settings.age.key_file.as_ref().map(|p| p.display().to_string()),
                "identity_files": settings.age.identity_files.as_ref().map(|files| {
                    files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
                }),
                "ssh_identity_files": settings.age.ssh_identity_files.as_ref().map(|files| {
                    files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
                }),
                "env": {
                    "MISE_AGE_KEY": std::env::var("MISE_AGE_KEY").ok(),
                },
            });
            println!("{}", serde_json::to_string_pretty(&config)?);
        } else {
            self.print_config(&settings);
        }

        Ok(())
    }

    fn print_config(&self, settings: &Settings) {
        println!("Age Configuration:");
        println!();

        if let Some(key_file) = &settings.age.key_file {
            println!("Key file: {}", key_file.display());
        }

        if let Some(identity_files) = &settings.age.identity_files {
            if !identity_files.is_empty() {
                println!("Identity files:");
                for file in identity_files {
                    println!("  - {}", file.display());
                }
            }
        }

        if let Some(ssh_files) = &settings.age.ssh_identity_files {
            if !ssh_files.is_empty() {
                println!("SSH identity files:");
                for file in ssh_files {
                    println!("  - {}", file.display());
                }
            }
        }

        // Note: recipients are not stored in settings currently
        // They are derived from identity files

        if let Ok(age_key) = std::env::var("MISE_AGE_KEY") {
            if !age_key.is_empty() {
                println!("MISE_AGE_KEY: (set)");
            }
        }
    }

    fn get_config_value(&self, settings: &Settings, path: &str) -> Result<String> {
        match path {
            "key_file" => Ok(settings
                .age
                .key_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()),
            "identity_files" => Ok(settings
                .age
                .identity_files
                .as_ref()
                .map(|files| {
                    files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()),
            "ssh_identity_files" => Ok(settings
                .age
                .ssh_identity_files
                .as_ref()
                .map(|files| {
                    files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()),
            "recipients" => Ok(String::new()), // Not currently stored in settings
            "ssh_recipients" => Ok(String::new()), // Not currently stored in settings
            _ => Err(eyre!("Unknown configuration path: {}", path)),
        }
    }
}
