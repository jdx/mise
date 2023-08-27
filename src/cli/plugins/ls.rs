use std::collections::BTreeSet;
use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::{ExternalPlugin, PluginType};
use crate::tool::Tool;

/// List installed plugins
///
/// Can also show remotely available plugins to install.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct PluginsLs {
    /// List all available remote plugins
    /// Same as `rtx plugins ls-remote`
    #[clap(short, long, hide = true, verbatim_doc_comment)]
    pub all: bool,

    /// The built-in plugins only
    /// Normally these are not shown
    #[clap(short, long, verbatim_doc_comment)]
    pub core: bool,

    /// Show the git url for each plugin
    /// e.g.: https://github.com/asdf-vm/asdf-node.git
    #[clap(short, long, verbatim_doc_comment)]
    pub urls: bool,

    /// Show the git refs for each plugin
    /// e.g.: main 1234abc
    #[clap(long, verbatim_doc_comment)]
    pub refs: bool,
}

impl Command for PluginsLs {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let mut tools = config.tools.values().cloned().collect::<BTreeSet<_>>();

        if self.all {
            for (plugin, url) in config.get_shorthands() {
                let mut ep = ExternalPlugin::new(plugin);
                ep.repo_url = Some(url.to_string());
                let tool = Tool::new(plugin.clone(), Box::from(ep));
                tools.insert(Arc::new(tool));
            }
        } else if self.core {
            tools.retain(|p| matches!(p.plugin.get_type(), PluginType::Core));
        } else {
            tools.retain(|p| matches!(p.plugin.get_type(), PluginType::External));
        }

        if self.urls || self.refs {
            for tool in tools {
                rtxprint!(out, "{:29}", tool.name);
                if self.urls {
                    if let Some(url) = tool.get_remote_url() {
                        rtxprint!(out, " {}", url);
                    }
                }
                if self.refs {
                    if let Ok(aref) = tool.current_abbrev_ref() {
                        rtxprint!(out, " {}", aref);
                    }
                    if let Ok(sha) = tool.current_sha_short() {
                        rtxprint!(out, " {}", sha);
                    }
                }
                rtxprint!(out, "\n");
            }
        } else {
            for tool in tools {
                rtxprintln!(out, "{}", tool.name);
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx plugins ls</bold>
  node
  ruby

  $ <bold>rtx plugins ls --urls</bold>
  node                        https://github.com/asdf-vm/asdf-node.git
  ruby                          https://github.com/asdf-vm/asdf-ruby.git
"#
);

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::cli::tests::grep;
    use crate::{assert_cli, assert_cli_snapshot};

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
        assert_str_eq!(
            grep(stdout, "zephyr"),
            "zephyr                        https://github.com/nsaunders/asdf-zephyr.git"
        );
    }

    #[test]
    fn test_plugin_refs() {
        let stdout = assert_cli!("plugin", "list", "--refs");
        assert!(stdout.contains("dummy"))
    }
}
