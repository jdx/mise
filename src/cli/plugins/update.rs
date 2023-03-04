use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

/// Updates a plugin to the latest version
///
/// note: this updates the plugin itself, not the runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "upgrade", after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Update {
    /// Plugin(s) to update
    #[clap()]
    plugin: Option<Vec<String>>,

    /// Update all plugins
    #[clap(long, short = 'a', conflicts_with = "plugin")]
    all: bool,
}

impl Command for Update {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugins: Vec<_> = match (self.plugin, self.all) {
            (Some(plugins), _) => plugins
                .into_iter()
                .map(|p| {
                    let (p, ref_) = match p.split_once('@') {
                        Some((p, ref_)) => (p, Some(ref_.to_string())),
                        None => (p.as_str(), None),
                    };
                    let plugin = config.plugins.get(p).ok_or_else(|| {
                        eyre!("plugin {} not found", style(p).cyan().for_stderr())
                    })?;
                    Ok((plugin, ref_))
                })
                .collect::<Result<_>>()?,
            (_, true) => config
                .plugins
                .values()
                .map(|p| (p, None))
                .collect::<Vec<_>>(),
            _ => Err(eyre!("no plugins specified"))?,
        };

        for (plugin, ref_) in plugins {
            rtxprintln!(out, "updating plugin {}", plugin.name);
            plugin.update(ref_)?;
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx plugins update --all        # update all plugins
      $ rtx plugins update nodejs       # update only nodejs
      $ rtx plugins update nodejs@beta  # specify a ref
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, assert_cli_err};

    #[test]
    fn test_plugin_update() {
        assert_cli!(
            "plugin",
            "install",
            "tiny",
            "https://github.com/jdxcode/rtx-tiny.git"
        );
        let err = assert_cli_err!("p", "update");
        assert_str_eq!(err.to_string(), "no plugins specified");
        assert_cli!("plugins", "update", "tiny");
    }
}
