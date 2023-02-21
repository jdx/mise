use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::PluginName;

/// List aliases
/// Shows the aliases that can be specified.
/// These can come from user config or from plugins in `bin/list-aliases`.
///
/// For user config, aliases are defined like the following in `~/.config/rtx/config.toml`:
///
///   [alias.nodejs]
///   lts = "18.0.0"
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP.as_str(), verbatim_doc_comment)]
pub struct AliasLs {
    /// Show aliases for <PLUGIN>
    #[clap(short, long)]
    pub plugin: Option<PluginName>,
}

impl Command for AliasLs {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        for (plugin_name, aliases) in &config.aliases {
            if let Some(plugin) = &self.plugin {
                if plugin_name != plugin {
                    continue;
                }
            }

            for (from, to) in aliases.iter() {
                rtxprintln!(out, "{:20} {:20} {}", plugin_name, from, to);
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx aliases
      nodejs    lts/hydrogen   18.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use crate::assert_cli;
    use crate::cli::tests::ensure_plugin_installed;

    #[test]
    fn test_alias_ls() {
        ensure_plugin_installed("tiny");
        let stdout = assert_cli!("aliases");
        assert!(stdout.contains("my/alias"));
    }
}
