use color_eyre::Result;
use color_eyre::eyre::bail;
use console::style;
use self_update::backends::github::Update;
use self_update::{Status, cargo_crate_version};

use crate::cli::version::{ARCH, OS};
use crate::config::Settings;
use crate::{cmd, env};
use std::collections::BTreeMap;
use std::fs;
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
    if let Some(path) = &*env::MISE_SELF_UPDATE_INSTRUCTIONS {
        if let Some(msg) = read_instructions_file(path) {
            return Some(msg);
        }
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
    /// Update even if already up to date
    #[clap(long, short)]
    force: bool,

    /// Disable auto-updating plugins
    #[clap(long)]
    no_plugins: bool,

    /// Skip confirmation prompt
    #[clap(long, short)]
    yes: bool,

    /// Update to a specific version
    version: Option<String>,
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
            let version = style(status.version()).bright().yellow();
            miseprintln!("Updated mise to {version}");
        } else {
            miseprintln!("mise is already up to date");
        }
        if !self.no_plugins {
            cmd!(&*env::MISE_BIN, "plugins", "update").run()?;
        }

        Ok(())
    }

    fn do_update(&self) -> Result<Status> {
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
        let target = format!("{}-{}", *OS, *ARCH);
        #[cfg(target_env = "musl")]
        let target = format!("{target}-musl");
        if self.force || self.version.is_some() {
            update.target_version_tag(&v);
        }
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
        Ok(status)
    }

    pub fn is_available() -> bool {
        if let Some(b) = *env::MISE_SELF_UPDATE_AVAILABLE {
            return b;
        }
        let has_disable = env::MISE_SELF_UPDATE_DISABLED_PATH.is_some();
        let has_instructions = env::MISE_SELF_UPDATE_INSTRUCTIONS.is_some();
        !(has_disable || has_instructions)
    }
}
