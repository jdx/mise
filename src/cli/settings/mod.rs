use eyre::Result;

mod add;
mod get;
mod ls;
mod set;
mod unset;

fn canonical_setting(key: &str) -> &str {
    match key {
        "pipx" => "pypi",
        "pipx.uvx" => "pypi.uvx",
        "pipx.registry_url" => "pypi.registry_url",
        _ => key,
    }
}

fn remove_legacy_pypi_setting(settings: &mut dyn toml_edit::TableLike, key: &str) -> bool {
    let Some(leaf) = key.strip_prefix("pypi.") else {
        return false;
    };
    if !matches!(leaf, "uvx" | "registry_url") {
        return false;
    }
    settings
        .get_mut("pipx")
        .and_then(toml_edit::Item::as_table_like_mut)
        .is_some_and(|legacy| legacy.remove(leaf).is_some())
}

/// Manage settings
///
/// With no arguments, lists the settings set in config files (same as
/// `mise settings ls`). With SETTING, prints its effective value. With
/// SETTING=VALUE or SETTING VALUE, writes it to the global config
/// (~/.config/mise/config.toml), or to the nearest project config with
/// `--local`. Every setting is described at
/// https://mise.jdx.dev/configuration/settings.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise settings"###,
        help = r###"List settings set in config files"###
    ),
    example(
        r###"mise settings jobs"###,
        help = r###"Show the effective value of one setting"###
    ),
    example(
        r###"mise settings jobs=4"###,
        help = r###"Set jobs in the global config"###
    ),
    example(
        r###"mise settings --local node.mirror_url https://npmmirror.com/mirrors/node/"###,
        help = r###"Set a setting in the project config"###
    )
)]
pub(crate) struct Settings {
    #[usage(subcommand)]
    command: Option<Commands>,

    #[usage(flatten)]
    ls: ls::SettingsLs,

    /// The value to set
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
            Self::Get(mut cmd) => {
                cmd.setting = canonical_setting(&cmd.setting).to_owned();
                cmd.run()
            }
            Self::Ls(mut cmd) => {
                cmd.setting = cmd.setting.map(|key| canonical_setting(&key).to_owned());
                cmd.run()
            }
            Self::Set(cmd) => cmd.run(),
            Self::Unset(cmd) => cmd.run(),
        }
    }
}

impl Settings {
    /// Whether this sets or unsets a setting whose bad value fails settings loading: a pypi
    /// alias conflict, or an unknown choice for a strict setting. Neither may prevent editing
    /// the file that contains it, so these run without loading settings.
    pub(crate) fn is_repair(&self) -> bool {
        let key = match &self.command {
            Some(Commands::Set(cmd)) => Some(cmd.setting.as_str()),
            Some(Commands::Unset(cmd)) => Some(cmd.key.as_str()),
            None if self.value.is_some()
                || self
                    .ls
                    .setting
                    .as_ref()
                    .is_some_and(|key| key.contains('=')) =>
            {
                self.ls.setting.as_deref()
            }
            _ => None,
        };
        key.is_some_and(|key| {
            let key = canonical_setting(key.split('=').next().unwrap_or(key));
            matches!(key, "pypi.uvx" | "pypi.registry_url")
                || crate::config::settings::STRICT_CHOICE_SETTINGS.contains(&key)
        })
    }

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
