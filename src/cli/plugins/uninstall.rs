use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

/// Removes a plugin
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP.as_str())]
pub struct PluginsUninstall {
    /// Plugin to remove
    #[clap()]
    plugin: String,
}

impl Command for PluginsUninstall {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugin = config.plugins.get(&self.plugin);
        match plugin {
            Some(plugin) if plugin.is_installed() => {
                rtxprintln!(out, "uninstalling plugin: {}", style(&self.plugin).cyan());
                plugin.uninstall()?;
            }
            _ => {
                warn!(
                    "{} is not installed",
                    style(&self.plugin).cyan().for_stderr()
                );
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx uninstall nodejs
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_plugin_uninstall() {
        assert_cli!(
            "plugin",
            "add",
            "tiny",
            "https://github.com/jdxcode/rtx-tiny"
        );
        assert_cli_snapshot!("plugin", "rm", "tiny");
        assert_cli_snapshot!("plugin", "rm", "tiny");
        assert_cli!(
            "plugin",
            "add",
            "tiny",
            "https://github.com/jdxcode/rtx-tiny"
        );
    }
}
