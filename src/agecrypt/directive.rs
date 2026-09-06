//! The experimental `[env]` age directives: `mise set --age-encrypt` writes
//! them and environment resolution decrypts them. Everything here stays
//! behind `settings.experimental` (the gate is in `decrypt_age_directive`
//! and in `mise set`); the encryption of machine backups in the parent
//! module does not go through this layer.
//!
//! The envelope is age, then zstd above a size threshold, then unpadded
//! base64, and must stay that way: values already written to mise.toml
//! files are read back with it.

use std::io::{Read, Write};
use std::path::Path;

use age::{Decryptor, Encryptor, Identity, Recipient};
use base64::Engine;
use eyre::{Result, WrapErr, eyre};

use super::{
    ZSTD_COMPRESSION_LEVEL, default_recipient_strings, load_all_identities, parse_recipient,
    ssh_public_key_for_private, unusable_identity_hint,
};
use crate::config::Settings;
use crate::config::env_directive::{AgeFormat, EnvDirective, EnvDirectiveOptions};
use crate::file;

const COMPRESSION_THRESHOLD: usize = 1024; // 1KB

pub(crate) async fn create_age_directive(
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

pub(crate) async fn decrypt_age_directive(directive: &EnvDirective) -> Result<String> {
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

            let loaded = load_all_identities().await;
            if loaded.identities.is_empty() {
                return Err(eyre!(
                    "[experimental] No age identities found for decryption"
                ));
            }

            let decryptor = Decryptor::new(&ciphertext[..])?;
            let mut decrypted = Vec::new();

            let identity_refs: Vec<&dyn Identity> = loaded
                .identities
                .iter()
                .map(|i| i.as_ref() as &dyn Identity)
                .collect();

            match decryptor.decrypt(identity_refs.into_iter()) {
                Ok(mut reader) => {
                    reader.read_to_end(&mut decrypted)?;
                }
                Err(e) => {
                    return Err(eyre!(
                        "[experimental] Failed to decrypt: {e}{}",
                        unusable_identity_hint(&loaded.unusable)
                    ));
                }
            }

            String::from_utf8(decrypted)
                .wrap_err("[experimental] Decrypted value is not valid UTF-8")
        }
        _ => Err(eyre!("[experimental] Not an Age directive")),
    }
}

pub(crate) async fn load_recipients_from_defaults() -> Result<Vec<Box<dyn Recipient + Send>>> {
    let mut parsed_recipients: Vec<Box<dyn Recipient + Send>> = Vec::new();
    for recipient_str in default_recipient_strings().await? {
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

pub(crate) async fn load_recipients_from_key_file(
    path: &Path,
) -> Result<Vec<Box<dyn Recipient + Send>>> {
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
        if line.starts_with("AGE-SECRET-KEY-")
            && let Ok(identity) = line.parse::<age::x25519::Identity>()
        {
            let public_key = identity.to_public();
            recipients.push(Box::new(public_key));
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

pub(crate) async fn load_ssh_recipient_from_path(path: &Path) -> Result<Box<dyn Recipient + Send>> {
    let content = file::read_to_string(path)?;
    let trimmed = content.trim();

    // Check if it's a public key
    if trimmed.starts_with("ssh-") {
        match trimmed.parse::<age::ssh::Recipient>() {
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
        ssh_public_key_for_private(path).await.and_then(|s| {
            parse_recipient(&s)?
                .ok_or_else(|| eyre!("[experimental] Failed to parse SSH recipient"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env;

    #[tokio::test]
    async fn test_age_x25519_round_trip_small() -> Result<()> {
        let key = age::x25519::Identity::generate();
        let recipient = key.to_public();

        // Small value should not be compressed
        let plaintext = "secret value";
        let recipients: Vec<Box<dyn Recipient + Send>> = vec![Box::new(recipient)];
        let directive =
            create_age_directive("TEST_VAR".to_string(), plaintext, &recipients).await?;

        if let EnvDirective::Age { value, format, .. } = directive {
            // Small value should not be compressed (format should be None/Raw)
            assert!(format.is_none() || matches!(format, Some(AgeFormat::Raw)));

            use age::secrecy::ExposeSecret;
            env::set_var("MISE_AGE_KEY", key.to_string().expose_secret());
            let decrypted = decrypt_age_directive(&EnvDirective::Age {
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

        if let EnvDirective::Age { value, format, .. } = directive {
            // Large value should be compressed
            assert_eq!(format, Some(AgeFormat::Zstd));

            use age::secrecy::ExposeSecret;
            env::set_var("MISE_AGE_KEY", key.to_string().expose_secret());
            let decrypted = decrypt_age_directive(&EnvDirective::Age {
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
}
