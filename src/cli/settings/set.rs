use miette::{IntoDiagnostic, Result};
use toml_edit::Document;

use crate::{env, file};

/// Add/update a setting
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["add", "create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsSet {
    /// The setting to set
    #[clap()]
    pub setting: String,
    /// The value to set
    pub value: String,
}

impl SettingsSet {
    pub fn run(self) -> Result<()> {
        let value: toml_edit::Value = match self.setting.as_str() {
            "all_compile" => parse_bool(&self.value)?,
            "always_keep_download" => parse_bool(&self.value)?,
            "always_keep_install" => parse_bool(&self.value)?,
            "asdf_compat" => parse_bool(&self.value)?,
            "color" => parse_bool(&self.value)?,
            "disable_default_shorthands" => parse_bool(&self.value)?,
            "disable_tools" => self.value.split(',').map(|s| s.to_string()).collect(),
            "experimental" => parse_bool(&self.value)?,
            "jobs" => parse_i64(&self.value)?,
            "legacy_version_file" => parse_bool(&self.value)?,
            "node_compile" => parse_bool(&self.value)?,
            "not_found_auto_install" => parse_bool(&self.value)?,
            "paranoid" => parse_bool(&self.value)?,
            "plugin_autoupdate_last_check_duration" => parse_i64(&self.value)?,
            "python_compile" => parse_bool(&self.value)?,
            "python_venv_auto_create" => parse_bool(&self.value)?,
            "quiet" => parse_bool(&self.value)?,
            "raw" => parse_bool(&self.value)?,
            "shorthands_file" => self.value.into(),
            "task_output" => self.value.into(),
            "trusted_config_paths" => self.value.split(':').map(|s| s.to_string()).collect(),
            "verbose" => parse_bool(&self.value)?,
            "yes" => parse_bool(&self.value)?,
            _ => return Err(miette!("Unknown setting: {}", self.setting)),
        };

        let path = &*env::MISE_SETTINGS_FILE;
        file::create_dir_all(path.parent().unwrap())?;
        let mut new_file = false;
        if !path.exists() {
            file::write(path, "")?;
            new_file = true;
        }
        let raw = file::read_to_string(path)?;
        let mut settings: Document = raw.parse().into_diagnostic()?;
        settings.insert(&self.setting, toml_edit::Item::Value(value));
        if new_file {
            settings
                .key_decor_mut(&self.setting)
                .unwrap()
                .set_prefix("#:schema https://mise.jdx.dev/schema/settings.json\n");
        }
        file::write(path, settings.to_string())
    }
}

fn parse_bool(value: &str) -> Result<toml_edit::Value> {
    match value.to_lowercase().as_str() {
        "1" | "true" | "yes" | "y" => Ok(true.into()),
        "0" | "false" | "no" | "n" => Ok(false.into()),
        _ => Err(miette!("{} must be true or false", value)),
    }
}

fn parse_i64(value: &str) -> Result<toml_edit::Value> {
    match value.parse::<i64>() {
        Ok(value) => Ok(value.into()),
        Err(_) => Err(miette!("{} must be a number", value)),
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise settings set legacy_version_file true</bold>
"#
);

#[cfg(test)]
pub mod tests {
    use crate::test::reset_config;

    #[test]
    fn test_settings_set() {
        reset_config();
        assert_cli!("settings", "set", "legacy_version_file", "0");
        assert_cli!("settings", "set", "always_keep_download", "y");
        assert_cli!(
            "settings",
            "set",
            "plugin_autoupdate_last_check_duration",
            "1"
        );

        assert_cli_snapshot!("settings", @r###"
        all_compile = false
        always_keep_download = false
        always_keep_install = false
        asdf_compat = false
        color = true
        disable_default_shorthands = false
        disable_tools = []
        experimental = false
        jobs = 4
        legacy_version_file = true
        legacy_version_file_disable_tools = []
        node_compile = false
        not_found_auto_install = true
        paranoid = false
        plugin_autoupdate_last_check_duration = "7d"
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
        verbose = false
        yes = true
        "###);
        reset_config();
    }
}
