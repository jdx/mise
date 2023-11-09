use color_eyre::Result;
use console::style;

use self_update::backends::github::{ReleaseList, Update};
use self_update::update::Release;
use self_update::{cargo_crate_version, Status};

use crate::cli::command::Command;
use crate::cli::version::{ARCH, OS};
use crate::config::Config;
use crate::env;
use crate::output::Output;

/// Updates rtx itself
///
/// Uses whatever package manager was used to install rtx or just downloads
/// a binary from GitHub Releases if rtx was installed manually.
/// Supports: standalone, brew, deb, rpm
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SelfUpdate {}

impl Command for SelfUpdate {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let latest = &self.fetch_releases()?[0].version;
        let status = self.do_update(&config, latest)?;

        if status.updated() {
            let version = style(status.version()).bright().yellow();
            rtxprintln!(out, "Updated rtx to {version}");
        } else {
            rtxprintln!(out, "rtx is already up to date");
        }

        Ok(())
    }
}

impl SelfUpdate {
    fn fetch_releases(&self) -> Result<Vec<Release>> {
        let mut releases = ReleaseList::configure();
        if let Some(token) = &*env::GITHUB_API_TOKEN {
            releases.auth_token(token);
        }
        let releases = releases
            .repo_owner("jdx")
            .repo_name("rtx")
            .build()?
            .fetch()?;
        Ok(releases)
    }

    fn do_update(&self, config: &Config, latest: &str) -> Result<Status> {
        let current_version =
            env::var("RTX_SELF_UPDATE_VERSION").unwrap_or(cargo_crate_version!().to_string());
        let target = format!("{}-{}", *OS, *ARCH);
        let mut update = Update::configure();
        if let Some(token) = &*env::GITHUB_API_TOKEN {
            update.auth_token(token);
        }
        let status = update
            .repo_owner("jdx")
            .repo_name("rtx")
            .bin_name("rtx")
            .verifying_keys([*include_bytes!("../../zipsign.pub")])
            .show_download_progress(true)
            .current_version(&current_version)
            .target(&target)
            .bin_path_in_archive("rtx/bin/rtx")
            .identifier(&format!("rtx-v{latest}-{target}.tar.gz"))
            .no_confirm(config.settings.yes)
            .build()?
            .update()?;
        Ok(status)
    }
}
