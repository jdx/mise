use eyre::Result;
use toml_edit::DocumentMut;

use crate::config::settings::SettingsFile;
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
        let mut config: DocumentMut = raw.parse()?;
        if !config.contains_key("settings") {
            return Ok(());
        }
        let settings = config["settings"].as_table_mut().unwrap();
        settings.remove(&self.setting);

        // validate
        let _: SettingsFile = toml::from_str(&config.to_string())?;

        file::write(&path, config.to_string())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings unset legacy_version_file</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_settings_unset() {
        reset();

        assert_cli!("settings", "unset", "jobs");

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
        go_default_packages_file = "~/.default-go-packages"
        go_download_mirror = "https://dl.google.com/go"
        go_repo = "https://github.com/golang/go"
        go_set_gopath = false
        go_set_goroot = true
        go_skip_checksum = false
        http_timeout = 30
        jobs = 4
        legacy_version_file = true
        legacy_version_file_disable_tools = []
        node_compile = false
        not_found_auto_install = true
        paranoid = false
        plugin_autoupdate_last_check_duration = "20m"
        python_default_packages_file = "~/.default-python-packages"
        python_pyenv_repo = "https://github.com/pyenv/pyenv.git"
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

        reset();
    }
}
