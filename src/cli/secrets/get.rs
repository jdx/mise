use eyre::Result;

use crate::config::Config;
use crate::secrets::{SecretConfig, SecretKey, SecretManager, SecretProviderType};

#[derive(Debug, clap::Args)]
#[clap(about = "Get a secret value")]
pub struct Get {
    /// Name of the secret (env var name or key)
    name: String,

    /// Secret provider to use
    #[clap(long)]
    provider: Option<SecretProviderType>,

    /// Show the actual value (not redacted)
    #[clap(long, short = 's')]
    show: bool,
}

impl Get {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let secret_manager = SecretManager::new()?;

        // First check if it's defined in config
        let mut secret_config = None;
        for (_source, cf) in &config.config_files {
            if let Ok(entries) = cf.env_entries() {
                for directive in entries {
                    use crate::config::env_directive::EnvDirective;
                    if let EnvDirective::Secret(key, config, _) = directive {
                        if key == self.name {
                            secret_config = Some(config);
                            break;
                        }
                    }
                }
            }
            if secret_config.is_some() {
                break;
            }
        }

        // If not in config, create a default config
        let secret_config = secret_config.unwrap_or_else(|| SecretConfig {
            provider: self.provider.clone(),
            key: Some(self.name.clone()),
            default: None,
            required: true,
            redact: !self.show,
            extra: Default::default(),
        });

        // Get default provider - same logic as in Config
        let default_provider = if let Ok(provider) = std::env::var("MISE_SECRETS_PROVIDER") {
            provider.parse().unwrap_or_else(|_| {
                if std::env::var("CI").is_ok() {
                    SecretProviderType::Env
                } else {
                    SecretProviderType::Keyring
                }
            })
        } else if std::env::var("CI").is_ok() {
            SecretProviderType::Env
        } else {
            SecretProviderType::Keyring
        };
        let provider_type = secret_config
            .provider
            .as_ref()
            .or(self.provider.as_ref())
            .unwrap_or(&default_provider);

        match secret_manager
            .get(&self.name, &secret_config, provider_type.clone())
            .await?
        {
            Some(value) => {
                if self.show {
                    println!("{}", value);
                } else {
                    println!("{} = <redacted>", self.name);
                    eprintln!("Use --show to display the actual value");
                }
                Ok(())
            }
            None => Err(eyre::eyre!(
                "Secret '{}' not found in provider {}",
                self.name,
                provider_type
            )),
        }
    }
}
