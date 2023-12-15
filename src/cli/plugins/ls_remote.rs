use std::collections::HashSet;

use color_eyre::eyre::Result;
use console::{measure_text_width, pad_str, Alignment};
use itertools::Itertools;

use crate::config::Config;

/// List all available remote plugins
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["list-remote", "list-all"], long_about = LONG_ABOUT, verbatim_doc_comment)]
pub struct PluginsLsRemote {
    /// Show the git url for each plugin
    /// e.g.: https://github.com/rtx-plugins/rtx-nodejs.git
    #[clap(short, long)]
    pub urls: bool,

    /// Only show the name of each plugin
    /// by default it will show a "*" next to installed plugins
    #[clap(long)]
    pub only_names: bool,
}

impl PluginsLsRemote {
    pub fn run(self, config: &Config) -> Result<()> {
        let installed_plugins = config
            .list_plugins()
            .into_iter()
            .filter(|p| p.is_installed())
            .map(|p| p.name().to_string())
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
            let installed = if !self.only_names && installed_plugins.contains(plugin.as_str()) {
                "*"
            } else {
                " "
            };
            let url = if self.urls { repo } else { "" };
            let plugin = pad_str(plugin, max_plugin_len, Alignment::Left, None);
            rtxprintln!("{} {}{}", plugin, installed, url);
        }

        Ok(())
    }
}

const LONG_ABOUT: &str = r#"
List all available remote plugins

The full list is here: https://github.com/jdx/rtx/blob/main/src/default_shorthands.rs

Examples:
  $ rtx plugins ls-remote
"#;

#[cfg(test)]
mod tests {

    #[test]
    fn test_plugin_list_remote() {
        let stdout = assert_cli!("plugin", "ls-remote");
        assert!(stdout.contains("tiny"));
    }
}
