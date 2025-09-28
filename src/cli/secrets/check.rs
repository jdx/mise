use eyre::Result;
use indexmap::IndexMap;

use crate::config::Config;
use crate::secrets::{SecretConfig, SecretManager, SecretProviderType};

#[derive(Debug, clap::Args)]
#[clap(about = "Check that all secrets can be resolved")]
pub struct Check {
    /// Only check secrets from specific provider
    #[clap(long)]
    provider: Option<SecretProviderType>,
}

impl Check {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let secret_manager = SecretManager::new()?;

        // Collect all secrets from env configs
        let mut all_secrets = IndexMap::new();

        // Find all env vars that are secret-backed
        for (source, cf) in &config.config_files {
            if let Ok(entries) = cf.env_entries() {
                for directive in entries {
                    use crate::config::env_directive::EnvDirective;
                    if let EnvDirective::Secret(key, secret_config, _) = directive {
                        if let Some(provider) = &self.provider {
                            // Filter by provider if specified
                            if secret_config.provider.as_ref() == Some(provider) {
                                all_secrets.insert(key.clone(), secret_config);
                            }
                        } else {
                            all_secrets.insert(key.clone(), secret_config);
                        }
                    }
                }
            }
        }

        if all_secrets.is_empty() {
            println!("No secrets configured");
            return Ok(());
        }

        println!("Checking {} secret(s)...", all_secrets.len());

        let missing = secret_manager.check(&all_secrets).await?;

        if missing.is_empty() {
            println!("✓ All secrets are accessible");
            Ok(())
        } else {
            eprintln!("\n✗ {} secret(s) failed validation:", missing.len());
            for m in &missing {
                eprintln!("  - {}: {}", m.key, m.error);
            }
            Err(eyre::eyre!(
                "{} secrets could not be resolved",
                missing.len()
            ))
        }
    }
}
