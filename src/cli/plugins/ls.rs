use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::cli::plugins::ls_remote::PluginsLsRemote;
use crate::config::Config;
use crate::output::Output;

/// List installed plugins
///
/// Can also show remotely available plugins to install.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP.as_str(), verbatim_doc_comment)]
pub struct PluginsLs {
    /// List all available remote plugins
    /// Same as `rtx plugins ls-remote`
    #[clap(short, long, verbatim_doc_comment)]
    pub all: bool,

    /// Show the git url for each plugin
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(short, long, verbatim_doc_comment)]
    pub urls: bool,
}

impl Command for PluginsLs {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        if self.all {
            return PluginsLsRemote { urls: self.urls }.run(config, out);
        }

        for plugin in config.plugins.values() {
            if self.urls {
                if let Some(url) = plugin.get_remote_url() {
                    rtxprintln!(out, "{:29} {}", plugin.name, url);
                    continue;
                }
            }
            rtxprintln!(out, "{}", plugin.name);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx plugins ls
      nodejs
      ruby

      $ rtx plugins ls --urls
      nodejs                        https://github.com/asdf-vm/asdf-nodejs.git
      ruby                          https://github.com/asdf-vm/asdf-ruby.git
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::cli::tests::grep;
    use crate::git::Git;
    use crate::{assert_cli, assert_cli_snapshot, dirs};

    #[test]
    fn test_plugin_list() {
        assert_cli_snapshot!("plugin", "list");
    }

    #[test]
    fn test_plugin_list_urls() {
        let stdout = assert_cli!("plugin", "list", "--urls");
        let git = Git::new(dirs::CURRENT.clone());
        let cur_remote = git.get_remote_url().unwrap();
        assert_str_eq!(
            grep(stdout, "dummy"),
            "dummy                         ".to_owned() + cur_remote.as_str()
        );
    }

    #[test]
    fn test_plugin_list_all() {
        let stdout = assert_cli!("plugin", "list", "--all", "--urls");
        assert_str_eq!(
            grep(stdout, "zephyr"),
            "zephyr                        https://github.com/nsaunders/asdf-zephyr.git"
        );
    }
}
