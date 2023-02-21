use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::output::Output;

/// Clears an alias for a plugin
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases=["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP.as_str(), verbatim_doc_comment)]
pub struct AliasUnset {
    /// The plugin to remove the alias from
    pub plugin: String,
    /// The alias to remove
    pub alias: String,
}

impl Command for AliasUnset {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let rtxrc = config.rtxrc;
        rtxrc.remove_alias(&self.plugin, &self.alias)?;
        rtxrc.save()
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx alias unset nodejs lts/hydrogen
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use crate::assert_cli;

    use crate::test::reset_config;

    #[test]
    fn test_settings_unset() {
        reset_config();

        assert_cli!("alias", "unset", "shfmt", "my/alias");

        let stdout = assert_cli!("aliases");
        assert_snapshot!(stdout, @r###"
        "###);

        reset_config();
    }
}
