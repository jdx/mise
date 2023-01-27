use color_eyre::eyre::Result;
use indoc::indoc;

use crate::cli::command::Command;
use crate::cli::plugins::ls_remote::PluginsLsRemote;
use crate::config::Config;
use crate::output::Output;
use crate::shorthand_repository::ShorthandRepo;

/// List installed plugins
///
/// Can also show remotely available plugins to install.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct PluginsLs {
    /// list all available remote plugins
    ///
    /// same as `rtx plugins ls-remote`
    #[clap(short, long)]
    pub all: bool,

    /// show the git url for each plugin
    ///
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(short, long)]
    pub urls: bool,
}

impl Command for PluginsLs {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        if self.all {
            return PluginsLsRemote { urls: self.urls }.run(config, out);
        }

        for plugin in config.ts.list_installed_plugins() {
            if self.urls {
                let shr = ShorthandRepo::new(&config.settings);
                let url = shr.lookup(&plugin.name)?;
                rtxprintln!(out, "{:29} {}", plugin.name, url);
            } else {
                rtxprintln!(out, "{}", plugin.name);
            }
        }
        Ok(())
    }
}

const AFTER_LONG_HELP: &str = indoc! {r#"
    List installed plugins
    Can also show remotely available plugins to install.

    Examples:
    
      $ rtx plugins ls
      nodejs
      ruby
      
      $ rtx plugins ls --urls
      nodejs                        https://github.com/asdf-vm/asdf-nodejs.git
      ruby                          https://github.com/asdf-vm/asdf-ruby.git
    "#};

#[cfg(test)]
mod test {
    use crate::assert_cli;
    use crate::cli::test::ensure_plugin_installed;
    use crate::output::Output;

    #[test]
    fn test_plugin_list() {
        ensure_plugin_installed("nodejs");
        let Output { stdout, .. } = assert_cli!("plugin", "list");
        assert!(stdout.content.contains("nodejs"));
    }

    #[test]
    fn test_plugin_list_urls() {
        ensure_plugin_installed("nodejs");
        let Output { stdout, .. } = assert_cli!("plugin", "list", "--urls");
        assert!(stdout
            .content
            .contains("https://github.com/asdf-vm/asdf-nodejs.git"));
    }

    #[test]
    fn test_plugin_list_all() {
        ensure_plugin_installed("nodejs");
        let Output { stdout, .. } = assert_cli!("plugin", "list", "--all", "--urls");
        assert!(stdout
            .content
            .contains("https://github.com/asdf-vm/asdf-nodejs.git"));
    }
}
