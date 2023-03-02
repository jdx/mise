use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser, RuntimeArgVersion};
use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;

/// List runtime versions available for install
///
/// note that these versions are cached for commands like `rtx install nodejs@latest`
/// however _this_ command will always clear that cache and fetch the latest remote versions
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list-remote", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str(), alias = "list-all")]
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
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugin = config
            .plugins
            .get(&self.plugin.plugin)
            .ok_or(PluginNotInstalled(self.plugin.plugin))?;
        plugin.clear_remote_version_cache()?;

        let prefix = match self.plugin.version {
            RuntimeArgVersion::Version(v) => Some(v),
            _ => self.prefix,
        };

        let versions = plugin.list_remote_versions(&config.settings)?.clone();
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
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx ls-remote nodejs
      18.0.0
      20.0.0

      $ rtx ls-remote nodejs@18
      18.0.0
      18.1.0

      $ rtx ls-remote nodejs 18
      18.0.0
      18.1.0
    "#, style("Examples:").bold().underlined()}
});

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
