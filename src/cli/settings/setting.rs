use crate::config::Settings;
use crate::{env, file};
use eyre::Result;
use serde_json::Value;
use std::collections::BTreeMap;
use toml_edit::Document;

fn load_settings() -> eyre::Result<BTreeMap<String, Value>> {
    let settings = Settings::try_get()?;
    let json = settings.to_string();
    let doc: BTreeMap<String, Value> = serde_json::from_str(&json)?;

    Ok(doc)
}

pub fn get_setting(setting: String) -> eyre::Result<()> {
    let settings = load_settings()?;
    match settings.get(&setting) {
        Some(value) => Ok(miseprintln!("{}", value)),
        None => Err(eyre!("Unknown setting: {}", setting)),
    }
}

pub fn list_settings() -> eyre::Result<()> {
    let settings = load_settings()?;

    for (key, value) in settings {
        if Settings::hidden_configs().contains(key.as_str()) {
            continue;
        }
        miseprintln!("{} = {}", key, value);
    }

    Ok(())
}

pub fn set_settings(setting: String, value: String) -> Result<()> {
    let value: toml_edit::Value = match setting.as_str() {
        "all_compile" => parse_bool(&value)?,
        "always_keep_download" => parse_bool(&value)?,
        "always_keep_install" => parse_bool(&value)?,
        "asdf_compat" => parse_bool(&value)?,
        "color" => parse_bool(&value)?,
        "disable_default_shorthands" => parse_bool(&value)?,
        "disable_tools" => value.split(',').map(|s| s.to_string()).collect(),
        "experimental" => parse_bool(&value)?,
        "jobs" => parse_i64(&value)?,
        "legacy_version_file" => parse_bool(&value)?,
        "node_compile" => parse_bool(&value)?,
        "not_found_auto_install" => parse_bool(&value)?,
        "paranoid" => parse_bool(&value)?,
        "plugin_autoupdate_last_check_duration" => parse_i64(&value)?,
        "python_compile" => parse_bool(&value)?,
        "python_venv_auto_create" => parse_bool(&value)?,
        "quiet" => parse_bool(&value)?,
        "raw" => parse_bool(&value)?,
        "shorthands_file" => value.into(),
        "task_output" => value.into(),
        "trusted_config_paths" => value.split(':').map(|s| s.to_string()).collect(),
        "verbose" => parse_bool(&value)?,
        "yes" => parse_bool(&value)?,
        _ => return Err(eyre!("Unknown setting: {}", setting)),
    };

    let path = &*env::MISE_GLOBAL_CONFIG_FILE;
    file::create_dir_all(path.parent().unwrap())?;
    let raw = file::read_to_string(path).unwrap_or_default();
    let mut config: Document = raw.parse()?;
    if !config.contains_key("settings") {
        config["settings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let settings = config["settings"].as_table_mut().unwrap();
    settings.insert(&setting, toml_edit::Item::Value(value));
    file::write(path, config.to_string())
}

fn parse_bool(value: &str) -> Result<toml_edit::Value> {
    match value.to_lowercase().as_str() {
        "1" | "true" | "yes" | "y" => Ok(true.into()),
        "0" | "false" | "no" | "n" => Ok(false.into()),
        _ => Err(eyre!("{} must be true or false", value)),
    }
}

fn parse_i64(value: &str) -> Result<toml_edit::Value> {
    match value.parse::<i64>() {
        Ok(value) => Ok(value.into()),
        Err(_) => Err(eyre!("{} must be a number", value)),
    }
}

// tests section
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

    #[test]
    fn test_settings_get() {
        reset_config();
        let stdout = assert_cli!("settings", "legacy_version_file");
        assert_snapshot!(stdout, @r###"
        true
        "###);
    }

    #[test]
    fn test_settings_get_unknown() {
        let err = assert_cli_err!("settings", "unknown");
        assert_display_snapshot!(err, @"Unknown setting: unknown");
    }

    #[test]
    fn test_settings_set() {
        reset_config();
        assert_cli!("settings", "legacy_version_file=0");
        assert_cli!("settings", "always_keep_download=y");
        assert_cli!(
            "settings",
            "plugin_autoupdate_last_check_duration=1"
        );

        assert_cli_snapshot!("settings", @r###"
        activate_aggressive = false
        all_compile = false
        always_keep_download = false
        always_keep_install = false
        asdf_compat = false
        color = true
        disable_default_shorthands = false
        disable_tools = []
        experimental = true
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
        verbose = true
        yes = true
        "###);
        reset_config();
    }
}
