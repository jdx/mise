use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::output::Output;

/// Add/update an alias for a plugin
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["add", "create"], after_long_help = AFTER_LONG_HELP.as_str(), verbatim_doc_comment)]
pub struct AliasSet {
    /// The plugin to set the alias for
    pub plugin: String,
    /// The alias to set
    pub alias: String,
    /// The value to set the alias to
    pub value: String,
}

impl Command for AliasSet {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let rtxrc = config.rtxrc;

        rtxrc.set_alias(&self.plugin, &self.alias, &self.value)?;
        rtxrc.save()
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx alias set nodejs lts/hydrogen 18.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
pub mod tests {
    use insta::assert_snapshot;

    use crate::assert_cli;

    use crate::test::reset_config;

    #[test]
    fn test_alias_set() {
        reset_config();
        assert_cli!("alias", "set", "tiny", "my/alias", "3.0");

        let stdout = assert_cli!("aliases");
        println!("stdout {}", stdout);
        assert_snapshot!(stdout);
        reset_config();
    }
}
