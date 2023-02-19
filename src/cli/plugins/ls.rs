use atty::Stream;
use color_eyre::eyre::Result;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::cli::plugins::ls_remote::PluginsLsRemote;
use crate::config::Config;
use crate::output::Output;
use crate::ui::color::Color;

/// List installed plugins
///
/// Can also show remotely available plugins to install.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP.as_str(), verbatim_doc_comment)]
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

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx plugins ls
      nodejs
      ruby

      $ rtx plugins ls --urls
      nodejs                        https://github.com/asdf-vm/asdf-nodejs.git
      ruby                          https://github.com/asdf-vm/asdf-ruby.git
    "#, COLOR.header("Examples:")}
});

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;
    use crate::cli::tests::grep;

    #[test]
    fn test_plugin_list() {
        let stdout = assert_cli!("plugin", "list");
        assert_str_eq!(grep(stdout, "nodejs"), "nodejs");
    }

    #[test]
    fn test_plugin_list_urls() {
        let stdout = assert_cli!("plugin", "list", "--urls");
        assert_str_eq!(
            grep(stdout, "shfmt"),
            "shfmt                         https://github.com/luizm/asdf-shfmt.git"
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
