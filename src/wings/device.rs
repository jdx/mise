//! Local device signing key for mise-wings dev auth.
//!
//! This is the client-side half of the device-bound auth protocol:
//! the server stores the public key during browser-approved device
//! enrollment, then requires a fresh challenge signed by this key for
//! refresh-token rotation.

use std::path::PathBuf;

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::env;

const DEVICE_FILENAME: &str = "device.json";
const DEVICE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKey {
    version: u32,
    host: String,
    key_kind: String,
    hardware_backed: bool,
    seed: String,
}

impl DeviceKey {
    fn path() -> PathBuf {
        env::MISE_STATE_DIR.join("wings").join(DEVICE_FILENAME)
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = crate::file::read_to_string(&path)?;
        let key: Self = serde_json::from_str(&raw).wrap_err("decoding wings device key")?;
        if key.version != DEVICE_SCHEMA_VERSION {
            bail!(
                "wings device key schema mismatch (got v{}, expected v{}); run `mise wings login`",
                key.version,
                DEVICE_SCHEMA_VERSION,
            );
        }
        Ok(Some(key))
    }

    pub fn load_for_current_host() -> Result<Option<Self>> {
        let Some(key) = Self::load()? else {
            return Ok(None);
        };
        if key.host == crate::wings::host() {
            Ok(Some(key))
        } else {
            Ok(None)
        }
    }

    pub fn load_or_generate() -> Result<Self> {
        if let Some(key) = Self::load_for_current_host()? {
            return Ok(key);
        }
        let seed: [u8; 32] = rand::random();
        let key = Self {
            version: DEVICE_SCHEMA_VERSION,
            host: crate::wings::host().to_string(),
            key_kind: "ed25519-software".into(),
            hardware_backed: false,
            seed: STANDARD.encode(seed),
        };
        key.save()?;
        Ok(key)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            crate::file::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            f.write_all(json.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            crate::file::write(&path, json)?;
        }
        Ok(())
    }

    pub fn delete() -> Result<()> {
        let path = Self::path();
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub fn key_kind(&self) -> &str {
        &self.key_kind
    }

    pub fn hardware_backed(&self) -> bool {
        self.hardware_backed
    }

    pub fn public_key_base64(&self) -> Result<String> {
        Ok(STANDARD.encode(self.verifying_key()?.as_bytes()))
    }

    pub fn sign_challenge(&self, device_id: &str, challenge: &str) -> Result<String> {
        let signature = self
            .signing_key()?
            .sign(canonical_message(device_id, challenge).as_bytes());
        Ok(STANDARD.encode(signature.to_bytes()))
    }

    fn signing_key(&self) -> Result<SigningKey> {
        let seed = STANDARD
            .decode(&self.seed)
            .wrap_err("decoding wings device key seed")?;
        let seed: [u8; 32] = seed
            .try_into()
            .map_err(|_| eyre::eyre!("wings device key seed must be 32 bytes"))?;
        Ok(SigningKey::from_bytes(&seed))
    }

    fn verifying_key(&self) -> Result<VerifyingKey> {
        Ok(self.signing_key()?.verifying_key())
    }
}

pub fn canonical_message(device_id: &str, challenge: &str) -> String {
    format!("mise-wings-device-auth:v1\n{device_id}\n{challenge}")
}
