use color_eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use self_update::backends::github::Update;
use self_update::cargo_crate_version;

use crate::cli::command::Command;
use crate::cli::version::{ARCH, OS, VERSION};
use crate::config::Config;
use crate::env;
use crate::output::Output;

/// Updates rtx itself
/// Uses whatever package manager was used to install rtx or just downloads
/// a binary from GitHub Releases if rtx was installed manually.
/// Supports: standalone, brew, deb, rpm
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
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

pub static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx self-update
      Checking target-arch... macos-arm64
      Checking current version... v1.0.0
      Checking latest released version... v{version}
      New release found! v1.0.0 --> v{version}
      New release is compatible

      rtx release status:
        * Current exe: "/Users/jdx/bin/rtx"
        * New exe release: "rtx-v{version}-macos-arm64"

      The new release will be downloaded/extracted and the existing binary will be replaced.
      Do you want to continue? [Y/n] y
      Downloading...
      Extracting archive... Done
      Replacing binary file... Done
      Updated rtx to {version}
    "#, style("Examples:").bold().underlined(), version=*VERSION}
});
