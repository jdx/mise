use color_eyre::eyre::Result;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::Plugin;

/// get the latest runtime version of a plugin's runtimes
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Latest {
    /// Runtime to get the latest version of
    #[clap(value_parser = RuntimeArgParser)]
    runtime: RuntimeArg,

    /// the version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[clap(hide = true)]
    asdf_version: Option<String>,
}

impl Command for Latest {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let prefix = match self.runtime.version.as_str() {
            "latest" => match self.asdf_version {
                Some(version) => version,
                None => "latest".to_string(),
            },
            v => v.into(),
        };
        let plugin = Plugin::load_ensure_installed(&self.runtime.plugin, &config.settings)?;

        if let Some(version) = plugin.latest_version(&prefix)? {
            rtxprintln!(out, "{}", version);
        }
        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ rtx latest nodejs@18  # get the latest version of nodejs 18
  18.0.0
  
  $ rtx latest nodejs     # get the latest stable version of nodejs
  20.0.0
"#;

#[cfg(test)]
mod test {
    use insta::assert_display_snapshot;

    use crate::assert_cli;

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
}
