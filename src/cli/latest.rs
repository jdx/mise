use color_eyre::eyre::{eyre, Result};
use console::style;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::toolset::ToolVersionRequest;

/// Gets the latest available version for a plugin
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Latest {
    /// Runtime to get the latest version of
    #[clap(value_parser = RuntimeArgParser)]
    runtime: RuntimeArg,

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[clap(hide = true)]
    asdf_version: Option<String>,
}

impl Command for Latest {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let mut prefix = match self.runtime.tvr {
            None => self.asdf_version,
            Some(ToolVersionRequest::Version(_, version)) => Some(version),
            _ => Err(eyre!(
                "invalid version: {}",
                style(&self.runtime).cyan().for_stderr()
            ))?,
        };
        let plugin = config.tools.get(&self.runtime.plugin).ok_or_else(|| {
            eyre!(
                "plugin {} not found. run {} to install it",
                style(self.runtime.plugin.to_string()).cyan().for_stderr(),
                style(format!("rtx plugin install {}", self.runtime.plugin))
                    .yellow()
                    .for_stderr()
            )
        })?;
        if let Some(v) = prefix {
            prefix = Some(config.resolve_alias(&plugin.name, &v)?);
        }

        if let Some(version) = plugin.latest_version(&config.settings, prefix)? {
            rtxprintln!(out, "{}", version);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx latest nodejs@18</bold>  # get the latest version of nodejs 18
  18.0.0

  $ <bold>rtx latest nodejs</bold>     # get the latest stable version of nodejs
  20.0.0
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_display_snapshot;
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, assert_cli_err, assert_cli_snapshot};

    #[test]
    fn test_latest() {
        assert_cli_snapshot!("latest", "dummy@1");
    }

    #[test]
    fn test_latest_asdf_format() {
        assert_cli_snapshot!("latest", "dummy", "1");
    }

    #[test]
    fn test_latest_system() {
        let err = assert_cli_err!("latest", "dummy@system");
        assert_display_snapshot!(err);
    }

    #[test]
    fn test_latest_missing_plugin() {
        let stdout = assert_cli_err!("latest", "invalid_plugin");
        assert_display_snapshot!(stdout);
    }

    #[test]
    fn test_latest_alias() {
        let stdout = assert_cli!("latest", "tiny@lts");
        assert_str_eq!(stdout, "3.1.0\n");
    }
}
