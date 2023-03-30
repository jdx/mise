use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::output::Output;

/// Add/update an alias for a plugin
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["add", "create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasSet {
    /// The plugin to set the alias for
    pub plugin: String,
    /// The alias to set
    pub alias: String,
    /// The value to set the alias to
    pub value: String,
}

impl Command for AliasSet {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        config
            .global_config
            .set_alias(&self.plugin, &self.alias, &self.value);
        config.global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx alias set nodejs lts/hydrogen 18.0.0</bold>
"#
);

#[cfg(test)]
pub mod tests {
    use crate::test::reset_config;
    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_alias_set() {
        reset_config();
        assert_cli!("alias", "set", "tiny", "my/alias", "3.0");

        assert_cli_snapshot!("aliases");
        reset_config();
    }
}
