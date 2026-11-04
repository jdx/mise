use console::{Alignment, measure_text_width, pad_str};
use eyre::Result;
use itertools::Itertools;

use crate::config::Config;
use crate::toolset::install_state;

/// List registry tools that have an asdf or vfox plugin backend
#[derive(Debug, usage_rs::Args)]
#[usage(visible_aliases = ["list-remote", "list-all"], long_about = LONG_ABOUT, example(r###"mise plugins ls-remote"###, help = "List registry tools with a plugin backend"), verbatim_doc_comment)]
pub(super) struct PluginsLsRemote {
    /// Show the plugin source for each shorthand, e.g. vfox:jdx/vfox-poetry
    #[usage(short, long)]
    pub urls: bool,

    /// Only show the name of each plugin, without the `*` marking installed plugins
    #[usage(long)]
    pub only_names: bool,
}

impl PluginsLsRemote {
    pub(super) async fn run(self, config: &Config) -> Result<()> {
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

const LONG_ABOUT: &str = r#"List registry tools that have an asdf or vfox plugin backend

Each line is a registry shorthand that lists an asdf or vfox plugin among its
backends; mise may still install the tool through another backend first. `*`
marks plugins you have installed. Entries from the `shorthands_file` setting
are listed too. Use `mise registry` to list every registry tool and its
backends in order."#;
