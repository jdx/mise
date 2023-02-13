use std::collections::HashSet;

use color_eyre::eyre::Result;
use itertools::Itertools;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::shorthand::SHORTHAND_MAP;

/// List all available remote plugins
///
/// These are fetched from https://github.com/asdf-vm/asdf-plugins
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list-remote", long_about = LONG_ABOUT, verbatim_doc_comment, alias = "list-all")]
pub struct PluginsLsRemote {
    /// show the git url for each plugin
    ///
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(short, long)]
    pub urls: bool,
}

impl Command for PluginsLsRemote {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let installed_plugins = config
            .ts
            .list_plugins()
            .into_iter()
            .filter(|p| p.is_installed())
            .map(|p| p.name.clone())
            .collect::<HashSet<_>>();

        for (plugin, repo) in SHORTHAND_MAP.iter().sorted().collect_vec() {
            let installed = if installed_plugins.contains(*plugin) {
                "*"
            } else {
                " "
            };
            let url = if self.urls { *repo } else { "" };
            rtxprintln!(out, "{:28} {}{}", plugin, installed, url);
        }

        Ok(())
    }
}

const LONG_ABOUT: &str = r#"
List all available remote plugins

These are fetched from https://github.com/asdf-vm/asdf-plugins

Examples:
  $ rtx plugins ls-remote
"#;

#[cfg(test)]
mod tests {
    use crate::assert_cli;
    use crate::cli::tests::ensure_plugin_installed;

    #[test]
    fn test_plugin_list_remote() {
        ensure_plugin_installed("nodejs");
        let stdout = assert_cli!("plugin", "ls-remote");
        assert!(stdout.contains("nodejs"));
    }
}
