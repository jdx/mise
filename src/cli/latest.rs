use color_eyre::eyre::{eyre, Result};
use console::style;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::config::Config;
use crate::output::Output;
use crate::toolset::ToolVersionRequest;

/// Gets the latest available version for a plugin
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Latest {
    /// Tool to get the latest version of
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser)]
    tool: ToolArg,

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[clap(hide = true)]
    asdf_version: Option<String>,

    /// Show latest installed instead of available version
    #[clap(short, long)]
    installed: bool,
}

impl Latest {
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let mut prefix = match self.tool.tvr {
            None => self.asdf_version,
            Some(ToolVersionRequest::Version(_, version)) => Some(version),
            _ => Err(eyre!(
                "invalid version: {}",
                style(&self.tool).cyan().for_stderr()
            ))?,
        };
        let plugin = config.tools.get(&self.tool.plugin).ok_or_else(|| {
            eyre!(
                "plugin {} not found. run {} to install it",
                style(self.tool.plugin.to_string()).cyan().for_stderr(),
                style(format!("rtx plugin install {}", self.tool.plugin))
                    .yellow()
                    .for_stderr()
            )
        })?;
        if let Some(v) = prefix {
            prefix = Some(config.resolve_alias(&plugin.name, &v)?);
        }

        let latest_version = if self.installed {
            plugin.latest_installed_version(prefix)?
        } else {
            plugin.latest_version(&config.settings, prefix)?
        };
        if let Some(version) = latest_version {
            rtxprintln!(out, "{}", version);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx latest node@20</bold>  # get the latest version of node 20
  20.0.0

  $ <bold>rtx latest node</bold>     # get the latest stable version of node
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
    fn test_latest_installed() {
        assert_cli_snapshot!("latest", "dummy");
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
