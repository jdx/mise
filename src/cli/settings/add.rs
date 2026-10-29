use eyre::{Result, eyre};

use crate::cli::settings::set::set;

/// Append a value to an array setting
///
/// Adds the value to an array setting such as `disable_hints`, keeping existing
/// entries. Writes ~/.config/mise/config.toml, or the nearest project config
/// with `--local`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise settings add disable_hints python_multi"###,
        help = "Stop showing the python_multi hint"
    ),
    verbatim_doc_comment
)]
pub(super) struct SettingsAdd {
    /// The array setting to append to
    #[usage()]
    pub setting: String,
    /// The value to append (or pass SETTING=VALUE)
    pub value: Option<String>,
    /// Write to the nearest project config instead of the global config
    ///
    /// The nearest project config is the lowest-precedence TOML file in the
    /// nearest directory that has one, or ./mise.toml.
    #[usage(long, short)]
    pub local: bool,

    /// Write to the global override file, `config.override.toml`
    ///
    /// The override file outranks all project config, so the setting wins over
    /// the same setting in a project's `mise.toml`.
    #[usage(long = "override", conflicts = "local")]
    pub override_file: bool,
}

impl SettingsAdd {
    pub(super) fn run(self) -> Result<()> {
        match self.value {
            Some(value) => set(&self.setting, &value, true, self.local, self.override_file),
            None => {
                let (key, value) = self.setting.split_once('=').ok_or_else(|| {
                    eyre!(
                        "Usage: mise settings add <KEY>=<VALUE> or mise settings add <KEY> <VALUE>"
                    )
                })?;
                set(key, value, true, self.local, self.override_file)
            }
        }
    }
}
