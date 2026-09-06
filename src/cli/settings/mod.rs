use eyre::Result;

mod add;
mod get;
mod ls;
mod set;
mod unset;

#[derive(Debug, usage_rs::Args)]
#[usage(about = "Manage settings", after_long_help = AFTER_LONG_HELP)]
pub(crate) struct Settings {
    #[usage(subcommand)]
    command: Option<Commands>,

    #[usage(flatten)]
    ls: ls::SettingsLs,

    /// Setting value to set
    #[usage(conflicts = "all")]
    value: Option<String>,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Add(add::SettingsAdd),
    Get(get::SettingsGet),
    #[usage(visible_alias = "list")]
    Ls(ls::SettingsLs),
    Set(set::SettingsSet),
    Unset(unset::SettingsUnset),
}

impl Commands {
    fn inherit_local(&mut self, local: bool) {
        if !local {
            return;
        }
        match self {
            Self::Add(cmd) => cmd.local = true,
            Self::Get(cmd) => cmd.local = true,
            Self::Ls(cmd) => cmd.local = true,
            Self::Set(cmd) => cmd.local = true,
            Self::Unset(cmd) => cmd.local = true,
        }
    }

    pub(crate) fn run(self) -> Result<()> {
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
    pub(crate) async fn run(self) -> Result<()> {
        let parent_local = self.ls.local;
        let mut cmd = self.command.unwrap_or_else(|| {
            if let Some(value) = self.value {
                Commands::Set(set::SettingsSet {
                    setting: self.ls.setting.unwrap(),
                    value: Some(value),
                    local: self.ls.local,
                })
            } else if let Some(setting) = self.ls.setting {
                if let Some((setting, value)) = setting.split_once('=') {
                    Commands::Set(set::SettingsSet {
                        setting: setting.to_string(),
                        value: Some(value.to_string()),
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
        cmd.inherit_local(parent_local);

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

    # set the value of the setting "node.mirror_url" to "https://npmmirror.com/mirrors/node/"
    $ <bold>mise settings node.mirror_url https://npmmirror.com/mirrors/node/</bold>
"#
);
