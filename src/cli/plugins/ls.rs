use eyre::Result;
use std::collections::BTreeMap;
use tabled::{Table, Tabled};

use crate::config::Config;
use crate::plugins::PluginType;
use crate::plugins::core::CORE_PLUGINS;
use crate::registry::full_to_url;
use crate::toolset::install_state;
use crate::ui::table;

/// List installed plugins
///
/// Can also show remotely available plugins to install.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct PluginsLs {
    /// List all available remote plugins
    /// Same as `mise plugins ls-remote`
    #[clap(short, long, hide = true, verbatim_doc_comment)]
    pub all: bool,

    /// The built-in plugins only
    /// Normally these are not shown
    #[clap(short, long, verbatim_doc_comment, conflicts_with = "all", hide = true)]
    pub core: bool,

    /// List installed plugins
    #[clap(long, verbatim_doc_comment, conflicts_with = "all", hide = true)]
    pub user: bool,

    /// Show the git url for each plugin
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(short, long, alias = "url", verbatim_doc_comment)]
    pub urls: bool,

    /// Show the git refs for each plugin
    /// e.g.: main 1234abc
    #[clap(long, hide = true, verbatim_doc_comment)]
    pub refs: bool,
}

impl PluginsLs {
    pub async fn run(self, config: &Config) -> Result<()> {
        let mut plugins: BTreeMap<_, _> = install_state::list_plugins()
            .iter()
            .map(|(k, p)| (k.clone(), (*p, None)))
            .collect();

        if self.core {
            for p in CORE_PLUGINS.keys() {
                miseprintln!("{p}");
            }
            return Ok(());
        }

        if self.all {
            for (name, backends) in &config.shorthands {
                for full in backends {
                    let plugin_type = PluginType::from_full(full)?;
                    plugins.insert(name.clone(), (plugin_type, Some(full_to_url(full))));
                }
            }
        }

        let plugins = plugins
            .into_iter()
            .map(|(short, (pt, url))| {
                let plugin = pt.plugin(short.clone());
                if let Some(url) = url {
                    plugin.set_remote_url(url);
                }
                (short, plugin)
            })
            .collect::<BTreeMap<_, _>>();

        if self.urls || self.refs {
            let data = plugins
                .into_iter()
                .map(|(name, p)| {
                    let remote_url = p.get_remote_url().unwrap_or_else(|e| {
                        warn!("{name}: {e:?}");
                        None
                    });
                    let abbrev_ref = p.current_abbrev_ref().unwrap_or_else(|e| {
                        warn!("{name}: {e:?}");
                        None
                    });
                    let sha_short = p.current_sha_short().unwrap_or_else(|e| {
                        warn!("{name}: {e:?}");
                        None
                    });
                    let mut row = Row {
                        plugin: name,
                        url: remote_url.unwrap_or_default(),
                        ref_: String::new(),
                        sha: String::new(),
                    };
                    if p.is_installed() {
                        row.ref_ = abbrev_ref.unwrap_or_default();
                        row.sha = sha_short.unwrap_or_default();
                    }
                    row
                })
                .collect::<Vec<_>>();
            let mut table = Table::new(data);
            table::default_style(&mut table, false);
            miseprintln!("{table}");
        } else {
            hint!("registry", "see available plugins with", "mise registry");
            for tool in plugins.values() {
                miseprintln!("{tool}");
            }
        }
        Ok(())
    }
}

#[derive(Tabled)]
#[tabled(rename_all = "PascalCase")]
struct Row {
    plugin: String,
    url: String,
    ref_: String,
    sha: String,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise plugins ls</bold>
    node
    ruby

    $ <bold>mise plugins ls --urls</bold>
    node    https://github.com/asdf-vm/asdf-nodejs.git
    ruby    https://github.com/asdf-vm/asdf-ruby.git
"#
);
