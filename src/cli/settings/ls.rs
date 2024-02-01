use eyre::Result;

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
        let mut settings = settings.as_dict()?;
        for k in Settings::hidden_configs() {
            settings.remove(*k);
        }
        miseprintln!("{}", settings);
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
        cargo_binstall = true
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
        python_pyenv_repo = "https://github.com/pyenv/pyenv.git"
        python_venv_auto_create = false
        quiet = false
        raw = false
        trusted_config_paths = []
        verbose = true
        yes = true

        [status]
        missing_tools = "if_other_versions_installed"
        show_env = false
        show_tools = false
        "###);
    }
}
