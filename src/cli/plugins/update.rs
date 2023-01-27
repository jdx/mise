use std::sync::Arc;

use color_eyre::eyre::{eyre, Result};
use owo_colors::{OwoColorize, Stream};

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::Plugin;

/// updates a plugin to the latest version
///
/// note: this updates the plugin itself, not the runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "upgrade", after_long_help = AFTER_LONG_HELP)]
pub struct PluginsUpdate {
    /// plugin(s) to update
    #[clap()]
    plugin: Option<Vec<String>>,
}

impl Command for PluginsUpdate {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugins: Vec<Arc<Plugin>> = match self.plugin {
            Some(plugins) => plugins
                .into_iter()
                .map(|p| {
                    config.ts.find_plugin(&p).ok_or_else(|| {
                        eyre!(
                            "plugin {} not found",
                            p.if_supports_color(Stream::Stderr, |t| t.cyan())
                        )
                    })
                })
                .collect::<Result<Vec<Arc<Plugin>>>>()?,
            None => config.ts.list_installed_plugins(),
        };

        for plugin in plugins {
            rtxprintln!(out, "updating plugin {}", plugin.name);
            plugin.update(None)?;
        }
        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  rtx plugins update         # update all plugins
  rtx plugins update nodejs  # update only nodejs
"#;

#[cfg(test)]
mod test {
    use crate::assert_cli;
    use crate::cli::test::ensure_plugin_installed;

    #[test]
    fn test_plugin_update() {
        ensure_plugin_installed("nodejs");
        assert_cli!("plugin", "update");
        assert_cli!("plugin", "update", "nodejs");
    }
}
