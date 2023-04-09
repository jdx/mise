use color_eyre::eyre::Result;
use std::sync::Arc;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::tool::Tool;
use crate::toolset::ToolVersionRequest;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt;

/// List runtime versions available for install
///
/// note that the results are cached for 24 hours
/// run `rtx cache clean` to clear the cache and get fresh results
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP, aliases = ["list-all", "list-remote"])]
pub struct LsRemote {
    /// Plugin to get versions for
    #[clap(value_parser = RuntimeArgParser)]
    plugin: RuntimeArg,

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    #[clap(verbatim_doc_comment)]
    prefix: Option<String>,
}

impl Command for LsRemote {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let plugin = self.get_plugin(&mut config)?;

        let prefix = match &self.plugin.tvr {
            Some(ToolVersionRequest::Version(_, v)) => Some(v),
            _ => self.prefix.as_ref(),
        };

        let versions = plugin.list_remote_versions(&config.settings)?.clone();
        let versions = match prefix {
            Some(prefix) => versions
                .into_iter()
                .filter(|v| v.starts_with(prefix))
                .collect(),
            None => versions,
        };

        for version in versions {
            rtxprintln!(out, "{}", version);
        }

        Ok(())
    }
}

impl LsRemote {
    fn get_plugin(&self, config: &mut Config) -> Result<Arc<Tool>> {
        let plugin_name = self.plugin.plugin.clone();
        let tool = config.get_or_create_tool(&plugin_name);
        self.ensure_remote_plugin_is_installed(&tool, config)?;
        Ok(tool)
    }

    fn ensure_remote_plugin_is_installed(&self, tool: &Tool, config: &mut Config) -> Result<()> {
        if tool.is_installed() {
            return Ok(());
        }
        if prompt::confirm(&format!(
            "Plugin {} is not installed, would you like to install it?",
            tool.name
        ))? {
            let mpr = MultiProgressReport::new(config.settings.verbose);
            let mut pr = mpr.add();
            tool.install(config, &mut pr, false)?;
            return Ok(());
        }

        Err(PluginNotInstalled(tool.name.clone()))?
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx ls-remote nodejs</bold>
  18.0.0
  20.0.0

  $ <bold>rtx ls-remote nodejs@18</bold>
  18.0.0
  18.1.0

  $ <bold>rtx ls-remote nodejs 18</bold>
  18.0.0
  18.1.0
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
