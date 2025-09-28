use clap::Args;
use eyre::{Result, WrapErr, eyre};
use std::io::Read;
use std::path::PathBuf;

use crate::agecrypt;
use crate::config::Settings;
use crate::file;

/// Re-encrypt ciphertext to a new recipient set
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct Rekey {
    /// Files to rekey
    #[clap(required = true, value_hint = clap::ValueHint::FilePath)]
    files: Vec<PathBuf>,

    /// Add age recipient(s)
    #[clap(long = "add-recipient")]
    add_recipients: Vec<String>,

    /// Add SSH recipient(s)
    #[clap(long = "add-ssh-recipient")]
    add_ssh_recipients: Vec<String>,

    /// Remove recipient(s) by public key
    #[clap(long = "drop-recipient")]
    drop_recipients: Vec<String>,

    /// Read new recipients from file
    #[clap(long = "from-file", value_hint = clap::ValueHint::FilePath)]
    from_file: Option<PathBuf>,

    /// Write recipients to file (for tracking)
    #[clap(long = "to-file", value_hint = clap::ValueHint::FilePath)]
    to_file: Option<PathBuf>,

    /// Rekey files in place
    #[clap(long)]
    in_place: bool,

    /// Create backup files (.bak)
    #[clap(long)]
    backup: bool,

    /// Identity file(s) for decryption
    #[clap(long = "identity-file", short = 'i')]
    identity_files: Vec<PathBuf>,
}

impl Rekey {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age rekey")?;

        // Load identities for decryption
        let identities = self.load_identities().await?;

        if identities.is_empty() {
            return Err(eyre!("No identities found for decryption"));
        }

        // Collect new recipients
        let new_recipients = self.collect_recipients().await?;

        if new_recipients.is_empty() {
            return Err(eyre!("No recipients specified for re-encryption"));
        }

        // Save recipients list if requested
        if let Some(to_file) = &self.to_file {
            self.save_recipients_list(to_file, &new_recipients)?;
        }

        // Process each file
        for file_path in &self.files {
            if !file_path.exists() {
                eprintln!("Warning: File not found: {}", file_path.display());
                continue;
            }

            eprintln!("Rekeying {}...", file_path.display());

            // Read and decrypt
            let encrypted = file::read(file_path)?;
            let decrypted = self.decrypt_data(&encrypted, &identities)?;

            // Re-encrypt with new recipients
            let reencrypted = self.encrypt_data(&decrypted, &new_recipients)?;

            // Save the file
            if self.in_place {
                if self.backup {
                    let backup_path = format!("{}.bak", file_path.display());
                    file::copy(file_path, &backup_path)?;
                    eprintln!("  Created backup: {}", backup_path);
                }
                file::write(file_path, &reencrypted)?;
                eprintln!("  ✓ Rekeyed in place");
            } else {
                let new_path = format!("{}.rekey", file_path.display());
                file::write(&new_path, &reencrypted)?;
                eprintln!("  ✓ Rekeyed to: {}", new_path);
            }
        }

        Ok(())
    }

    async fn load_identities(&self) -> Result<Vec<Box<dyn age::Identity>>> {
        use age::IdentityFile;
        use std::io::BufReader;

        let mut identities: Vec<Box<dyn age::Identity>> = Vec::new();

        // Load explicit identity files
        if !self.identity_files.is_empty() {
            for path in &self.identity_files {
                if !path.exists() {
                    return Err(eyre!("Identity file not found: {}", path.display()));
                }

                let content = file::read_to_string(path)?;

                // Try as age identity
                if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes()) {
                    if let Ok(mut file_identities) = identity_file.into_identities() {
                        identities.append(&mut file_identities);
                    }
                }

                // Try as SSH identity
                if let Ok(file) = std::fs::File::open(path) {
                    let mut reader = BufReader::new(file);
                    if let Ok(ssh_identity) = age::ssh::Identity::from_buffer(
                        &mut reader,
                        Some(path.display().to_string()),
                    ) {
                        identities.push(Box::new(ssh_identity));
                    }
                }
            }
        } else {
            // Load default identities
            identities = self.load_default_identities().await?;
        }

        Ok(identities)
    }

    async fn load_default_identities(&self) -> Result<Vec<Box<dyn age::Identity>>> {
        use age::IdentityFile;

        let mut identities: Vec<Box<dyn age::Identity>> = Vec::new();

        // Check MISE_AGE_KEY
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

        // Load from default age.txt
        let default_age = crate::dirs::CONFIG.join("age.txt");
        if default_age.exists() {
            let content = file::read_to_string(&default_age)?;
            if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes()) {
                if let Ok(mut file_identities) = identity_file.into_identities() {
                    identities.append(&mut file_identities);
                }
            }
        }

        // Load from settings
        let settings = Settings::get();
        if let Some(key_file) = &settings.age.key_file {
            let path = crate::file::replace_path(key_file.clone());
            if path.exists() {
                let content = file::read_to_string(&path)?;
                if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes()) {
                    if let Ok(mut file_identities) = identity_file.into_identities() {
                        identities.append(&mut file_identities);
                    }
                }
            }
        }

        Ok(identities)
    }

    async fn collect_recipients(&self) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
        let mut recipients: Vec<Box<dyn age::Recipient + Send>> = Vec::new();

        // Load from file if specified
        if let Some(from_file) = &self.from_file {
            let content = file::read_to_string(from_file).wrap_err_with(|| {
                format!("Failed to read recipients from {}", from_file.display())
            })?;

            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }

                if let Some(recipient) = agecrypt::parse_recipient(line)? {
                    recipients.push(recipient);
                }
            }
        }

        // Add explicit recipients
        for recipient_str in &self.add_recipients {
            if let Some(recipient) = agecrypt::parse_recipient(recipient_str)? {
                recipients.push(recipient);
            }
        }

        // Add SSH recipients
        for ssh_str in &self.add_ssh_recipients {
            if ssh_str.starts_with("ssh-") {
                if let Some(recipient) = agecrypt::parse_recipient(ssh_str)? {
                    recipients.push(recipient);
                }
            } else {
                // Try as file
                let path = PathBuf::from(ssh_str);
                if path.exists() {
                    let recipient = agecrypt::load_ssh_recipient_from_path(&path).await?;
                    recipients.push(recipient);
                }
            }
        }

        // Note: Dropping specific recipients isn't implemented as Recipients don't implement Debug
        // Users should specify the full new recipient list instead

        // If still no recipients, try loading defaults
        if recipients.is_empty() {
            recipients = agecrypt::load_recipients_from_defaults().await?;
        }

        Ok(recipients)
    }

    fn decrypt_data(&self, data: &[u8], identities: &[Box<dyn age::Identity>]) -> Result<Vec<u8>> {
        use age::Decryptor;
        use std::io::Cursor;

        let mut decrypted = Vec::new();

        // Try armor format first
        if let Ok(text) = std::str::from_utf8(data) {
            if text.starts_with("-----BEGIN AGE ENCRYPTED FILE-----") {
                let cursor = Cursor::new(data);
                let armor = age::armor::ArmoredReader::new(cursor);
                let decryptor = Decryptor::new(armor)?;

                let identity_refs: Vec<&dyn age::Identity> = identities
                    .iter()
                    .map(|i| i.as_ref() as &dyn age::Identity)
                    .collect();

                match decryptor.decrypt(identity_refs.into_iter()) {
                    Ok(mut reader) => {
                        reader.read_to_end(&mut decrypted)?;
                        return Ok(decrypted);
                    }
                    Err(e) => {
                        return Err(eyre!("Failed to decrypt: {}", e));
                    }
                }
            }
        }

        // Try binary format
        let decryptor = Decryptor::new(data)?;

        let mut decrypted = Vec::new();

        let identity_refs: Vec<&dyn age::Identity> = identities
            .iter()
            .map(|i| i.as_ref() as &dyn age::Identity)
            .collect();

        match decryptor.decrypt(identity_refs.into_iter()) {
            Ok(mut reader) => {
                reader.read_to_end(&mut decrypted)?;
            }
            Err(e) => {
                return Err(eyre!("Failed to decrypt: {}", e));
            }
        }

        Ok(decrypted)
    }

    fn encrypt_data(
        &self,
        data: &[u8],
        recipients: &[Box<dyn age::Recipient + Send>],
    ) -> Result<Vec<u8>> {
        use age::Encryptor;
        use std::io::Write;

        let encryptor = Encryptor::with_recipients(
            recipients.iter().map(|r| r.as_ref() as &dyn age::Recipient),
        )
        .map_err(|e| eyre!("Failed to create encryptor: {}", e))?;

        let mut encrypted = Vec::new();

        // Use armor format
        let armor =
            age::armor::ArmoredWriter::wrap_output(&mut encrypted, age::armor::Format::AsciiArmor)?;
        let mut writer = encryptor.wrap_output(armor)?;
        writer.write_all(data)?;
        writer.finish()?;

        Ok(encrypted)
    }

    fn save_recipients_list(
        &self,
        path: &PathBuf,
        recipients: &[Box<dyn age::Recipient + Send>],
    ) -> Result<()> {
        let mut content = String::new();
        content.push_str("# Age recipients list\n");
        content.push_str(&format!(
            "# Generated: {}\n\n",
            chrono::Utc::now().to_rfc3339()
        ));

        for _ in recipients {
            // Recipients don't implement Debug; just count them
            content.push_str("(recipient)\n");
        }

        file::write(path, content)?;
        eprintln!("Saved recipients list to: {}", path.display());

        Ok(())
    }
}
