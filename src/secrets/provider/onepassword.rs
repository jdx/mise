use async_trait::async_trait;
use eyre::Result;
use regex::Regex;
use std::process::Command;

use super::{super::SecretKey, SecretProvider};

#[derive(Debug)]
pub struct OnePasswordProvider {
    op_path: String,
}

impl OnePasswordProvider {
    pub fn new() -> Result<Self> {
        // Check if op CLI is available
        let op_path = which::which("op")
            .map_err(|_| {
                eyre::eyre!("1Password CLI (op) not found. Install with: mise use -g 1password")
            })?
            .to_string_lossy()
            .to_string();

        // Verify it's version 2.x
        let output = Command::new(&op_path)
            .arg("--version")
            .output()
            .map_err(|e| eyre::eyre!("Failed to run op --version: {}", e))?;

        let version = String::from_utf8_lossy(&output.stdout);
        if !version.starts_with('2') {
            return Err(eyre::eyre!(
                "1Password CLI v2 required, found: {}",
                version.trim()
            ));
        }

        Ok(Self { op_path })
    }

    fn parse_op_reference(&self, key: &str) -> Option<(String, String, String)> {
        // Parse op://<vault>/<item>/<field>
        let re = Regex::new(r"^op://([^/]+)/([^/]+)/([^/]+)$").ok()?;
        let captures = re.captures(key)?;

        Some((
            captures.get(1)?.as_str().to_string(),
            captures.get(2)?.as_str().to_string(),
            captures.get(3)?.as_str().to_string(),
        ))
    }
}

#[async_trait]
impl SecretProvider for OnePasswordProvider {
    async fn get(&self, key: &SecretKey) -> Result<Option<String>> {
        // Check if we have auth
        // In CI, expect OP_SERVICE_ACCOUNT_TOKEN
        // Locally, expect desktop app or CLI session

        let mut cmd = Command::new(&self.op_path);
        cmd.arg("read");

        if let Some((vault, item, field)) = self.parse_op_reference(&key.key) {
            // Use structured format
            cmd.arg(format!("op://{}/{}/{}", vault, item, field));
        } else {
            // Assume it's a direct reference
            cmd.arg(&key.key);
        }

        let output = tokio::task::spawn_blocking(move || cmd.output()).await??;

        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(Some(value))
        } else if output.status.code() == Some(1) {
            // Item not found
            Ok(None)
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            if error.contains("not signed in") || error.contains("authentication required") {
                Err(eyre::eyre!(
                    "1Password authentication required. Sign in with 'op signin' or use OP_SERVICE_ACCOUNT_TOKEN"
                ))
            } else {
                Err(eyre::eyre!("1Password error: {}", error))
            }
        }
    }

    fn name(&self) -> &str {
        "1password"
    }
}
