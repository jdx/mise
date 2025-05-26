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

    #[clap(flatten)]
    ls: ls::SettingsLs,

    /// Setting value to set
    #[clap(conflicts_with = "all")]
    value: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Add(add::SettingsAdd),
    Get(get::SettingsGet),
    #[clap(visible_alias = "list")]
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
    pub async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or_else(|| {
            if let Some(value) = self.value {
                Commands::Set(set::SettingsSet {
                    setting: self.ls.setting.unwrap(),
                    value,
                    local: self.ls.local,
                })
            } else if let Some(setting) = self.ls.setting {
                if let Some((setting, value)) = setting.split_once('=') {
                    Commands::Set(set::SettingsSet {
                        setting: setting.to_string(),
                        value: value.to_string(),
                        local: self.ls.local,
                    })
                } else {
                    Commands::Get(get::SettingsGet {
                        setting,
                        local: self.ls.local,
                    })
                }
            } else {
                Commands::Ls(self.ls)
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

    # set the value of the setting "node.mirror_url" to "https://npm.taobao.org/mirrors/node"
    $ <bold>mise settings node.mirror_url https://npm.taobao.org/mirrors/node</bold>
"#
);
