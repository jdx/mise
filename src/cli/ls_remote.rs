use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::cli::args::tool::ToolArg;
use crate::cli::args::tool::ToolArgParser;
use crate::config::Config;
use crate::output::Output;
use crate::tool::Tool;
use crate::toolset::ToolVersionRequest;

/// List runtime versions available for install
///
/// note that the results are cached for 24 hours
/// run `rtx cache clean` to clear the cache and get fresh results
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP, aliases = ["list-all", "list-remote"])]
pub struct LsRemote {
    /// Plugin to get versions for
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser, required_unless_present = "all")]
    plugin: Option<ToolArg>,

    /// Show all installed plugins and versions
    #[clap(long, verbatim_doc_comment, conflicts_with_all = ["plugin", "prefix"])]
    all: bool,

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    #[clap(verbatim_doc_comment)]
    prefix: Option<String>,
}

impl LsRemote {
    pub fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        if let Some(plugin) = self.get_plugin(&mut config)? {
            self.run_single(config, out, plugin)
        } else {
            self.run_all(config, out)
        }
    }

    fn run_single(self, config: Config, out: &mut Output, plugin: Arc<Tool>) -> Result<()> {
        let prefix = match &self.plugin {
            Some(tool_arg) => match &tool_arg.tvr {
                Some(ToolVersionRequest::Version(_, v)) => Some(v.clone()),
                _ => self.prefix.clone(),
            },
            _ => self.prefix.clone(),
        };

        let versions = plugin.list_remote_versions(&config.settings)?;
        let versions = match prefix {
            Some(prefix) => versions
                .into_iter()
                .filter(|v| v.starts_with(&prefix))
                .collect(),
            None => versions,
        };

        for version in versions {
            rtxprintln!(out, "{}", version);
        }

        Ok(())
    }

    fn run_all(self, config: Config, out: &mut Output) -> Result<()> {
        for plugin in config.tools.values() {
            let versions = plugin.list_remote_versions(&config.settings)?;
            for version in versions {
                rtxprintln!(out, "{}@{}", plugin.name, version);
            }
        }
        Ok(())
    }

    fn get_plugin(&self, config: &mut Config) -> Result<Option<Arc<Tool>>> {
        match &self.plugin {
            Some(tool_arg) => {
                let tool = config.get_or_create_tool(&tool_arg.plugin);
                tool.ensure_installed(config, None, false)?;
                Ok(Some(tool))
            }
            None => Ok(None),
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx ls-remote node</bold>
  18.0.0
  20.0.0

  $ <bold>rtx ls-remote node@20</bold>
  20.0.0
  20.1.0

  $ <bold>rtx ls-remote node 20</bold>
  20.0.0
  20.1.0
"#
);

#[cfg(test)]
mod tests {
    use crate::assert_cli_snapshot;

    #[test]
    fn test_list_remote() {
        assert_cli_snapshot!("list-remote", "dummy");
    }

    #[test]
    fn test_ls_remote_prefix() {
        assert_cli_snapshot!("list-remote", "dummy", "1");
        assert_cli_snapshot!("list-remote", "dummy@2");
    }
}
