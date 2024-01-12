use eyre::Result;
use toml_edit::Document;

use crate::{env, file};

/// Clears a setting
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsUnset {
    /// The setting to remove
    pub setting: String,
}

impl SettingsUnset {
    pub fn run(self) -> Result<()> {
        let path = env::MISE_CONFIG_DIR.join("config.toml");
        let raw = file::read_to_string(&path)?;
        let mut settings: Document = raw.parse()?;
        settings.remove(&self.setting);
        file::write(&path, settings.to_string())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise settings unset legacy_version_file</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset_config;

    #[test]
    fn test_settings_unset() {
        reset_config();

        assert_cli!("settings", "unset", "legacy_version_file");

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

        reset_config();
    }
}
