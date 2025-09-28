use async_trait::async_trait;
use eyre::Result;

use super::{super::SecretKey, SecretProvider};

#[derive(Debug)]
pub struct EnvProvider {}

impl EnvProvider {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl SecretProvider for EnvProvider {
    async fn get(&self, key: &SecretKey) -> Result<Option<String>> {
        Ok(std::env::var(&key.key).ok())
    }

    fn name(&self) -> &str {
        "env"
    }
}
