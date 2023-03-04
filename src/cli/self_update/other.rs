use crate::cli::version::VERSION;
use color_eyre::eyre::eyre;
use color_eyre::Result;
use console::style;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::{cmd, env};

/// Updates rtx itself
/// Uses whatever package manager was used to install rtx or just downloads
/// a binary from GitHub Releases if rtx was installed manually.
/// Supports: standalone, brew, deb, rpm
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct SelfUpdate {}

impl Command for SelfUpdate {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let cmd = if cfg!(feature = "brew") {
            "brew upgrade rtx"
        } else if cfg!(feature = "deb") {
            "sudo apt update && sudo apt install rtx"
        } else if cfg!(feature = "rpm") {
            "sudo dnf upgrade rtx"
        } else {
            return Err(eyre!("Self-update is not supported"));
        };
        rtxprintln!(out, "Running `{}`", style(&cmd).yellow());
        cmd!(&*env::SHELL, "-c", cmd).run()?;

        Ok(())
    }
}

pub static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    indoc::formatdoc! {r#"
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

#[cfg(test)]
mod tests {
    use insta::assert_display_snapshot;

    use crate::assert_cli_err;

    use super::*;

    #[test]
    fn test_self_update() -> Result<()> {
        let err = assert_cli_err!("self-update");
        assert_display_snapshot!(err);

        Ok(())
    }
}
