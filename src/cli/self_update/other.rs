use color_eyre::eyre::eyre;
use color_eyre::Result;
use console::style;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::{cmd, env};

/// Updates rtx itself
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
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
        rtxprintln!(out, "running `{}`", style(&cmd).yellow());
        cmd!(&*env::SHELL, "-c", cmd).run()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::assert_cli_err;
    use insta::assert_display_snapshot;

    use super::*;

    #[test]
    fn test_self_update() -> Result<()> {
        let err = assert_cli_err!("self-update");
        assert_display_snapshot!(err);

        Ok(())
    }
}
