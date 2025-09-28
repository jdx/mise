use async_trait::async_trait;
use eyre::Result;

use super::{MissingSecret, SecretKey};

pub mod env;
pub mod keyring;
pub mod onepassword;

#[async_trait]
pub trait SecretProvider: Send + Sync + std::fmt::Debug {
    async fn get(&self, key: &SecretKey) -> Result<Option<String>>;

    async fn check(&self, keys: &[SecretKey]) -> Result<Vec<MissingSecret>> {
        let mut missing = Vec::new();
        for key in keys {
            if self.get(key).await?.is_none() {
                missing.push(MissingSecret {
                    key: key.clone(),
                    error: format!("Secret not found"),
                });
            }
        }
        Ok(missing)
    }

    fn name(&self) -> &str;
}
