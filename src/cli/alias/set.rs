use eyre::Result;

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Add/update an alias for a backend/plugin
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["add", "create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasSet {
    /// The backend/plugin to set the alias for
    pub plugin: BackendArg,
    /// The alias to set
    pub alias: String,
    /// The value to set the alias to
    pub value: Option<String>,
}

impl AliasSet {
    pub async fn run(self) -> Result<()> {
        let mut global_config = Config::get().await?.global_config()?;
        match &self.value {
            None => global_config.set_backend_alias(&self.plugin, &self.alias)?,
            Some(val) => global_config.set_alias(&self.plugin, &self.alias, val)?,
        }
        global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise alias set maven asdf:mise-plugins/mise-maven</bold>
    $ <bold>mise alias set node lts-jod 22.0.0</bold>
"#
);
