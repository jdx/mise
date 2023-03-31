use color_eyre::eyre::{eyre, Result};
use console::style;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

/// Updates a plugin to the latest version
///
/// note: this updates the plugin itself, not the runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "upgrade", after_long_help = AFTER_LONG_HELP)]
pub struct Update {
    /// Plugin(s) to update
    #[clap()]
    plugin: Option<Vec<String>>,

    /// Update all plugins
    #[clap(long, short = 'a', conflicts_with = "plugin", hide = true)]
    all: bool,
}

impl Command for Update {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugins: Vec<_> = match self.plugin {
            Some(plugins) => plugins
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
            None => config
                .plugins
                .values()
                .map(|p| (p, None))
                .collect::<Vec<_>>(),
        };

        for (plugin, ref_) in plugins {
            rtxprintln!(out, "updating plugin {}", plugin.name);
            plugin.update(ref_)?;
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx plugins update</bold>              # update all plugins
  $ <bold>rtx plugins update nodejs</bold>       # update only nodejs
  $ <bold>rtx plugins update nodejs@beta</bold>  # specify a ref
"#
);

#[cfg(test)]
mod tests {
    use crate::assert_cli;

    #[test]
    fn test_plugin_update() {
        assert_cli!(
            "plugin",
            "install",
            "tiny",
            "https://github.com/jdxcode/rtx-tiny.git"
        );
        // assert_cli!("p", "update"); tested in e2e
        assert_cli!("plugins", "update", "tiny");
    }
}
