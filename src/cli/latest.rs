use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser, RuntimeArgVersion};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

/// Get the latest runtime version of a plugin's runtimes
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
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
        let prefix = match self.runtime.version {
            RuntimeArgVersion::None => match self.asdf_version {
                Some(version) => version,
                None => "latest".to_string(),
            },
            RuntimeArgVersion::Version(version) => version,
            _ => Err(eyre!(
                "invalid version: {}",
                style(&self.runtime).cyan().for_stderr()
            ))?,
        };
        let plugin = config.plugins.get(&self.runtime.plugin).ok_or_else(|| {
            eyre!(
                "plugin {} not found. run {} to install it",
                style(self.runtime.plugin.to_string()).cyan().for_stderr(),
                style(format!("rtx plugin install {}", self.runtime.plugin))
                    .yellow()
                    .for_stderr()
            )
        })?;

        plugin.clear_remote_version_cache()?;
        if let Some(version) = plugin.latest_version(&config.settings, &prefix)? {
            rtxprintln!(out, "{}", version);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx latest nodejs@18  # get the latest version of nodejs 18
      18.0.0

      $ rtx latest nodejs     # get the latest stable version of nodejs
      20.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use insta::assert_display_snapshot;

    use crate::{assert_cli, assert_cli_err};

    #[test]
    fn test_latest() {
        assert_cli!("plugins", "install", "nodejs");
        let stdout = assert_cli!("latest", "nodejs@12");
        assert_display_snapshot!(stdout);
    }

    #[test]
    fn test_latest_ruby() {
        assert_cli!("plugins", "install", "ruby");
        let stdout = assert_cli!("latest", "ruby");
        assert!(stdout.starts_with("3."));
    }

    #[test]
    fn test_latest_asdf_format() {
        let stdout = assert_cli!("latest", "nodejs", "12");
        assert_display_snapshot!(stdout);
    }

    #[test]
    fn test_latest_system() {
        let stdout = assert_cli_err!("latest", "nodejs@system");
        assert_display_snapshot!(stdout);
    }

    #[test]
    fn test_latest_missing_plugin() {
        let stdout = assert_cli_err!("latest", "invalid_plugin");
        assert_display_snapshot!(stdout);
    }
}
