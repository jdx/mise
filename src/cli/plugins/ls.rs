use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::cli::plugins::ls_remote::PluginsLsRemote;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::PluginType;

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
        if self.all {
            return PluginsLsRemote {
                urls: self.urls,
                only_names: false,
            }
            .run(config, out);
        }

        let mut plugins = config.tools.values().collect::<Vec<_>>();

        if self.core {
            plugins.retain(|p| matches!(p.plugin.get_type(), PluginType::Core));
        } else {
            plugins.retain(|p| matches!(p.plugin.get_type(), PluginType::External));
        }

        if self.urls || self.refs {
            for plugin in plugins {
                rtxprint!(out, "{:29}", plugin.name);
                if self.urls {
                    if let Some(url) = plugin.get_remote_url() {
                        rtxprint!(out, " {}", url);
                    }
                }
                if self.refs {
                    if let Ok(aref) = plugin.current_abbrev_ref() {
                        rtxprint!(out, " {}", aref);
                    }
                    if let Ok(sha) = plugin.current_sha_short() {
                        rtxprint!(out, " {}", sha);
                    }
                }
                rtxprint!(out, "\n");
            }
        } else {
            for plugin in plugins {
                rtxprintln!(out, "{}", plugin.name);
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
