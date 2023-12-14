use color_eyre::eyre::Result;

use crate::config::config_file::ConfigFile;
use crate::config::Config;

/// Clears an alias for a plugin
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasUnset {
    /// The plugin to remove the alias from
    pub plugin: String,
    /// The alias to remove
    pub alias: String,
}

impl AliasUnset {
    pub fn run(self) -> Result<()> {
        let mut global_config = Config::get().global_config.clone();
        global_config.remove_alias(&self.plugin, &self.alias);
        global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx alias unset node lts-hydrogen</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset_config;
    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_alias_unset() {
        reset_config();

        assert_cli!("alias", "unset", "tiny", "my/alias");
        assert_cli_snapshot!("aliases");

        reset_config();
    }
}
