use console::{Alignment, measure_text_width, pad_str};
use eyre::Result;
use itertools::Itertools;

use crate::config::Config;
use crate::toolset::install_state;

/// List all available remote plugins
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["list-remote", "list-all"], long_about = LONG_ABOUT, verbatim_doc_comment)]
pub struct PluginsLsRemote {
    /// Show the git url for each plugin
    /// e.g.: https://github.com/mise-plugins/mise-poetry.git
    #[clap(short, long)]
    pub urls: bool,

    /// Only show the name of each plugin
    /// by default it will show a "*" next to installed plugins
    #[clap(long)]
    pub only_names: bool,
}

impl PluginsLsRemote {
    pub async fn run(self, config: &Config) -> Result<()> {
        let installed_plugins = install_state::list_plugins();

        let shorthands = config.shorthands.iter().sorted().collect_vec();
        let max_plugin_len = shorthands
            .iter()
            .map(|(plugin, _)| measure_text_width(plugin))
            .max()
            .unwrap_or(0);

        if shorthands.is_empty() {
            warn!("default shorthands are disabled");
        }

        for (plugin, backends) in shorthands {
            for repo in backends {
                let installed =
                    if !self.only_names && installed_plugins.contains_key(plugin.as_str()) {
                        "*"
                    } else {
                        " "
                    };
                let url = if self.urls { repo } else { "" };
                let plugin = pad_str(plugin, max_plugin_len, Alignment::Left, None);
                miseprintln!("{} {}{}", plugin, installed, url);
            }
        }

        Ok(())
    }
}

const LONG_ABOUT: &str = r#"
List all available remote plugins

The full list is here: https://github.com/jdx/mise/blob/main/registry.toml

Examples:

    $ mise plugins ls-remote
"#;
