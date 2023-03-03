use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;

use crate::output::Output;
use crate::toolset::ToolsetBuilder;

/// Shows the path that a bin name points to
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Which {
    #[clap()]
    pub bin_name: String,

    /// Show the plugin name instead of the path
    #[clap(long, conflicts_with = "version")]
    pub plugin: bool,

    /// Show the version instead of the path
    #[clap(long, conflicts_with = "plugin")]
    pub version: bool,
}

impl Command for Which {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&mut config);

        match ts.which(&config, &self.bin_name) {
            Some(rtv) => {
                if self.version {
                    rtxprintln!(out, "{}", rtv.version);
                } else if self.plugin {
                    rtxprintln!(out, "{}", rtv.plugin.name);
                } else {
                    let path = rtv.which(&config.settings, &self.bin_name)?;
                    rtxprintln!(out, "{}", path.unwrap().display());
                }
                Ok(())
            }
            None => Err(eyre!("{} not found", self.bin_name)),
        }
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx which node
      /home/username/.local/share/rtx/installs/nodejs/18.0.0/bin/node
      $ rtx which node --plugin
      nodejs
      $ rtx which node --version
      18.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_which() {
        assert_cli!("global", "dummy@1.0.0");
        assert_cli_snapshot!("which", "dummy");
        assert_cli!("global", "dummy@ref:master");
        assert_cli!("uninstall", "dummy@1.0.0");
    }

    #[test]
    fn test_which_plugin() {
        assert_cli!("global", "dummy@1.0.0");
        assert_cli_snapshot!("which", "--plugin", "dummy");
        assert_cli!("global", "dummy@ref:master");
        assert_cli!("uninstall", "dummy@1.0.0");
    }

    #[test]
    fn test_which_version() {
        assert_cli!("global", "dummy@1.0.0");
        assert_cli_snapshot!("which", "--version", "dummy");
        assert_cli!("global", "dummy@ref:master");
        assert_cli!("uninstall", "dummy@1.0.0");
    }
}
