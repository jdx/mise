use color_eyre::Result;
use console::style;
use self_update::cargo_crate_version;

use crate::cli::command::Command;
use crate::cli::version::{ARCH, OS};
use crate::config::Config;
use crate::output::Output;

/// updates rtx itself
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SelfUpdate {}

impl Command for SelfUpdate {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let status = self_update::backends::github::Update::configure()
            .repo_owner("jdxcode")
            .repo_name("rtx")
            .bin_name("rtx")
            .show_download_progress(true)
            .current_version(cargo_crate_version!())
            .target(&format!("{}-{}", *OS, *ARCH))
            .build()?
            .update()?;
        let version = style(status.version()).bright().yellow();
        rtxprintln!(out, "Updated rtx to {version}");

        Ok(())
    }
}
