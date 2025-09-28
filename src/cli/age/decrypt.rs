use clap::Args;
use eyre::{Result, WrapErr, eyre};
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::config::Settings;
use crate::file;

/// Decrypt stdin or file(s) using identities from flags or defaults
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct Decrypt {
    /// Files to decrypt (stdin if not specified)
    #[clap(value_hint = clap::ValueHint::FilePath)]
    files: Vec<PathBuf>,

    /// Identity file(s) to use for decryption
    #[clap(long = "identity-file", short = 'i')]
    identity_files: Vec<PathBuf>,

    /// Output file (stdout if not specified)
    #[clap(long, short = 'o', value_hint = clap::ValueHint::FilePath)]
    out: Option<PathBuf>,

    /// Decrypt files in place (removes suffix)
    #[clap(long)]
    in_place: bool,
}

impl Decrypt {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age decrypt")?;

        // Load identities
        let identities = self.load_identities().await?;

        if identities.is_empty() {
            return Err(eyre!(
                "No identities found for decryption. Use --identity-file or configure defaults"
            ));
        }

        if self.files.is_empty() {
            // Read from stdin
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input)?;

            let decrypted = self.decrypt_data(&input, &identities)?;

            if let Some(out_path) = self.out {
                file::write(&out_path, &decrypted)?;
            } else {
                std::io::stdout().write_all(&decrypted)?;
            }
        } else {
            // Decrypt files
            for file_path in &self.files {
                if !file_path.exists() {
                    return Err(eyre!("File not found: {}", file_path.display()));
                }

                let input = file::read(file_path)?;
                let decrypted = self.decrypt_data(&input, &identities)?;

                let output_path = if self.in_place {
                    // Remove .age suffix if present
                    let file_str = file_path.to_string_lossy();
                    if let Some(base) = file_str.strip_suffix(".age") {
                        PathBuf::from(base)
                    } else {
                        return Err(eyre!(
                            "Cannot decrypt {} in place: doesn't end with .age",
                            file_path.display()
                        ));
                    }
                } else if let Some(ref out_path) = self.out {
                    out_path.clone()
                } else {
                    // Write to stdout
                    std::io::stdout().write_all(&decrypted)?;
                    continue;
                };

                file::write(&output_path, &decrypted)?;
                eprintln!(
                    "Decrypted {} -> {}",
                    file_path.display(),
                    output_path.display()
                );
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

                let content = file::read_to_string(path)
                    .wrap_err_with(|| format!("Failed to read {}", path.display()))?;

                // Try as age identity file
                if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes()) {
                    if let Ok(mut file_identities) = identity_file.into_identities() {
                        identities.append(&mut file_identities);
                    }
                }

                // Try as SSH identity
                if path.extension().and_then(|s| s.to_str()) != Some("pub") {
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
            }
        } else {
            // Load default identities using the existing function from agecrypt
            // We need to expose this functionality
            identities = self.load_default_identities().await?;
        }

        Ok(identities)
    }

    async fn load_default_identities(&self) -> Result<Vec<Box<dyn age::Identity>>> {
        use age::IdentityFile;
        use std::io::BufReader;

        let mut identities: Vec<Box<dyn age::Identity>> = Vec::new();

        // Check MISE_AGE_KEY environment variable
        if let Ok(age_key) = std::env::var("MISE_AGE_KEY") {
            if !age_key.is_empty() {
                for line in age_key.lines() {
                    let line = line.trim();
                    if line.starts_with("AGE-SECRET-KEY-") {
                        if let Ok(identity) = line.parse::<age::x25519::Identity>() {
                            identities.push(Box::new(identity));
                        }
                    }
                }

                if identities.is_empty() {
                    if let Ok(identity_file) = IdentityFile::from_buffer(age_key.as_bytes()) {
                        if let Ok(mut file_identities) = identity_file.into_identities() {
                            identities.append(&mut file_identities);
                        }
                    }
                }
            }
        }

        // Load from settings
        let settings = Settings::get();

        // Check settings.age.key_file and settings.age.identity_files
        let mut identity_paths = Vec::new();

        if let Some(key_file) = &settings.age.key_file {
            identity_paths.push(crate::file::replace_path(key_file.clone()));
        }

        if let Some(identity_files) = &settings.age.identity_files {
            for path in identity_files {
                identity_paths.push(crate::file::replace_path(path.clone()));
            }
        }

        // Add default age.txt
        let default_age = crate::dirs::CONFIG.join("age.txt");
        if default_age.exists() && !identity_paths.contains(&default_age) {
            identity_paths.push(default_age);
        }

        // Load age identities
        for path in identity_paths {
            if path.exists() {
                if let Ok(content) = file::read_to_string(&path) {
                    if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes()) {
                        if let Ok(mut file_identities) = identity_file.into_identities() {
                            identities.append(&mut file_identities);
                        }
                    }
                }
            }
        }

        // Load SSH identities
        if let Some(ssh_files) = &settings.age.ssh_identity_files {
            for path in ssh_files {
                let path = crate::file::replace_path(path.clone());
                if path.exists() {
                    if let Ok(file) = std::fs::File::open(&path) {
                        let mut reader = BufReader::new(file);
                        if let Ok(ssh_identity) = age::ssh::Identity::from_buffer(
                            &mut reader,
                            Some(path.display().to_string()),
                        ) {
                            identities.push(Box::new(ssh_identity));
                        }
                    }
                }
            }
        }

        // Add default SSH keys
        let home = &*crate::dirs::HOME;
        let ssh_dir = home.join(".ssh");
        let default_ssh_keys = vec![ssh_dir.join("id_ed25519"), ssh_dir.join("id_rsa")];

        for path in default_ssh_keys {
            if path.exists() {
                if let Ok(file) = std::fs::File::open(&path) {
                    let mut reader = BufReader::new(file);
                    if let Ok(ssh_identity) = age::ssh::Identity::from_buffer(
                        &mut reader,
                        Some(path.display().to_string()),
                    ) {
                        identities.push(Box::new(ssh_identity));
                    }
                }
            }
        }

        Ok(identities)
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
}
