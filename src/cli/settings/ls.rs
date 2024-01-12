use std::collections::BTreeMap;

use eyre::Result;
use serde_json::Value;

use crate::config::Settings;

/// Show current settings
///
/// This is the contents of ~/.config/mise/config.toml
///
/// Note that aliases are also stored in this file
/// but managed separately with `mise aliases`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsLs {}

impl SettingsLs {
    pub fn run(self) -> Result<()> {
        let settings = Settings::try_get()?;
        let json = settings.to_string();
        let doc: BTreeMap<String, Value> = serde_json::from_str(&json)?;
        for (key, value) in doc {
            if Settings::hidden_configs().contains(key.as_str()) {
                continue;
            }
            miseprintln!("{} = {}", key, value);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise settings</bold>
  legacy_version_file = false
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset_config;

    #[test]
    fn test_settings_ls() {
        reset_config();
        assert_cli_snapshot!("settings", @r###"
        activate_aggressive = false
        all_compile = false
        always_keep_download = true
        always_keep_install = true
        asdf_compat = false
        color = true
        disable_default_shorthands = false
        disable_tools = []
        experimental = true
        jobs = 2
        legacy_version_file = true
        legacy_version_file_disable_tools = []
        node_compile = false
        not_found_auto_install = true
        paranoid = false
        plugin_autoupdate_last_check_duration = "20m"
        python_compile = false
        python_default_packages_file = "~/.default-python-packages"
        python_patch_url = null
        python_patches_directory = null
        python_precompiled_arch = null
        python_precompiled_os = null
        python_pyenv_repo = "https://github.com/pyenv/pyenv.git"
        python_venv_auto_create = false
        quiet = false
        raw = false
        shorthands_file = null
        task_output = null
        trusted_config_paths = []
        verbose = true
        yes = true
        "###);
    }
}
