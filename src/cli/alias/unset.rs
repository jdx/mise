use eyre::Result;

use crate::cli::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::config::Config;

/// Clears an alias for a plugin
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasUnset {
    /// The plugin to remove the alias from
    pub plugin: BackendArg,
    /// The alias to remove
    pub alias: String,
}

impl AliasUnset {
    pub fn run(self) -> Result<()> {
        let mut global_config = Config::try_get()?.global_config()?;
        global_config.remove_alias(&self.plugin, &self.alias)?;
        global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise alias unset node lts-jod</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_alias_unset() {
        reset();

        assert_cli!("alias", "unset", "tiny", "my/alias");
        assert_cli_snapshot!("aliases", @r#"
        java  lts          21   
        node  lts          22   
        node  lts-argon    4    
        node  lts-boron    6    
        node  lts-carbon   8    
        node  lts-dubnium  10   
        node  lts-erbium   12   
        node  lts-fermium  14   
        node  lts-gallium  16   
        node  lts-hydrogen 18   
        node  lts-iron     20   
        node  lts-jod      22   
        tiny  lts          3.1.0
        tiny  lts-prev     2.0.0
        "#);

        reset();
    }
}
