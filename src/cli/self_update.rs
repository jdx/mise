use clap::Args;
use color_eyre::Result;
use console::style;
use self_update::backends::github::{ReleaseList, Update};
use self_update::update::Release;
use self_update::{cargo_crate_version, Status};

use crate::cli::version::{ARCH, OS};
use crate::config::Config;

use crate::{cmd, env};

/// Updates rtx itself
///
/// Uses the GitHub Releases API to find the latest release and binary
/// By default, this will also update any installed plugins
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
    pub fn run(self, _config: Config) -> Result<()> {
        let status = self.do_update()?;

        if status.updated() {
            let version = style(status.version()).bright().yellow();
            rtxprintln!("Updated rtx to {version}");
        } else {
            rtxprintln!("rtx is already up to date");
        }
        if !self.no_plugins {
            cmd!(&*env::RTX_EXE, "plugins", "update").run()?;
        }

        Ok(())
    }

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

    fn latest_version(&self) -> Result<String> {
        let releases = self.fetch_releases()?;
        Ok(releases[0].version.clone())
    }

    fn do_update(&self) -> Result<Status> {
        let v = self
            .version
            .clone()
            .map_or_else(|| self.latest_version(), Ok)
            .map(|v| format!("v{}", v))?;
        let target = format!("{}-{}", *OS, *ARCH);
        let mut update = Update::configure();
        if let Some(token) = &*env::GITHUB_API_TOKEN {
            update.auth_token(token);
        }
        if self.force || self.version.is_some() {
            update.target_version_tag(&v);
        }
        let status = update
            .repo_owner("jdx")
            .repo_name("rtx")
            .bin_name("rtx")
            .verifying_keys([*include_bytes!("../../zipsign.pub")])
            .show_download_progress(true)
            .current_version(cargo_crate_version!())
            .target(&target)
            .bin_path_in_archive("rtx/bin/rtx")
            .identifier(&format!("rtx-{v}-{target}.tar.gz"))
            .no_confirm(self.yes)
            .build()?
            .update()?;
        Ok(status)
    }

    pub fn is_available() -> bool {
        !std::fs::canonicalize(&*env::RTX_EXE)
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .map(|p| p.join("lib").join(".disable-self-update").exists())
            .unwrap_or_default()
    }

    pub fn command() -> clap::Command {
        Self::augment_args(clap::Command::new("self-update"))
    }
}
