use async_trait::async_trait;
use eyre::Result;
use keyring::Entry;

use super::{super::SecretKey, SecretProvider};

#[derive(Debug)]
pub struct KeyringProvider {
    default_service: String,
}

impl KeyringProvider {
    pub fn new() -> Result<Self> {
        // Try to determine project ID from config or cwd
        let default_service = if let Ok(project_dir) = std::env::current_dir() {
            if let Some(name) = project_dir.file_name() {
                format!("mise:{}", name.to_string_lossy())
            } else {
                "mise:default".to_string()
            }
        } else {
            "mise:default".to_string()
        };

        Ok(Self { default_service })
    }

    fn parse_key(&self, key: &str) -> (String, String) {
        // Support "service/account" format in key
        if let Some((service, account)) = key.split_once('/') {
            (service.to_string(), account.to_string())
        } else {
            // Use default service and key as account
            (self.default_service.clone(), key.to_string())
        }
    }
}

#[async_trait]
impl SecretProvider for KeyringProvider {
    async fn get(&self, key: &SecretKey) -> Result<Option<String>> {
        let (service, account) = self.parse_key(&key.key);

        match Entry::new(&service, &account) {
            Ok(entry) => match entry.get_password() {
                Ok(password) => Ok(Some(password)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(eyre::eyre!("Keyring error: {}", e)),
            },
            Err(e) => Err(eyre::eyre!("Failed to access keyring: {}", e)),
        }
    }

    fn name(&self) -> &str {
        "keyring"
    }
}
