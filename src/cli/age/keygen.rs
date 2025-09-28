use age::secrecy::ExposeSecret;
use clap::Args;
use eyre::{Result, WrapErr, eyre};
use std::path::PathBuf;

use crate::config::Settings;
use crate::dirs;
use crate::file;

/// Generate an age identity (x25519)
#[derive(Debug, Args)]
#[clap(verbatim_doc_comment)]
pub struct Keygen {
    /// Output file path for the generated key
    #[clap(long, short = 'o', value_hint = clap::ValueHint::FilePath)]
    out: Option<PathBuf>,

    /// Force overwrite if the key file already exists
    #[clap(long, short = 'f')]
    force: bool,

    /// Add a comment to the key file
    #[clap(long)]
    comment: Option<String>,

    /// Output in ASCII armor format (not applicable for age keys)
    #[clap(long, hide = true)]
    armor: bool,
}

impl Keygen {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("age keygen")?;

        let key_path = self.out.unwrap_or_else(|| dirs::CONFIG.join("age.txt"));

        if key_path.exists() && !self.force {
            return Err(eyre!(
                "Key file already exists at {}. Use --force to overwrite.",
                key_path.display()
            ));
        }

        // Generate a new age identity
        let identity = age::x25519::Identity::generate();
        let public_key = identity.to_public();

        // Build the content
        let mut content = String::new();

        if let Some(comment) = self.comment {
            content.push_str(&format!("# {}\n", comment));
        }

        content.push_str(&format!("# created: {}\n", chrono::Utc::now().to_rfc3339()));
        content.push_str(&format!("# public key: {}\n", public_key));
        content.push_str(identity.to_string().expose_secret());
        content.push('\n');

        // Create parent directories if needed
        if let Some(parent) = key_path.parent() {
            file::create_dir_all(parent)?;
        }

        // Write the key file with restricted permissions
        file::write(&key_path, content.as_bytes())
            .wrap_err_with(|| format!("Failed to write key file to {}", key_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }

        eprintln!("Generated age identity at: {}", key_path.display());
        eprintln!("Public key: {}", public_key);

        Ok(())
    }
}
