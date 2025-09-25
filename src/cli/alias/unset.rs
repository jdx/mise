use eyre::Result;

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Clears an alias for a backend/plugin
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasUnset {
    /// The backend/plugin to remove the alias from
    pub plugin: BackendArg,
    /// The alias to remove
    pub alias: Option<String>,
}

impl AliasUnset {
    pub async fn run(self) -> Result<()> {
        let mut global_config = Config::get().await?.global_config()?;
        match self.alias {
            None => {
                global_config.remove_backend_alias(&self.plugin)?;
            }
            Some(ref alias) => {
                global_config.remove_alias(&self.plugin, alias)?;
            }
        }
        global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise alias unset maven</bold>
    $ <bold>mise alias unset node lts-jod</bold>
"#
);
