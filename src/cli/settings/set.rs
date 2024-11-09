use eyre::{bail, eyre, Result};
use toml_edit::DocumentMut;

use crate::config::settings::{SettingsFile, SettingsType, SETTINGS_META};
use crate::{env, file};

/// Add/update a setting
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsSet {
    /// The setting to set
    #[clap()]
    pub setting: String,
    /// The value to set
    pub value: String,
}

impl SettingsSet {
    pub fn run(self) -> Result<()> {
        set(&self.setting, &self.value, false)
    }
}

pub fn set(mut key: &str, value: &str, add: bool) -> Result<()> {
    let value = if let Some(meta) = SETTINGS_META.get(key) {
        match meta.type_ {
            SettingsType::Bool => parse_bool(value)?,
            SettingsType::Integer => parse_i64(value)?,
            SettingsType::Duration => parse_duration(value)?,
            SettingsType::Url | SettingsType::Path | SettingsType::String => value.into(),
            SettingsType::ListString => parse_list_by_comma(value)?,
            SettingsType::ListPath => parse_list_by_colon(value)?,
        }
    } else {
        bail!("Unknown setting: {}", key);
    };

    let path = &*env::MISE_GLOBAL_CONFIG_FILE;
    file::create_dir_all(path.parent().unwrap())?;
    let raw = file::read_to_string(path).unwrap_or_default();
    let mut config: DocumentMut = raw.parse()?;
    if !config.contains_key("settings") {
        config["settings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let mut settings = config["settings"].as_table_mut().unwrap();
    if key.contains(".") {
        let (parent_key, child_key) = key.split_once('.').unwrap();

        key = child_key;
        settings = settings
            .entry(parent_key)
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut()
            .unwrap();
    }

    let value = match settings.get(key).map(|c| c.as_array()) {
        Some(Some(array)) if add => {
            let mut array = array.clone();
            array.extend(value.as_array().unwrap().iter().cloned());
            array.into()
        }
        _ => value,
    };
    settings.insert(key, value.into());

    // validate
    let _: SettingsFile = toml::from_str(&config.to_string())?;

    file::write(path, config.to_string())
}

fn parse_list_by_comma(value: &str) -> Result<toml_edit::Value> {
    Ok(value.split(',').map(|s| s.to_string()).collect())
}

fn parse_list_by_colon(value: &str) -> Result<toml_edit::Value> {
    Ok(value.split(':').map(|s| s.to_string()).collect())
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

fn parse_duration(value: &str) -> Result<toml_edit::Value> {
    humantime::parse_duration(value)?;
    Ok(value.into())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings set legacy_version_file true</bold>
"#
);

#[cfg(test)]
pub mod tests {
    use crate::test::reset;

    #[test]
    fn test_settings_set() {
        reset();
        assert_cli!("settings", "set", "legacy_version_file", "0");
        assert_cli!("settings", "set", "always_keep_download", "y");
        assert_cli!("settings", "set", "status.missing_tools", "never");
        assert_cli!(
            "settings",
            "set",
            "plugin_autoupdate_last_check_duration",
            "1"
        );

        assert_cli_snapshot!("settings", @r#"
        activate_aggressive = false
        all_compile = false
        always_keep_download = true
        always_keep_install = true
        asdf_compat = false
        cache_prune_age = "0"
        color = true
        disable_backends = []
        disable_default_registry = false
        disable_hints = []
        disable_tools = []
        experimental = true
        fetch_remote_versions_cache = "1h"
        fetch_remote_versions_timeout = "10s"
        go_default_packages_file = "~/.default-go-packages"
        go_download_mirror = "https://dl.google.com/go"
        go_repo = "https://github.com/golang/go"
        go_set_gopath = false
        go_set_goroot = true
        go_skip_checksum = false
        http_timeout = "30s"
        jobs = 2
        legacy_version_file = false
        legacy_version_file_disable_tools = []
        libgit2 = true
        lockfile = false
        not_found_auto_install = true
        paranoid = false
        pin = false
        plugin_autoupdate_last_check_duration = "1"
        quiet = false
        raw = false
        task_timings = false
        trusted_config_paths = []
        use_versions_host = true
        verbose = true
        yes = true

        [cargo]
        binstall = true

        [node]

        [npm]
        bun = false

        [pipx]
        uvx = false

        [python]
        default_packages_file = "~/.default-python-packages"
        pyenv_repo = "https://github.com/pyenv/pyenv.git"
        venv_auto_create = false
        venv_stdlib = false

        [ruby]
        default_packages_file = "~/.default-gems"
        ruby_build_repo = "https://github.com/rbenv/ruby-build.git"
        ruby_install = false
        ruby_install_repo = "https://github.com/postmodern/ruby-install.git"

        [status]
        missing_tools = "never"
        show_env = false
        show_tools = false
        "#);
        reset();
    }
}
