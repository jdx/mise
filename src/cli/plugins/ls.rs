use std::collections::BTreeSet;
use std::sync::Arc;

use eyre::Result;
use rayon::prelude::*;
use tabled::settings::object::Columns;
use tabled::settings::{Margin, Modify, Padding, Style};
use tabled::Tabled;

use crate::config::Config;
use crate::plugins;
use crate::plugins::{ExternalPlugin, PluginType};

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
    #[clap(short, long, verbatim_doc_comment, conflicts_with = "all")]
    pub core: bool,

    /// List installed plugins
    ///
    /// This is the default behavior but can be used with --core
    /// to show core and user plugins
    #[clap(long, verbatim_doc_comment, conflicts_with = "all")]
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
    pub fn run(self, config: &Config) -> Result<()> {
        let mut tools = plugins::list().into_iter().collect::<BTreeSet<_>>();

        if self.all {
            for (plugin, url) in config.get_shorthands() {
                let mut ep = ExternalPlugin::new(plugin.clone());
                ep.repo_url = Some(url.to_string());
                tools.insert(Arc::new(ep));
            }
        } else if self.user && self.core {
        } else if self.core {
            tools.retain(|p| matches!(p.get_plugin_type(), PluginType::Core));
        } else {
            tools.retain(|p| matches!(p.get_plugin_type(), PluginType::External));
        }

        if self.urls || self.refs {
            let data = tools
                .into_par_iter()
                .map(|p| {
                    let mut row = Row {
                        plugin: p.id().to_string(),
                        url: p.get_remote_url().unwrap_or_default(),
                        ref_: String::new(),
                        sha: String::new(),
                    };
                    if p.is_installed() {
                        row.ref_ = p.current_abbrev_ref().unwrap_or_default();
                        row.sha = p.current_sha_short().unwrap_or_default();
                    }
                    row
                })
                .collect::<Vec<_>>();
            let mut table = tabled::Table::new(data);
            table
                .with(Style::empty())
                .with(Margin::new(0, 0, 0, 0))
                .with(Modify::new(Columns::first()).with(Padding::new(0, 1, 0, 0)))
                .with(Modify::new(Columns::last()).with(Padding::zero()));
            miseprintln!("{table}");
        } else {
            for tool in tools {
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

#[cfg(test)]
mod tests {
    use crate::cli::tests::grep;

    #[test]
    fn test_plugin_list() {
        assert_cli_snapshot!("plugin", "list");
    }

    #[test]
    fn test_plugin_list_urls() {
        let stdout = assert_cli!("plugin", "list", "--urls");
        assert!(stdout.contains("dummy"))
    }

    #[test]
    fn test_plugin_list_all() {
        let stdout = assert_cli!("plugin", "list", "--all", "--urls");
        assert_snapshot!(grep(stdout, "zephyr"));
    }

    #[test]
    fn test_plugin_refs() {
        let stdout = assert_cli!("plugin", "list", "--refs");
        assert!(stdout.contains("dummy"))
    }
}
