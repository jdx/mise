use color_eyre::eyre::{bail, Result};

use crate::backend::ABackend;
use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::toolset::ToolRequest;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Gets the latest available version for a plugin
///
/// Supports prefixes such as `node@20` to get the latest version of node 20.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Latest {
    /// Tool to get the latest version of
    #[clap(value_name = "TOOL@VERSION")]
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
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let mut prefix = match self.tool.tvr {
            None => self.asdf_version,
            Some(ToolRequest::Version { version, .. }) => Some(version),
            _ => bail!("invalid version: {}", self.tool.style()),
        };

        let backend: ABackend = self.tool.backend.into();
        let mpr = MultiProgressReport::get();
        if let Some(plugin) = backend.plugin() {
            plugin.ensure_installed(&mpr, false)?;
        }
        if let Some(v) = prefix {
            prefix = Some(config.resolve_alias(&backend, &v)?);
        }

        let latest_version = if self.installed {
            backend.latest_installed_version(prefix)?
        } else {
            backend.latest_version(prefix)?
        };
        if let Some(version) = latest_version {
            miseprintln!("{}", version);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise latest node@20</bold>  # get the latest version of node 20
    20.0.0

    $ <bold>mise latest node</bold>     # get the latest stable version of node
    20.0.0
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use pretty_assertions::assert_str_eq;
    use test_log::test;

    use crate::test::reset;

    #[test]
    fn test_latest() {
        reset();
        assert_cli_snapshot!("latest", "dummy@1");
    }

    #[test]
    fn test_latest_asdf_format() {
        reset();
        assert_cli_snapshot!("latest", "dummy", "1");
    }

    #[test]
    fn test_latest_system() {
        reset();
        let err = assert_cli_err!("latest", "dummy@system");
        assert_snapshot!(err);
    }

    #[test]
    fn test_latest_installed() {
        reset();
        assert_cli_snapshot!("latest", "dummy");
    }

    #[test]
    fn test_latest_missing_plugin() {
        reset();
        let stdout = assert_cli_err!("latest", "invalid_plugin");
        assert_snapshot!(stdout);
    }

    #[test]
    fn test_latest_alias() {
        reset();
        let stdout = assert_cli!("latest", "tiny@lts");
        assert_str_eq!(stdout, "3.1.0");
    }
}
