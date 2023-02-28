use color_eyre::eyre::Result;
use serde_derive::Deserialize;

use crate::cli::command::Command;
use crate::cmd;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::{Prompt, Warn};
use crate::output::Output;
use crate::toolset::ToolsetBuilder;

/// [internal] This is an internal command that writes an envrc file
/// for direnv to consume.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true)]
pub struct DirenvExec {}

#[derive(Debug, Default, Deserialize)]
struct DirenvWatches {
    #[serde(rename(deserialize = "DIRENV_WATCHES"))]
    watches: String,
}

impl Command for DirenvExec {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        if config.settings.missing_runtime_behavior == Prompt {
            config.settings.missing_runtime_behavior = Warn;
        }
        let ts = ToolsetBuilder::new().with_install_missing().build(&config);
        let mut cmd = if cfg!(test) {
            cmd!("env")
        } else {
            cmd!("direnv", "dump")
        };

        for (k, v) in ts.env() {
            cmd = cmd.env(k, v);
        }
        cmd = cmd.env("PATH", ts.path_env(&config.settings));

        let json = cmd!("direnv", "watch", "json", ".tool-versions").read()?;
        let w: DirenvWatches = serde_json::from_str(&json)?;
        cmd = cmd.env("DIRENV_WATCHES", w.watches);

        rtxprint!(out, "{}", cmd.read()?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::assert_cli;
    use crate::cli::tests::grep;
    use pretty_assertions::assert_str_eq;

    #[test]
    fn test_direnv_exec() {
        let stdout = assert_cli!("direnv", "exec");
        assert_str_eq!(grep(stdout, "JDXCODE_TINY="), "JDXCODE_TINY=3.1.0");
    }
}
