use atty::Stream;
use color_eyre::eyre::Result;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::ui::color::Color;

/// list runtime versions available for install
///
/// note that these versions are cached for commands like `rtx install nodejs@latest`
/// however _this_ command will always clear that cache and fetch the latest remote versions
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list-remote", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct LsRemote {
    /// Plugin
    #[clap()]
    plugin: String,
}

impl Command for LsRemote {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugin = config
            .ts
            .find_plugin(&self.plugin)
            .ok_or(PluginNotInstalled(self.plugin))?;
        let versions = plugin.list_remote_versions()?;

        for version in versions {
            rtxprintln!(out, "{}", version);
        }

        Ok(())
    }
}

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx list-remote nodejs
      18.0.0
      20.0.0
    "#, COLOR.header("Examples:")}
});

#[cfg(test)]
mod test {
    use crate::assert_cli;
    use crate::cli::test::ensure_plugin_installed;

    #[test]
    fn test_list_remote() {
        ensure_plugin_installed("nodejs");
        let stdout = assert_cli!("list-remote", "nodejs");
        assert!(stdout.contains("18.0.0"));
    }
}
