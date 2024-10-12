use eyre::{eyre, Result};
use toml_edit::DocumentMut;

use crate::config::settings::SettingsFile;
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
            "activate_aggressive" => parse_bool(&self.value)?,
            "all_compile" => parse_bool(&self.value)?,
            "always_keep_download" => parse_bool(&self.value)?,
            "always_keep_install" => parse_bool(&self.value)?,
            "asdf" => parse_bool(&self.value)?,
            "asdf_compat" => parse_bool(&self.value)?,
            "cargo_binstall" => parse_bool(&self.value)?,
            "color" => parse_bool(&self.value)?,
            "disable_default_shorthands" => parse_bool(&self.value)?,
            "disable_hints" => self.value.split(',').map(|s| s.to_string()).collect(),
            "disable_tools" => self.value.split(',').map(|s| s.to_string()).collect(),
            "experimental" => parse_bool(&self.value)?,
            "go_default_packages_file" => self.value.into(),
            "go_download_mirror" => self.value.into(),
            "go_repo" => self.value.into(),
            "go_set_gobin" => parse_bool(&self.value)?,
            "go_set_gopath" => parse_bool(&self.value)?,
            "go_set_goroot" => parse_bool(&self.value)?,
            "go_skip_checksum" => parse_bool(&self.value)?,
            "http_timeout" => parse_i64(&self.value)?,
            "jobs" => parse_i64(&self.value)?,
            "legacy_version_file" => parse_bool(&self.value)?,
            "legacy_version_file_disable_tools" => {
                self.value.split(',').map(|s| s.to_string()).collect()
            }
            "libgit2" => parse_bool(&self.value)?,
            "node.compile" => parse_bool(&self.value)?,
            "node.flavor" => self.value.into(),
            "node.mirror_url" => self.value.into(),
            "not_found_auto_install" => parse_bool(&self.value)?,
            "paranoid" => parse_bool(&self.value)?,
            "pin" => parse_bool(&self.value)?,
            "pipx_uvx" => parse_bool(&self.value)?,
            "plugin_autoupdate_last_check_duration" => self.value.into(),
            "python_compile" => parse_bool(&self.value)?,
            "python_default_packages_file" => self.value.into(),
            "python_pyenv_repo" => self.value.into(),
            "python_venv_auto_create" => parse_bool(&self.value)?,
            "quiet" => parse_bool(&self.value)?,
            "raw" => parse_bool(&self.value)?,
            "ruby.apply_patches" => self.value.into(),
            "ruby.default_packages_file" => self.value.into(),
            "ruby.ruby_build_repo" => self.value.into(),
            "ruby.ruby_build_opts" => self.value.into(),
            "ruby.ruby_install" => parse_bool(&self.value)?,
            "ruby.ruby_install_repo" => self.value.into(),
            "ruby.ruby_install_opts" => self.value.into(),
            "ruby.verbose_install" => parse_bool(&self.value)?,
            "shorthands_file" => self.value.into(),
            "status.missing_tools" => self.value.into(),
            "status.show_env" => parse_bool(&self.value)?,
            "status.show_tools" => parse_bool(&self.value)?,
            "task_output" => self.value.into(),
            "trusted_config_paths" => self.value.split(':').map(|s| s.to_string()).collect(),
            "verbose" => parse_bool(&self.value)?,
            "vfox" => parse_bool(&self.value)?,
            "yes" => parse_bool(&self.value)?,
            _ => return Err(eyre!("Unknown setting: {}", self.setting)),
        };

        let path = &*env::MISE_GLOBAL_CONFIG_FILE;
        file::create_dir_all(path.parent().unwrap())?;
        let raw = file::read_to_string(path).unwrap_or_default();
        let mut config: DocumentMut = raw.parse()?;
        if !config.contains_key("settings") {
            config["settings"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let settings = config["settings"].as_table_mut().unwrap();
        if self.setting.as_str().contains(".") {
            let mut parts = self.setting.splitn(2, '.');
            let status = settings
                .entry(parts.next().unwrap())
                .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
                .as_table_mut()
                .unwrap();
            status.insert(parts.next().unwrap(), toml_edit::Item::Value(value));
        } else {
            settings.insert(&self.setting, toml_edit::Item::Value(value));
        }

        // validate
        let _: SettingsFile = toml::from_str(&config.to_string())?;

        file::write(path, config.to_string())
    }
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
        asdf = true
        asdf_compat = false
        cache_prune_age = "0"
        cargo_binstall = true
        color = true
        disable_default_shorthands = false
        disable_hints = []
        disable_tools = []
        experimental = true
        go_default_packages_file = "~/.default-go-packages"
        go_download_mirror = "https://dl.google.com/go"
        go_repo = "https://github.com/golang/go"
        go_set_gopath = false
        go_set_goroot = true
        go_skip_checksum = false
        http_timeout = 30
        jobs = 2
        legacy_version_file = false
        legacy_version_file_disable_tools = []
        libgit2 = true
        not_found_auto_install = true
        paranoid = false
        pin = false
        pipx_uvx = false
        plugin_autoupdate_last_check_duration = "1"
        python_default_packages_file = "~/.default-python-packages"
        python_pyenv_repo = "https://github.com/pyenv/pyenv.git"
        python_venv_stdlib = false
        quiet = false
        raw = false
        trusted_config_paths = []
        use_versions_host = true
        verbose = true
        vfox = false
        yes = true

        [node]

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
