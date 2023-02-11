use color_eyre::eyre::Result;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use owo_colors::{OwoColorize, Stream};

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::ui::color::Color;

/// removes a plugin
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP.as_str())]
pub struct PluginsUninstall {
    /// plugin to remove
    #[clap()]
    plugin: String,
}

impl Command for PluginsUninstall {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugin = config.ts.find_plugin(&self.plugin);
        match plugin {
            Some(plugin) if plugin.is_installed() => {
                rtxprintln!(
                    out,
                    "uninstalling plugin: {}",
                    self.plugin.if_supports_color(Stream::Stderr, |t| t.cyan())
                );
                plugin.uninstall()?;
            }
            _ => {
                warn!(
                    "{} is not installed",
                    self.plugin.if_supports_color(Stream::Stderr, |t| t.cyan())
                );
            }
        }
        Ok(())
    }
}

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx uninstall nodejs
    "#, COLOR.header("Examples:")}
});

#[cfg(test)]
mod test {
    use insta::assert_snapshot;

    use crate::assert_cli;
    use crate::cli::test::ensure_plugin_installed;

    #[test]
    fn test_plugin_uninstall() {
        ensure_plugin_installed("nodejs");

        let stdout = assert_cli!("plugin", "rm", "nodejs");
        assert_snapshot!(stdout);

        let stdout = assert_cli!("plugin", "rm", "nodejs");
        assert_snapshot!(stdout);

        ensure_plugin_installed("nodejs");
    }
}
