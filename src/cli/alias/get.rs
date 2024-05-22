use color_eyre::eyre::{eyre, Result};

use crate::cli::args::ForgeArg;
use crate::config::Config;

/// Show an alias for a plugin
///
/// This is the contents of an alias.<PLUGIN> entry in ~/.config/mise/config.toml
///
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasGet {
    /// The plugin to show the alias for
    pub plugin: ForgeArg,
    /// The alias to show
    pub alias: String,
}

impl AliasGet {
    pub async fn run(self) -> Result<()> {
        let config = Config::try_get().await?;
        match config.get_all_aliases().await.get(&self.plugin) {
            Some(plugin) => match plugin.get(&self.alias) {
                Some(alias) => {
                    miseprintln!("{alias}");
                    Ok(())
                }
                None => Err(eyre!("Unknown alias: {}", &self.alias)),
            },
            None => Err(eyre!("Unknown plugin: {}", &self.plugin)),
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
   $ <bold>mise alias get node lts-hydrogen</bold>
   20.0.0
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;
    use test_log::test;

    #[test(tokio::test)]
    async fn test_alias_get() {
        reset().await;
        let stdout = assert_cli!("alias", "get", "tiny", "my/alias");
        assert_snapshot!(stdout, @r###"
        3.0
        "###);
    }

    #[test(tokio::test)]
    async fn test_alias_get_plugin_unknown() {
        reset().await;
        let err = assert_cli_err!("alias", "get", "unknown", "unknown");
        assert_snapshot!(err, @"Unknown plugin: unknown");
    }

    #[test(tokio::test)]
    async fn test_alias_get_alias_unknown() {
        reset().await;
        let err = assert_cli_err!("alias", "get", "tiny", "unknown");
        assert_snapshot!(err, @"Unknown alias: unknown");
    }
}
