use clap::Args;
use eyre::{Result, eyre};
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::agecrypt;
use crate::config::Settings;
use crate::file;

/// Encrypt stdin or file(s) using recipients from flags or defaults
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct Encrypt {
    /// Files to encrypt (stdin if not specified)
    #[clap(value_hint = clap::ValueHint::FilePath)]
    files: Vec<PathBuf>,

    /// Age recipient(s) to encrypt to
    #[clap(long = "recipient", short = 'r')]
    recipients: Vec<String>,

    /// SSH recipient(s) to encrypt to (public key or file)
    #[clap(long = "ssh-recipient")]
    ssh_recipients: Vec<String>,

    /// Identity file to derive recipient from
    #[clap(long = "identity-file", short = 'i')]
    identity_file: Option<PathBuf>,

    /// Output in ASCII armor format
    #[clap(long, short = 'a')]
    armor: bool,

    /// Encrypt files in place
    #[clap(long)]
    in_place: bool,

    /// Output file (stdout if not specified)
    #[clap(long, short = 'o', value_hint = clap::ValueHint::FilePath)]
    out: Option<PathBuf>,

    /// Suffix to add when encrypting in place
    #[clap(long, default_value = ".age")]
    suffix: String,
}

impl Encrypt {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age encrypt")?;

        // Collect recipients
        let recipients = self.collect_recipients().await?;

        if recipients.is_empty() {
            return Err(eyre!(
                "No recipients specified. Use --recipient, --ssh-recipient, or configure defaults"
            ));
        }

        if self.files.is_empty() {
            // Read from stdin
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input)?;

            let encrypted = self.encrypt_data(&input, &recipients)?;

            if let Some(out_path) = self.out {
                file::write(&out_path, &encrypted)?;
            } else {
                std::io::stdout().write_all(&encrypted)?;
            }
        } else {
            // Encrypt files
            for file_path in &self.files {
                if !file_path.exists() {
                    return Err(eyre!("File not found: {}", file_path.display()));
                }

                let input = file::read(file_path)?;
                let encrypted = self.encrypt_data(&input, &recipients)?;

                let output_path = if self.in_place {
                    let mut new_path = file_path.clone();
                    let new_name = format!(
                        "{}{}",
                        file_path.file_name().unwrap().to_string_lossy(),
                        self.suffix
                    );
                    new_path.set_file_name(new_name);
                    new_path
                } else if let Some(ref out_path) = self.out {
                    out_path.clone()
                } else {
                    // Write to stdout
                    std::io::stdout().write_all(&encrypted)?;
                    continue;
                };

                file::write(&output_path, &encrypted)?;
                eprintln!(
                    "Encrypted {} -> {}",
                    file_path.display(),
                    output_path.display()
                );
            }
        }

        Ok(())
    }

    async fn collect_recipients(&self) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
        let mut recipients: Vec<Box<dyn age::Recipient + Send>> = Vec::new();

        // Add explicit age recipients
        for recipient_str in &self.recipients {
            if let Some(recipient) = agecrypt::parse_recipient(recipient_str)? {
                recipients.push(recipient);
            }
        }

        // Add SSH recipients
        for ssh_str in &self.ssh_recipients {
            if ssh_str.starts_with("ssh-") {
                // Direct SSH public key
                if let Some(recipient) = agecrypt::parse_recipient(ssh_str)? {
                    recipients.push(recipient);
                }
            } else {
                // Try as file path
                let path = PathBuf::from(ssh_str);
                if path.exists() {
                    let recipient = agecrypt::load_ssh_recipient_from_path(&path).await?;
                    recipients.push(recipient);
                }
            }
        }

        // Add recipients from identity file
        if let Some(identity_path) = &self.identity_file {
            let file_recipients = agecrypt::load_recipients_from_key_file(identity_path).await?;
            recipients.extend(file_recipients);
        }

        // If no explicit recipients, load defaults
        if recipients.is_empty() {
            let default_recipients = agecrypt::load_recipients_from_defaults().await?;
            recipients.extend(default_recipients);
        }

        Ok(recipients)
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

        if self.armor {
            let armor = age::armor::ArmoredWriter::wrap_output(
                &mut encrypted,
                age::armor::Format::AsciiArmor,
            )?;
            let mut writer = encryptor.wrap_output(armor)?;
            writer.write_all(data)?;
            writer.finish()?;
        } else {
            let mut writer = encryptor.wrap_output(&mut encrypted)?;
            writer.write_all(data)?;
            writer.finish()?;
        }

        Ok(encrypted)
    }
}
