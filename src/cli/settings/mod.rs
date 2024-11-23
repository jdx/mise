use clap::Subcommand;
use eyre::Result;

mod add;
mod get;
mod ls;
mod set;
mod unset;

#[derive(Debug, clap::Args)]
#[clap(about = "Manage settings", after_long_help = AFTER_LONG_HELP)]
pub struct Settings {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Only display key names for each setting
    #[clap(long, verbatim_doc_comment, alias = "keys")]
    names: bool,

    #[clap(conflicts_with = "names")]
    setting: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Add(add::SettingsAdd),
    Get(get::SettingsGet),
    Ls(ls::SettingsLs),
    Set(set::SettingsSet),
    Unset(unset::SettingsUnset),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Add(cmd) => cmd.run(),
            Self::Get(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run(),
            Self::Set(cmd) => cmd.run(),
            Self::Unset(cmd) => cmd.run(),
        }
    }
}

impl Settings {
    pub fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or_else(|| {
            if let Some(setting) = self.setting {
                if let Some((setting, value)) = setting.split_once('=') {
                    Commands::Set(set::SettingsSet {
                        setting: setting.to_string(),
                        value: value.to_string(),
                    })
                } else {
                    Commands::Get(get::SettingsGet { setting })
                }
            } else {
                Commands::Ls(ls::SettingsLs {
                    key: None,
                    names: self.names,
                })
            }
        });

        cmd.run()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    # list all settings
    $ <bold>mise settings</bold>

    # get the value of the setting "always_keep_download"
    $ <bold>mise settings always_keep_download</bold>

    # set the value of the setting "always_keep_download" to "true"
    $ <bold>mise settings always_keep_download=true</bold>
"#
);
