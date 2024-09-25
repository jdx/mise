use eyre::Result;

use crate::cli::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::config::Config;

/// Add/update an alias for a plugin
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["add", "create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasSet {
    /// The plugin to set the alias for
    pub plugin: BackendArg,
    /// The alias to set
    pub alias: String,
    /// The value to set the alias to
    pub value: String,
}

impl AliasSet {
    pub fn run(self) -> Result<()> {
        let mut global_config = Config::try_get()?.global_config()?;
        global_config.set_alias(&self.plugin, &self.alias, &self.value)?;
        global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise alias set node lts-hydrogen 18.0.0</bold>
"#
);

#[cfg(test)]
pub mod tests {
    use crate::test::reset;

    #[test]
    fn test_alias_set() {
        reset();
        assert_cli!("alias", "set", "tiny", "my/alias", "3.0");

        assert_cli_snapshot!("aliases", @r#"
        java  lts          21   
        node  lts          20   
        node  lts-argon    4    
        node  lts-boron    6    
        node  lts-carbon   8    
        node  lts-dubnium  10   
        node  lts-erbium   12   
        node  lts-fermium  14   
        node  lts-gallium  16   
        node  lts-hydrogen 18   
        node  lts-iron     20   
        tiny  lts          3.1.0
        tiny  lts-prev     2.0.0
        tiny  my/alias     3.0
        "#);
        reset();
    }
}
