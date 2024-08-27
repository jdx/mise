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
pub struct SettingsLs {
    /// Only display key names for each setting
    #[clap(long, verbatim_doc_comment)]
    pub keys: bool,
}

impl SettingsLs {
    pub fn run(self) -> Result<()> {
        let settings = Settings::try_get()?;
        let mut settings = settings.as_dict()?;
        for k in Settings::hidden_configs() {
            settings.remove(*k);
        }
        if self.keys {
            return self.print_keys(&settings);
        }
        miseprintln!("{}", settings);
        Ok(())
    }

    fn print_keys(&self, settings: &toml::Table) -> Result<()> {
        for (k, v) in settings {
            miseprintln!("{k}");
            if let toml::Value::Table(t) = v {
                for (subkey, _) in t {
                    miseprintln!("{k}.{subkey}");
                }
            }
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
    use crate::test::reset;

    #[test]
    fn test_settings_ls() {
        reset();
        assert_cli_snapshot!("settings", @r###"
        activate_aggressive = false
        all_compile = false
        always_keep_download = true
        always_keep_install = true
        asdf = true
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
        jobs = 2
        legacy_version_file = true
        legacy_version_file_disable_tools = []
        libgit2 = true
        node_compile = false
        not_found_auto_install = true
        paranoid = false
        pipx_uvx = false
        plugin_autoupdate_last_check_duration = "20m"
        python_default_packages_file = "~/.default-python-packages"
        python_pyenv_repo = "https://github.com/pyenv/pyenv.git"
        quiet = false
        raw = false
        trusted_config_paths = []
        verbose = true
        vfox = false
        yes = true

        [status]
        missing_tools = "if_other_versions_installed"
        show_env = false
        show_tools = false
        "###);
    }

    #[test]
    fn test_settings_ls_keys() {
        reset();
        assert_cli_snapshot!("settings", "--keys", @r###"
        activate_aggressive
        all_compile
        always_keep_download
        always_keep_install
        asdf
        asdf_compat
        cargo_binstall
        color
        disable_default_shorthands
        disable_tools
        experimental
        go_default_packages_file
        go_download_mirror
        go_repo
        go_set_gopath
        go_set_goroot
        go_skip_checksum
        http_timeout
        jobs
        legacy_version_file
        legacy_version_file_disable_tools
        libgit2
        node_compile
        not_found_auto_install
        paranoid
        pipx_uvx
        plugin_autoupdate_last_check_duration
        python_default_packages_file
        python_pyenv_repo
        quiet
        raw
        status
        status.missing_tools
        status.show_env
        status.show_tools
        trusted_config_paths
        verbose
        vfox
        yes
        "###);
    }
}
