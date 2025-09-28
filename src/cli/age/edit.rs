use clap::Args;
use eyre::{Result, WrapErr, eyre};
use std::io::{Read, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;

use crate::config::Settings;
use crate::file;

/// Decrypt to temp, open $EDITOR, re-encrypt with original recipients
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct Edit {
    /// File to edit
    #[clap(value_hint = clap::ValueHint::FilePath)]
    file: PathBuf,

    /// Editor to use (defaults to $EDITOR)
    #[clap(long, short = 'e')]
    editor: Option<String>,

    /// Identity file(s) to use for decryption
    #[clap(long = "identity-file", short = 'i')]
    identity_files: Vec<PathBuf>,

    /// Override recipients for re-encryption (age1...)
    #[clap(long = "recipient", short = 'r')]
    recipients: Vec<String>,

    /// Override SSH recipients for re-encryption
    #[clap(long = "ssh-recipient")]
    ssh_recipients: Vec<String>,

    /// Don't create backup file
    #[clap(long)]
    no_backup: bool,
}

impl Edit {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age edit")?;

        if !self.file.exists() {
            return Err(eyre!("File not found: {}", self.file.display()));
        }

        // Determine editor
        let editor = self
            .editor
            .clone()
            .or_else(|| std::env::var("EDITOR").ok())
            .unwrap_or_else(|| "vi".to_string());

        // Read and decrypt the file
        let encrypted_content = file::read(&self.file)?;
        let (decrypted, original_recipients) =
            self.decrypt_with_metadata(&encrypted_content).await?;

        // Create temporary file with decrypted content
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(&decrypted)?;
        temp_file.flush()?;

        // Open editor
        let status = std::process::Command::new(&editor)
            .arg(temp_file.path())
            .status()
            .wrap_err_with(|| format!("Failed to launch editor: {}", editor))?;

        if !status.success() {
            return Err(eyre!("Editor exited with non-zero status"));
        }

        // Read edited content
        let mut edited = Vec::new();
        temp_file.reopen()?.read_to_end(&mut edited)?;

        // Check if content changed
        if edited == decrypted {
            eprintln!("No changes made");
            return Ok(());
        }

        // Determine recipients for re-encryption
        let recipients = if !self.recipients.is_empty() || !self.ssh_recipients.is_empty() {
            // Use override recipients
            self.collect_override_recipients().await?
        } else {
            // Use original recipients or defaults
            match original_recipients {
                Some(future) => future.resolve().await?,
                None => self.collect_default_recipients().await?,
            }
        };

        // Create backup if requested
        if !self.no_backup {
            let backup_path = format!("{}.bak", self.file.display());
            file::copy(&self.file, &backup_path)?;
            eprintln!("Created backup: {}", backup_path);
        }

        // Re-encrypt and save
        let encrypted = self.encrypt_data(&edited, &recipients)?;
        file::write(&self.file, &encrypted)?;

        eprintln!("File updated: {}", self.file.display());

        Ok(())
    }

    async fn decrypt_with_metadata(
        &self,
        data: &[u8],
    ) -> Result<(Vec<u8>, Option<RecipientsFuture>)> {
        use age::Decryptor;

        // Load identities for decryption
        let identities = self.load_identities().await?;

        if identities.is_empty() {
            return Err(eyre!("No identities found for decryption"));
        }

        // Try to extract recipient info from the file (if ASCII armor)
        let recipients_future = if let Ok(text) = std::str::from_utf8(data) {
            if text.starts_with("-----BEGIN AGE ENCRYPTED FILE-----") {
                Some(RecipientsFuture(self.extract_recipients_from_armor(text)?))
            } else {
                None
            }
        } else {
            None
        };

        // Decrypt the data
        let decryptor = Decryptor::new(data)?;

        let identity_refs: Vec<&dyn age::Identity> = identities
            .iter()
            .map(|i| i.as_ref() as &dyn age::Identity)
            .collect();

        let mut decrypted = Vec::new();
        match decryptor.decrypt(identity_refs.into_iter()) {
            Ok(mut reader) => {
                reader.read_to_end(&mut decrypted)?;
            }
            Err(e) => {
                return Err(eyre!("Failed to decrypt: {}", e));
            }
        }

        Ok((decrypted, recipients_future))
    }

    fn extract_recipients_from_armor(
        &self,
        text: &str,
    ) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
        let mut recipients = Vec::new();

        for line in text.lines().skip(1) {
            if line.starts_with("---") {
                break;
            }
            if line.starts_with("-> X25519 ") {
                if let Some(key) = line.strip_prefix("-> X25519 ") {
                    let key = key.split_whitespace().next().unwrap_or("");
                    if let Ok(recipient) = key.parse::<age::x25519::Recipient>() {
                        recipients.push(Box::new(recipient) as Box<dyn age::Recipient + Send>);
                    }
                }
            }
        }

        Ok(recipients)
    }

    async fn load_identities(&self) -> Result<Vec<Box<dyn age::Identity>>> {
        use age::IdentityFile;

        let mut identities: Vec<Box<dyn age::Identity>> = Vec::new();

        // Load explicit identity files
        if !self.identity_files.is_empty() {
            for path in &self.identity_files {
                if !path.exists() {
                    return Err(eyre!("Identity file not found: {}", path.display()));
                }

                let content = file::read_to_string(path)?;
                if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes()) {
                    if let Ok(mut file_identities) = identity_file.into_identities() {
                        identities.append(&mut file_identities);
                    }
                }
            }
        } else {
            // Load default identities
            if let Ok(age_key) = std::env::var("MISE_AGE_KEY") {
                for line in age_key.lines() {
                    let line = line.trim();
                    if line.starts_with("AGE-SECRET-KEY-") {
                        if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                            identities.push(Box::new(identity));
                        }
                    }
                }
            }

            // Load from default locations
            let default_age = crate::dirs::CONFIG.join("age.txt");
            if default_age.exists() {
                let content = file::read_to_string(&default_age)?;
                if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes()) {
                    if let Ok(mut file_identities) = identity_file.into_identities() {
                        identities.append(&mut file_identities);
                    }
                }
            }
        }

        Ok(identities)
    }

    async fn collect_override_recipients(&self) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
        let mut recipients: Vec<Box<dyn age::Recipient + Send>> = Vec::new();

        for recipient_str in &self.recipients {
            if let Ok(recipient) = recipient_str.parse::<age::x25519::Recipient>() {
                recipients.push(Box::new(recipient));
            }
        }

        for ssh_str in &self.ssh_recipients {
            if let Ok(recipient) = ssh_str.parse::<age::ssh::Recipient>() {
                recipients.push(Box::new(recipient));
            }
        }

        Ok(recipients)
    }

    async fn collect_default_recipients(&self) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
        crate::agecrypt::load_recipients_from_defaults().await
    }

    fn encrypt_data(
        &self,
        data: &[u8],
        recipients: &[Box<dyn age::Recipient + Send>],
    ) -> Result<Vec<u8>> {
        use age::Encryptor;

        let encryptor = Encryptor::with_recipients(
            recipients.iter().map(|r| r.as_ref() as &dyn age::Recipient),
        )
        .map_err(|e| eyre!("Failed to create encryptor: {}", e))?;

        let mut encrypted = Vec::new();

        // Use armor format to preserve recipient visibility
        let armor =
            age::armor::ArmoredWriter::wrap_output(&mut encrypted, age::armor::Format::AsciiArmor)?;
        let mut writer = encryptor.wrap_output(armor)?;
        writer.write_all(data)?;
        writer.finish()?;

        Ok(encrypted)
    }
}

// Wrapper to make the async future work
struct RecipientsFuture(Vec<Box<dyn age::Recipient + Send>>);

impl RecipientsFuture {
    async fn resolve(self) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
        Ok(self.0)
    }
}
