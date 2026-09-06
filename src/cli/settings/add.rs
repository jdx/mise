use eyre::{Result, eyre};

use crate::cli::settings::set::set;

/// Append a value to an array setting
///
/// Adds the value to an array setting such as `disable_hints`, keeping existing entries.
/// This modifies ~/.config/mise/config.toml by default, or the local config with `--local`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(r###"mise settings add disable_hints python_multi"###),
    verbatim_doc_comment
)]
pub(super) struct SettingsAdd {
    /// The setting to set
    #[usage()]
    pub setting: String,
    /// The value to set (optional if provided as KEY=VALUE)
    pub value: Option<String>,
    /// Use the local config file instead of the global one
    #[usage(long, short)]
    pub local: bool,
}

impl SettingsAdd {
    pub(super) fn run(self) -> Result<()> {
        match self.value {
            Some(value) => set(&self.setting, &value, true, self.local),
            None => {
                let (key, value) = self.setting.split_once('=').ok_or_else(|| {
                    eyre!(
                        "Usage: mise settings add <KEY>=<VALUE> or mise settings add <KEY> <VALUE>"
                    )
                })?;
                set(key, value, true, self.local)
            }
        }
    }
}
