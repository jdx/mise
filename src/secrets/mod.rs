use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::sync::Arc;

use eyre::Result;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub mod provider;

use provider::{
    SecretProvider, env::EnvProvider, keyring::KeyringProvider, onepassword::OnePasswordProvider,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretProviderType {
    Keyring,
    #[serde(rename = "1password")]
    OnePassword,
    Env,
    // Future providers: sops, dotenv
}

impl Display for SecretProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyring => write!(f, "keyring"),
            Self::OnePassword => write!(f, "1password"),
            Self::Env => write!(f, "env"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<SecretProviderType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default = "default_required")]
    pub required: bool,
    #[serde(default = "default_redact")]
    pub redact: bool,
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl Default for SecretConfig {
    fn default() -> Self {
        Self {
            provider: None,
            key: None,
            default: None,
            required: true,
            redact: true,
            extra: HashMap::new(),
        }
    }
}

fn default_required() -> bool {
    true
}

fn default_redact() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct SecretKey {
    pub name: String,
    pub key: String,
    pub provider: SecretProviderType,
}

impl Display for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.provider, self.key)
    }
}

#[derive(Debug)]
pub struct MissingSecret {
    pub key: SecretKey,
    pub error: String,
}

pub struct SecretManager {
    providers: HashMap<SecretProviderType, Arc<dyn SecretProvider>>,
    cache: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
}

impl SecretManager {
    pub fn new() -> Result<Self> {
        let mut providers: HashMap<SecretProviderType, Arc<dyn SecretProvider>> = HashMap::new();

        // Always include env provider
        providers.insert(SecretProviderType::Env, Arc::new(EnvProvider::new()));

        // Try to initialize keyring provider
        match KeyringProvider::new() {
            Ok(provider) => {
                providers.insert(SecretProviderType::Keyring, Arc::new(provider));
            }
            Err(e) => {
                debug!("Keyring provider not available: {}", e);
            }
        }

        // Try to initialize 1password provider
        match OnePasswordProvider::new() {
            Ok(provider) => {
                providers.insert(SecretProviderType::OnePassword, Arc::new(provider));
            }
            Err(e) => {
                debug!("1Password provider not available: {}", e);
            }
        }

        Ok(Self {
            providers,
            cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        })
    }

    pub async fn get(
        &self,
        env_var_name: &str,
        config: &SecretConfig,
        default_provider: SecretProviderType,
    ) -> Result<Option<String>> {
        let provider_type = config.provider.as_ref().unwrap_or(&default_provider);
        let default_key = env_var_name.to_string();
        let key = config.key.as_ref().unwrap_or(&default_key);

        // Check cache first
        let cache_key = format!("{}:{}", provider_type, key);
        {
            let cache = self.cache.lock().await;
            if let Some(value) = cache.get(&cache_key) {
                return Ok(Some(value.clone()));
            }
        }

        // Get from provider
        let provider = self
            .providers
            .get(provider_type)
            .ok_or_else(|| eyre::eyre!("Provider {} not available", provider_type))?;

        let secret_key = SecretKey {
            name: env_var_name.to_string(),
            key: key.clone(),
            provider: provider_type.clone(),
        };

        match provider.get(&secret_key).await? {
            Some(value) => {
                // Cache the value
                let mut cache = self.cache.lock().await;
                cache.insert(cache_key, value.clone());
                Ok(Some(value))
            }
            None => {
                if let Some(default) = &config.default {
                    Ok(Some(default.clone()))
                } else if config.required {
                    Err(eyre::eyre!("Required secret {} not found", key))
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub async fn check(
        &self,
        secrets: &IndexMap<String, SecretConfig>,
    ) -> Result<Vec<MissingSecret>> {
        let mut missing = Vec::new();

        for (env_var, config) in secrets {
            let default_provider = self.default_provider();
            let provider_type = config.provider.as_ref().unwrap_or(&default_provider);
            let key = config.key.as_ref().unwrap_or(env_var);

            let provider = match self.providers.get(provider_type) {
                Some(p) => p,
                None => {
                    missing.push(MissingSecret {
                        key: SecretKey {
                            name: env_var.clone(),
                            key: key.clone(),
                            provider: provider_type.clone(),
                        },
                        error: format!("Provider {} not available", provider_type),
                    });
                    continue;
                }
            };

            let secret_key = SecretKey {
                name: env_var.clone(),
                key: key.clone(),
                provider: provider_type.clone(),
            };

            if let Err(e) = provider.get(&secret_key).await {
                if config.required && config.default.is_none() {
                    missing.push(MissingSecret {
                        key: secret_key,
                        error: e.to_string(),
                    });
                }
            }
        }

        Ok(missing)
    }

    fn default_provider(&self) -> SecretProviderType {
        // Check environment variable
        if let Ok(provider) = std::env::var("MISE_SECRETS_PROVIDER") {
            if let Ok(p) = provider.parse() {
                return p;
            }
        }

        // Default to keyring in local dev, env in CI
        if std::env::var("CI").is_ok() {
            SecretProviderType::Env
        } else {
            SecretProviderType::Keyring
        }
    }
}

impl std::str::FromStr for SecretProviderType {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "keyring" => Ok(Self::Keyring),
            "1password" | "onepassword" => Ok(Self::OnePassword),
            "env" => Ok(Self::Env),
            _ => Err(eyre::eyre!("Unknown secret provider: {}", s)),
        }
    }
}
