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

    /// Setting name to get/set
    #[clap(conflicts_with = "all")]
    key: Option<String>,

    /// Setting value to set
    #[clap(conflicts_with = "all")]
    value: Option<String>,

    /// List all settings
    #[clap(long, short)]
    all: bool,

    /// Use the local config file instead of the global one
    #[clap(long, short, verbatim_doc_comment, global = true)]
    local: bool,

    /// Output in JSON format
    #[clap(long, short = 'J')]
    pub json: bool,

    /// Output in TOML format
    #[clap(long, short = 'T')]
    pub toml: bool,
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
            if let Some(value) = self.value {
                Commands::Set(set::SettingsSet {
                    key: self.key.unwrap(),
                    value,
                    local: self.local,
                })
            } else if let Some(key) = self.key {
                if let Some((key, value)) = key.split_once('=') {
                    Commands::Set(set::SettingsSet {
                        key: key.to_string(),
                        value: value.to_string(),
                        local: self.local,
                    })
                } else {
                    Commands::Get(get::SettingsGet {
                        key,
                        local: self.local,
                    })
                }
            } else {
                Commands::Ls(ls::SettingsLs {
                    all: self.all,
                    key: None,
                    local: self.local,
                    json: self.json,
                    toml: self.toml,
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

    # set the value of the setting "node.mirror_url" to "https://npm.taobao.org/mirrors/node"
    $ <bold>mise settings node.mirror_url https://npm.taobao.org/mirrors/node</bold>
"#
);
