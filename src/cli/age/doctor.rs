use atty::Stream;
use clap::Args;
use eyre::Result;
use std::path::PathBuf;

use crate::config::Settings;
use crate::file;

/// Diagnose config and decryptability
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct Doctor {
    /// Test decryption of specific files
    #[clap(value_hint = clap::ValueHint::FilePath)]
    files: Vec<PathBuf>,
}

impl Doctor {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age doctor")?;

        println!("Age Encryption Doctor");
        println!("====================\n");

        self.check_configuration().await?;
        self.check_identities().await?;
        self.check_recipients().await?;

        if !self.files.is_empty() {
            self.check_files(&self.files).await?;
        }

        self.check_environment()?;

        println!("\n✓ Age configuration check complete");

        Ok(())
    }

    async fn check_configuration(&self) -> Result<()> {
        println!("Configuration:");
        println!("--------------");

        let settings = Settings::get();

        // Check key file
        if let Some(key_file) = &settings.age.key_file {
            let path = crate::file::replace_path(key_file.clone());
            if path.exists() {
                println!("✓ Key file: {} (exists)", path.display());
            } else {
                println!("✗ Key file: {} (not found)", path.display());
            }
        } else {
            println!("- Key file: not configured");
        }

        // Check identity files
        if let Some(identity_files) = &settings.age.identity_files {
            for file in identity_files {
                let path = crate::file::replace_path(file.clone());
                if path.exists() {
                    println!("✓ Identity file: {} (exists)", path.display());
                } else {
                    println!("✗ Identity file: {} (not found)", path.display());
                }
            }
        }

        // Check SSH identity files
        if let Some(ssh_files) = &settings.age.ssh_identity_files {
            for file in ssh_files {
                let path = crate::file::replace_path(file.clone());
                if path.exists() {
                    println!("✓ SSH identity: {} (exists)", path.display());

                    // Check for public key
                    let pub_path = path.with_extension("pub");
                    if pub_path.exists() {
                        println!("  ✓ Public key: {} (exists)", pub_path.display());
                    } else {
                        println!("  ✗ Public key: {} (not found)", pub_path.display());
                    }
                } else {
                    println!("✗ SSH identity: {} (not found)", path.display());
                }
            }
        }

        // Check default age.txt
        let default_age = crate::dirs::CONFIG.join("age.txt");
        if default_age.exists() {
            println!("✓ Default age.txt: {} (exists)", default_age.display());
        } else {
            println!("- Default age.txt: not found");
        }

        Ok(())
    }

    async fn check_identities(&self) -> Result<()> {
        println!("\nIdentities:");
        println!("-----------");

        let mut identity_count = 0;

        // Check MISE_AGE_KEY
        if let Ok(age_key) = std::env::var("MISE_AGE_KEY") {
            if !age_key.is_empty() {
                let key_count = age_key
                    .lines()
                    .filter(|line| line.trim().starts_with("AGE-SECRET-KEY-"))
                    .count();
                if key_count > 0 {
                    println!("✓ MISE_AGE_KEY: {} identities", key_count);
                    identity_count += key_count;
                }
            }
        }

        // Count identities from files
        let settings = Settings::get();
        let mut paths = Vec::new();

        if let Some(key_file) = &settings.age.key_file {
            paths.push(crate::file::replace_path(key_file.clone()));
        }

        if let Some(identity_files) = &settings.age.identity_files {
            for file in identity_files {
                paths.push(crate::file::replace_path(file.clone()));
            }
        }

        let default_age = crate::dirs::CONFIG.join("age.txt");
        if default_age.exists() && !paths.contains(&default_age) {
            paths.push(default_age);
        }

        for path in paths {
            if path.exists() {
                if let Ok(content) = file::read_to_string(&path) {
                    let count = content
                        .lines()
                        .filter(|line| line.trim().starts_with("AGE-SECRET-KEY-"))
                        .count();
                    if count > 0 {
                        println!("✓ {}: {} identities", path.display(), count);
                        identity_count += count;
                    }
                }
            }
        }

        // Count SSH identities
        if let Some(ssh_files) = &settings.age.ssh_identity_files {
            for file in ssh_files {
                let path = crate::file::replace_path(file.clone());
                if path.exists() {
                    println!("✓ SSH identity: {}", path.display());
                    identity_count += 1;
                }
            }
        }

        if identity_count == 0 {
            println!("✗ No identities found for decryption");
        } else {
            println!("\nTotal identities available: {}", identity_count);
        }

        Ok(())
    }

    async fn check_recipients(&self) -> Result<()> {
        println!("\nRecipients:");
        println!("-----------");

        // Note: Recipients are not stored in settings currently
        // They are derived from identity files or specified at encryption time
        println!("- Recipients derived from identity files at encryption time");

        Ok(())
    }

    async fn check_files(&self, files: &[PathBuf]) -> Result<()> {
        println!("\nFile Decryption Tests:");
        println!("----------------------");

        for file_path in files {
            if !file_path.exists() {
                println!("✗ {}: File not found", file_path.display());
                continue;
            }

            print!("Testing {}: ", file_path.display());

            // Try to decrypt the file
            match self.test_decrypt(file_path).await {
                Ok(_) => println!("✓ Can decrypt"),
                Err(e) => println!("✗ Cannot decrypt: {}", e),
            }
        }

        Ok(())
    }

    async fn test_decrypt(&self, path: &PathBuf) -> Result<()> {
        use age::Decryptor;

        let content = file::read(path)?;

        // Load identities (simplified version)
        let mut identities: Vec<Box<dyn age::Identity>> = Vec::new();

        // Check MISE_AGE_KEY
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
            }
        }

        // Try to decrypt
        let decryptor = Decryptor::new(&content[..])?;

        let identity_refs: Vec<&dyn age::Identity> = identities
            .iter()
            .map(|i| i.as_ref() as &dyn age::Identity)
            .collect();

        match decryptor.decrypt(identity_refs.into_iter()) {
            Ok(_) => Ok(()),
            Err(e) => Err(eyre::eyre!("{}", e)),
        }
    }

    fn check_environment(&self) -> Result<()> {
        println!("\nEnvironment:");
        println!("------------");

        // Check TTY
        if atty::is(Stream::Stdin) {
            println!("✓ TTY detected (interactive mode)");
        } else {
            println!("- No TTY (non-interactive mode)");
        }

        // Check experimental features
        if std::env::var("MISE_EXPERIMENTAL").is_ok() {
            println!("✓ MISE_EXPERIMENTAL is set");
        } else {
            println!("- MISE_EXPERIMENTAL not set (age features are experimental)");
        }

        Ok(())
    }
}
