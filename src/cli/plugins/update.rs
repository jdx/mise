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
pub struct Update {
    /// plugin(s) to update
    #[clap()]
    plugin: Option<Vec<String>>,

    /// update all plugins
    #[clap(long, short = 'a', conflicts_with = "plugin")]
    all: bool,
}

impl Command for Update {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugins: Vec<Arc<Plugin>> = match (self.plugin, self.all) {
            (Some(plugins), _) => plugins
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
            (_, true) => config.ts.list_installed_plugins(),
            _ => Err(eyre!("no plugins specified"))?,
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
  rtx plugins update --all   # update all plugins
  rtx plugins update nodejs  # update only nodejs
"#;

#[cfg(test)]
mod test {
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, assert_cli_err};

    #[test]
    fn test_plugin_update() {
        let err = assert_cli_err!("p", "update");
        assert_str_eq!(err.to_string(), "no plugins specified");
        assert_cli!("plugin", "update", "--all");
        assert_cli!("plugins", "update", "nodejs");
    }
}
