use color_eyre::Result;
use console::style;

use self_update::backends::github::Update;
use self_update::cargo_crate_version;

use crate::cli::command::Command;
use crate::cli::version::{ARCH, OS};
use crate::config::Config;
use crate::env;
use crate::output::Output;

/// Updates rtx itself
/// Uses whatever package manager was used to install rtx or just downloads
/// a binary from GitHub Releases if rtx was installed manually.
/// Supports: standalone, brew, deb, rpm
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SelfUpdate {}

impl Command for SelfUpdate {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let current_version =
            env::var("RTX_SELF_UPDATE_VERSION").unwrap_or(cargo_crate_version!().to_string());
        let mut update = Update::configure();
        update
            .repo_owner("jdxcode")
            .repo_name("rtx")
            .bin_name("rtx")
            .show_download_progress(true)
            .current_version(&current_version)
            .target(&format!("{}-{}", *OS, *ARCH))
            .identifier("rtx-v");
        if let Some(token) = &*env::GITHUB_API_TOKEN {
            update.auth_token(token);
        }
        let status = update.build()?.update()?;
        if status.updated() {
            let version = style(status.version()).bright().yellow();
            rtxprintln!(out, "Updated rtx to {version}");
        } else {
            rtxprintln!(out, "rtx is already up to date");
        }

        Ok(())
    }
}
