use color_eyre::Result;
use color_eyre::eyre::bail;
use console::style;
use self_update::backends::github::Update;
use self_update::{Status, cargo_crate_version};

use crate::cli::version::{ARCH, OS};
use crate::config::Settings;
use crate::env;
use std::collections::BTreeMap;
use std::fs;
#[cfg(target_os = "macos")]
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Default, serde::Deserialize)]
struct InstructionsToml {
    message: Option<String>,
    #[serde(flatten)]
    commands: BTreeMap<String, String>,
}

fn read_instructions_file(path: &PathBuf) -> Option<String> {
    let body = fs::read_to_string(path).ok()?;
    let parsed: InstructionsToml = toml::from_str(&body).ok()?;
    if let Some(msg) = parsed.message {
        return Some(msg);
    }
    if let Some((_k, v)) = parsed.commands.into_iter().next() {
        return Some(v);
    }
    None
}

pub fn upgrade_instructions_text() -> Option<String> {
    if let Some(path) = &*env::MISE_SELF_UPDATE_INSTRUCTIONS
        && let Some(msg) = read_instructions_file(path)
    {
        return Some(msg);
    }
    None
}

/// Appends self-update guidance and packaging instructions (if any) to a message.
pub fn append_self_update_instructions(mut message: String) -> String {
    if SelfUpdate::is_available() {
        message.push_str("\nRun `mise self-update` to update mise");
    }
    if let Some(instructions) = upgrade_instructions_text() {
        message.push('\n');
        message.push_str(&instructions);
    }
    message
}

/// Updates mise itself.
///
/// Uses the GitHub Releases API to find the latest release and binary.
/// By default, this will also update any installed plugins.
/// Uses the `GITHUB_API_TOKEN` environment variable if set for higher rate limits.
///
/// This command is not available if mise is installed via a package manager.
#[derive(Debug, Default, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SelfUpdate {
    /// Update to a specific version
    version: Option<String>,

    /// Update even if already up to date
    #[clap(long, short)]
    force: bool,

    /// Skip confirmation prompt
    #[clap(long, short)]
    yes: bool,

    /// Disable auto-updating plugins
    #[clap(long)]
    no_plugins: bool,
}

impl SelfUpdate {
    pub async fn run(self) -> Result<()> {
        if !Self::is_available() && !self.force {
            if let Some(instructions) = upgrade_instructions_text() {
                warn!("{}", instructions);
            }
            bail!("mise is installed via a package manager, cannot update");
        }
        let status = self.do_update()?;

        if status.updated() {
            let version = status.version().to_string();
            let styled_version = style(&version).bright().yellow();
            miseprintln!("Updated mise to {styled_version}");
            #[cfg(windows)]
            if let Err(e) = Self::update_mise_shim(&version).await {
                warn!("Failed to update mise-shim.exe: {e}");
            }
        } else {
            miseprintln!("mise is already up to date");
        }
        if !self.no_plugins {
            cmd!(&*env::MISE_BIN, "plugins", "update").run()?;
        }

        Ok(())
    }

    fn do_update(&self) -> Result<Status> {
        // Use block_in_place to allow self_update's blocking HTTP calls
        // to work within mise's async runtime
        tokio::task::block_in_place(|| self.do_update_blocking())
    }

    fn do_update_blocking(&self) -> Result<Status> {
        let mut update = Update::configure();
        if let Some(token) = &*env::GITHUB_TOKEN {
            update.auth_token(token);
        }
        #[cfg(windows)]
        let bin_path_in_archive = "mise/bin/mise.exe";
        #[cfg(not(windows))]
        let bin_path_in_archive = "mise/bin/mise";
        update
            .repo_owner("jdx")
            .repo_name("mise")
            .bin_name("mise")
            .current_version(cargo_crate_version!())
            .bin_path_in_archive(bin_path_in_archive);

        let settings = Settings::try_get();
        let v = self
            .version
            .clone()
            .map_or_else(
                || -> Result<String> { Ok(update.build()?.get_latest_release()?.version) },
                Ok,
            )
            .map(|v| format!("v{v}"))?;

        // Check if already up to date (unless --force is specified)
        let current_version = format!("v{}", cargo_crate_version!());
        if !self.force && v == current_version {
            return Ok(Status::UpToDate(current_version));
        }

        let target = format!("{}-{}", *OS, *ARCH);
        #[cfg(target_env = "musl")]
        let target = format!("{target}-musl");
        // Always set target_version_tag to ensure we download the correct release
        // (fixes semver mismatch across year boundaries, e.g. 2025.x -> 2026.x)
        update.target_version_tag(&v);
        #[cfg(windows)]
        let target = format!("mise-{v}-{target}.zip");
        #[cfg(not(windows))]
        let target = format!("mise-{v}-{target}.tar.gz");
        let status = update
            .verifying_keys([*include_bytes!("../../zipsign.pub")])
            .show_download_progress(true)
            .target(&target)
            .no_confirm(settings.is_ok_and(|s| s.yes) || self.yes)
            .build()?
            .update()?;

        // Verify macOS binary signature after update
        #[cfg(target_os = "macos")]
        if status.updated() {
            Self::verify_macos_signature(&env::MISE_BIN)?;
        }

        Ok(status)
    }

    #[cfg(windows)]
    async fn update_mise_shim(version: &str) -> Result<()> {
        use crate::http::HTTP;
        use std::io::Read;

        let version = version.strip_prefix('v').unwrap_or(version);
        let archive_name = format!("mise-v{version}-{}-{}.zip", *OS, *ARCH);
        let url =
            format!("https://github.com/jdx/mise/releases/download/v{version}/{archive_name}",);
        debug!("Downloading mise-shim.exe from {url}");

        let temp_dir = tempfile::tempdir()?;
        // Use the real archive name so zipsign context matches the release signature
        let zip_path = temp_dir.path().join(&archive_name);
        HTTP.download_file(&url, &zip_path, None).await?;

        // Verify the archive signature using the same key as the main update
        Self::verify_zip_signature(&zip_path)?;

        let file = fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut shim_entry = match archive.by_name("mise/bin/mise-shim.exe") {
            Ok(entry) => entry,
            Err(_) => {
                warn!("mise-shim.exe not found in release archive, skipping");
                return Ok(());
            }
        };

        let dest = env::MISE_BIN
            .parent()
            .expect("MISE_BIN should have a parent directory")
            .join("mise-shim.exe");

        // Write to a temp file first, then rename for atomic replacement
        let mut buf = Vec::new();
        shim_entry.read_to_end(&mut buf)?;
        let temp_shim = temp_dir.path().join("mise-shim.exe");
        fs::write(&temp_shim, &buf)?;
        if fs::rename(&temp_shim, &dest).is_err() {
            // Fallback for cross-filesystem moves
            fs::copy(&temp_shim, &dest)?;
        }

        debug!("Updated mise-shim.exe at {}", dest.display());
        Ok(())
    }

    #[cfg(windows)]
    fn verify_zip_signature(path: &std::path::Path) -> Result<()> {
        let context = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.as_bytes())
            .ok_or_else(|| color_eyre::eyre::eyre!("non-UTF8 archive path"))?;

        let keys = zipsign_api::verify::collect_keys(
            [*include_bytes!("../../zipsign.pub")].into_iter().map(Ok),
        )
        .map_err(|e| color_eyre::eyre::eyre!("failed to load verification keys: {e}"))?;

        let mut file = fs::File::open(path)?;
        zipsign_api::verify::verify_zip(&mut file, &keys, Some(context))
            .map_err(|e| color_eyre::eyre::eyre!("zip signature verification failed: {e}"))?;

        debug!("Verified zip signature for {}", path.display());
        Ok(())
    }

    pub fn is_available() -> bool {
        if let Some(b) = *env::MISE_SELF_UPDATE_AVAILABLE {
            return b;
        }
        let has_disable = env::MISE_SELF_UPDATE_DISABLED_PATH.is_some();
        let has_instructions = env::MISE_SELF_UPDATE_INSTRUCTIONS.is_some();
        !(has_disable || has_instructions)
    }

    #[cfg(target_os = "macos")]
    fn verify_macos_signature(binary_path: &Path) -> Result<()> {
        use std::process::Command;

        debug!(
            "Verifying macOS code signature for: {}",
            binary_path.display()
        );

        // Check if codesign is available
        let codesign_check = Command::new("which").arg("codesign").output();

        if codesign_check.is_err() || !codesign_check.unwrap().status.success() {
            warn!("codesign command not found in PATH, skipping binary signature verification");
            warn!("This is unusual on macOS - consider verifying your system installation");
            return Ok(());
        }

        // Verify signature and identifier in one step using --test-requirement
        let output = Command::new("codesign")
            .args([
                "--verify",
                "--deep",
                "--strict",
                "-R=identifier \"dev.jdx.mise\"",
            ])
            .arg(binary_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "macOS binary signature verification failed (invalid signature or incorrect identifier): {}",
                stderr.trim()
            );
        }

        debug!("macOS binary signature verified successfully");
        Ok(())
    }
}
