use std::collections::HashSet;

use color_eyre::eyre::Result;
use console::{measure_text_width, pad_str, Alignment};
use itertools::Itertools;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

/// List all available remote plugins
///
/// These are fetched from https://github.com/asdf-vm/asdf-plugins
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list-remote", long_about = LONG_ABOUT, verbatim_doc_comment, alias = "list-all")]
pub struct PluginsLsRemote {
    /// show the git url for each plugin
    ///
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(short, long)]
    pub urls: bool,
}

impl Command for PluginsLsRemote {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let installed_plugins = config
            .plugins
            .values()
            .filter(|p| p.is_installed())
            .map(|p| p.name.clone())
            .collect::<HashSet<_>>();

        let shorthands = config.get_shorthands().iter().sorted().collect_vec();
        let max_plugin_len = shorthands
            .iter()
            .map(|(plugin, _)| measure_text_width(plugin))
            .max()
            .unwrap_or(0);

        if shorthands.is_empty() {
            warn!("default shorthands are disabled");
        }

        for (plugin, repo) in shorthands {
            let installed = if installed_plugins.contains(plugin) {
                "*"
            } else {
                " "
            };
            let url = if self.urls { repo } else { "" };
            let plugin = pad_str(plugin, max_plugin_len, Alignment::Left, None);
            rtxprintln!(out, "{} {}{}", plugin, installed, url);
        }

        Ok(())
    }
}

const LONG_ABOUT: &str = r#"
List all available remote plugins

These are fetched from https://github.com/asdf-vm/asdf-plugins

Examples:
  $ rtx plugins ls-remote
"#;

#[cfg(test)]
mod tests {
    use crate::assert_cli;

    #[test]
    fn test_plugin_list_remote() {
        let stdout = assert_cli!("plugin", "ls-remote");
        assert!(stdout.contains("nodejs"));
    }
}
