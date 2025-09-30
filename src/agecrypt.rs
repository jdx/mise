use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use age::ssh;
use age::{Decryptor, Encryptor, Identity, IdentityFile, Recipient};
use base64::Engine;
use eyre::{Result, WrapErr, eyre};
use indexmap::IndexSet;

use crate::config::Settings;
use crate::config::env_directive::{AgeFormat, EnvDirective, EnvDirectiveOptions};
use crate::file::{self, replace_path};
use crate::{dirs, env};

const ZSTD_COMPRESSION_LEVEL: i32 = 3;
const COMPRESSION_THRESHOLD: usize = 1024; // 1KB

pub async fn create_age_directive(
    key: String,
    value: &str,
    recipients: &[Box<dyn Recipient + Send>],
) -> Result<EnvDirective> {
    if recipients.is_empty() {
        return Err(eyre!(
            "[experimental] No age recipients provided for encryption"
        ));
    }

    let encryptor =
        match Encryptor::with_recipients(recipients.iter().map(|r| r.as_ref() as &dyn Recipient)) {
            Ok(encryptor) => encryptor,
            Err(e) => return Err(eyre!("[experimental] Failed to create encryptor: {}", e)),
        };

    let mut encrypted = Vec::new();
    let mut writer = encryptor.wrap_output(&mut encrypted)?;
    writer.write_all(value.as_bytes())?;
    writer.finish()?;

    // Determine format based on size and compression
    let (encoded, format) = if encrypted.len() > COMPRESSION_THRESHOLD {
        let compressed = zstd::encode_all(&encrypted[..], ZSTD_COMPRESSION_LEVEL)?;
        let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(&compressed);
        (encoded, Some(AgeFormat::Zstd))
    } else {
        let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(&encrypted);
        (encoded, None) // Use None for raw format (default)
    };

    Ok(EnvDirective::Age {
        key,
        value: encoded,
        format,
        options: EnvDirectiveOptions::default(),
    })
}

pub async fn decrypt_age_directive(directive: &EnvDirective) -> Result<String> {
    Settings::get().ensure_experimental("age encryption")?;

    match directive {
        EnvDirective::Age { value, format, .. } => {
            let decoded = base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(value)
                .wrap_err("[experimental] Failed to decode base64")?;

            let ciphertext = match format {
                Some(AgeFormat::Zstd) => zstd::decode_all(&decoded[..])
                    .wrap_err("[experimental] Failed to decompress zstd")?,
                Some(AgeFormat::Raw) | None => decoded,
            };

            let identities = load_all_identities().await?;
            if identities.is_empty() {
                return Err(eyre!(
                    "[experimental] No age identities found for decryption"
                ));
            }

            let decryptor = Decryptor::new(&ciphertext[..])?;
            let mut decrypted = Vec::new();

            let identity_refs: Vec<&dyn Identity> = identities
                .iter()
                .map(|i| i.as_ref() as &dyn Identity)
                .collect();

            match decryptor.decrypt(identity_refs.into_iter()) {
                Ok(mut reader) => {
                    reader.read_to_end(&mut decrypted)?;
                }
                Err(e) => {
                    return Err(eyre!("[experimental] Failed to decrypt: {}", e));
                }
            }

            String::from_utf8(decrypted)
                .wrap_err("[experimental] Decrypted value is not valid UTF-8")
        }
        _ => Err(eyre!("[experimental] Not an Age directive")),
    }
}

pub async fn load_recipients_from_defaults() -> Result<Vec<Box<dyn Recipient + Send>>> {
    let mut recipients: IndexSet<String> = IndexSet::new();

    // Try to load from age key file
    if let Some(key_file) = get_default_key_file().await {
        if key_file.exists() {
            let content = file::read_to_string(&key_file)?;
            // For age keys, we need to parse them as x25519 identities to get public keys
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("AGE-SECRET-KEY-") {
                    if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                        recipients.insert(identity.to_public().to_string());
                    }
                }
            }
        }
    }

    // Try to load from SSH private keys
    let ssh_key_paths = get_default_ssh_key_paths();
    for path in ssh_key_paths {
        if path.exists() {
            if let Ok(recipient) = load_ssh_recipient_from_private_key(&path).await {
                recipients.insert(recipient);
            }
        }
    }

    let mut parsed_recipients: Vec<Box<dyn Recipient + Send>> = Vec::new();
    for recipient_str in recipients {
        if let Some(recipient) = parse_recipient(&recipient_str)? {
            parsed_recipients.push(recipient);
        }
    }

    if parsed_recipients.is_empty() {
        return Err(eyre!(
            "[experimental] No age recipients found. Provide --age-recipient, --age-ssh-recipient, or configure settings.age.key_file"
        ));
    }

    Ok(parsed_recipients)
}

pub async fn load_recipients_from_key_file(path: &Path) -> Result<Vec<Box<dyn Recipient + Send>>> {
    let mut recipients: Vec<Box<dyn Recipient + Send>> = Vec::new();

    if !path.exists() {
        return Err(eyre!(
            "[experimental] Age key file not found: {}",
            path.display()
        ));
    }

    let content = file::read_to_string(path)?;

    // Parse age x25519 identities and convert to recipients
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("AGE-SECRET-KEY-") {
            if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                let public_key = identity.to_public();
                recipients.push(Box::new(public_key));
            }
        }
    }

    if recipients.is_empty() {
        return Err(eyre!(
            "[experimental] No valid age identities found in {}",
            path.display()
        ));
    }

    Ok(recipients)
}

pub fn parse_recipient(recipient_str: &str) -> Result<Option<Box<dyn Recipient + Send>>> {
    let trimmed = recipient_str.trim();

    if trimmed.starts_with("age1") {
        match trimmed.parse::<age::x25519::Recipient>() {
            Ok(r) => Ok(Some(Box::new(r))),
            Err(e) => Err(eyre!("[experimental] Invalid age recipient: {}", e)),
        }
    } else if trimmed.starts_with("ssh-") {
        // SSH recipient parsing - the age crate will validate it
        match trimmed.parse::<ssh::Recipient>() {
            Ok(r) => Ok(Some(Box::new(r))),
            Err(e) => Err(eyre!("[experimental] Invalid SSH recipient: {:?}", e)),
        }
    } else {
        Ok(None)
    }
}

pub async fn load_ssh_recipient_from_path(path: &Path) -> Result<Box<dyn Recipient + Send>> {
    let content = file::read_to_string(path)?;
    let trimmed = content.trim();

    // Check if it's a public key
    if trimmed.starts_with("ssh-") {
        match trimmed.parse::<ssh::Recipient>() {
            Ok(r) => return Ok(Box::new(r)),
            Err(e) => {
                return Err(eyre!(
                    "[experimental] Invalid SSH public key at {}: {:?}",
                    path.display(),
                    e
                ));
            }
        }
    }

    // Try to load as private key and derive public
    if path.extension().and_then(|s| s.to_str()) == Some("pub") {
        Err(eyre!(
            "[experimental] Invalid SSH public key at {}",
            path.display()
        ))
    } else {
        load_ssh_recipient_from_private_key(path)
            .await
            .and_then(|s| {
                parse_recipient(&s)?
                    .ok_or_else(|| eyre!("[experimental] Failed to parse SSH recipient"))
            })
    }
}

async fn load_ssh_recipient_from_private_key(path: &Path) -> Result<String> {
    // For SSH keys, we can't easily derive the public key from the private key using the age crate
    // So we'll try to read the corresponding .pub file
    let pub_path = path.with_extension("pub");
    if pub_path.exists() {
        let content = file::read_to_string(&pub_path)?;
        let trimmed = content.trim();
        if trimmed.starts_with("ssh-") {
            return Ok(trimmed.to_string());
        }
    }

    Err(eyre!(
        "[experimental] Could not find public key for SSH private key at {}. Expected {}.pub",
        path.display(),
        path.display()
    ))
}

async fn load_all_identities() -> Result<Vec<Box<dyn Identity>>> {
    // Get identity files first
    let identity_files = get_all_identity_files().await;
    let ssh_identity_files = get_all_ssh_identity_files();

    // Now process identities without holding them across await points
    let mut identities: Vec<Box<dyn Identity>> = Vec::new();

    // Check MISE_AGE_KEY environment variable
    if let Ok(age_key) = env::var("MISE_AGE_KEY") {
        if !age_key.is_empty() {
            // First try to parse as a raw age secret key
            for line in age_key.lines() {
                let line = line.trim();
                if line.starts_with("AGE-SECRET-KEY-") {
                    if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                        identities.push(Box::new(identity));
                    }
                }
            }

            // If no keys were found, try parsing as an identity file
            if identities.is_empty() {
                if let Ok(identity_file) = IdentityFile::from_buffer(age_key.as_bytes()) {
                    if let Ok(mut file_identities) = identity_file.into_identities() {
                        identities.append(&mut file_identities);
                    }
                }
            }
        }
    }

    // Load from identity files
    for path in identity_files {
        if path.exists() {
            match file::read_to_string(&path) {
                Ok(content) => {
                    if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes()) {
                        if let Ok(mut file_identities) = identity_file.into_identities() {
                            identities.append(&mut file_identities);
                        }
                    }
                }
                Err(e) => {
                    debug!(
                        "[experimental] Failed to read identity file {:?}: {}",
                        path, e
                    );
                }
            }
        }
    }

    // Load SSH identities
    for path in ssh_identity_files {
        if path.exists() {
            match std::fs::File::open(&path) {
                Ok(file) => {
                    let mut reader = BufReader::new(file);
                    match ssh::Identity::from_buffer(&mut reader, Some(path.display().to_string()))
                    {
                        Ok(identity) => {
                            identities.push(Box::new(identity));
                        }
                        Err(e) => {
                            debug!(
                                "[experimental] Failed to parse SSH identity from {:?}: {}",
                                path, e
                            );
                        }
                    }
                }
                Err(e) => {
                    debug!(
                        "[experimental] Failed to read SSH identity file {:?}: {}",
                        path, e
                    );
                }
            }
        }
    }

    Ok(identities)
}

async fn get_default_key_file() -> Option<PathBuf> {
    Settings::get()
        .age
        .key_file
        .clone()
        .map(replace_path)
        .or_else(|| {
            let default_path = dirs::CONFIG.join("age.txt");
            if default_path.exists() {
                Some(default_path)
            } else {
                None
            }
        })
}

async fn get_all_identity_files() -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Some(ref identity_files) = Settings::get().age.identity_files {
        for path in identity_files {
            // Apply path expansion for tilde and environment variables
            files.push(replace_path(path.clone()));
        }
    }

    if let Some(key_file) = Settings::get().age.key_file.clone() {
        files.push(replace_path(key_file));
    }

    let default_age_txt = dirs::CONFIG.join("age.txt");
    if default_age_txt.exists() && !files.contains(&default_age_txt) {
        files.push(default_age_txt);
    }

    files
}

fn get_all_ssh_identity_files() -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Some(ref ssh_identity_files) = Settings::get().age.ssh_identity_files {
        for path in ssh_identity_files {
            // Apply path expansion for tilde and environment variables
            files.push(replace_path(path.clone()));
        }
    }

    files.extend(get_default_ssh_key_paths());
    files
}

fn get_default_ssh_key_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = &*dirs::HOME;
    let ssh_dir = home.join(".ssh");
    paths.push(ssh_dir.join("id_ed25519"));
    paths.push(ssh_dir.join("id_rsa"));
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_age_x25519_round_trip_small() -> Result<()> {
        let key = age::x25519::Identity::generate();
        let recipient = key.to_public();

        // Small value should not be compressed
        let plaintext = "secret value";
        let recipients: Vec<Box<dyn Recipient + Send>> = vec![Box::new(recipient)];
        let directive =
            create_age_directive("TEST_VAR".to_string(), plaintext, &recipients).await?;

        if let crate::config::env_directive::EnvDirective::Age { value, format, .. } = directive {
            // Small value should not be compressed (format should be None/Raw)
            assert!(
                format.is_none()
                    || matches!(format, Some(crate::config::env_directive::AgeFormat::Raw))
            );

            use age::secrecy::ExposeSecret;
            env::set_var("MISE_AGE_KEY", key.to_string().expose_secret());
            let decrypted =
                decrypt_age_directive(&crate::config::env_directive::EnvDirective::Age {
                    key: "TEST_VAR".to_string(),
                    value,
                    format,
                    options: Default::default(),
                })
                .await?;
            env::remove_var("MISE_AGE_KEY");

            assert_eq!(decrypted, plaintext);
        } else {
            panic!("Expected Age directive");
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_age_x25519_round_trip_large() -> Result<()> {
        let key = age::x25519::Identity::generate();
        let recipient = key.to_public();

        // Large value should be compressed (>1KB)
        let plaintext = "x".repeat(2000);
        let recipients: Vec<Box<dyn Recipient + Send>> = vec![Box::new(recipient)];
        let directive =
            create_age_directive("TEST_VAR".to_string(), &plaintext, &recipients).await?;

        if let crate::config::env_directive::EnvDirective::Age { value, format, .. } = directive {
            // Large value should be compressed
            assert_eq!(format, Some(crate::config::env_directive::AgeFormat::Zstd));

            use age::secrecy::ExposeSecret;
            env::set_var("MISE_AGE_KEY", key.to_string().expose_secret());
            let decrypted =
                decrypt_age_directive(&crate::config::env_directive::EnvDirective::Age {
                    key: "TEST_VAR".to_string(),
                    value,
                    format,
                    options: Default::default(),
                })
                .await?;
            env::remove_var("MISE_AGE_KEY");

            assert_eq!(decrypted, plaintext);
        } else {
            panic!("Expected Age directive");
        }
        Ok(())
    }

    #[test]
    fn test_parse_recipient() -> Result<()> {
        let age_recipient = "age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p";
        let parsed = parse_recipient(age_recipient)?;
        assert!(parsed.is_some());

        // Note: The SSH recipient parser in the age crate is strict about format
        // This is a valid format example
        let ssh_recipient =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJmkfJ8VZq4m5k7tJVts7+nR01fbRvLHLgeQCF6FWYr5";
        let parsed = parse_recipient(ssh_recipient)?;
        assert!(parsed.is_some());

        let invalid = "invalid_recipient";
        let parsed = parse_recipient(invalid)?;
        assert!(parsed.is_none());

        Ok(())
    }
}
