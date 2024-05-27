use crate::config::Settings;
use eyre::bail;

/// Show a current setting
///
/// This is the contents of a single entry in ~/.config/mise/config.toml
///
/// Note that aliases are also stored in this file
/// but managed separately with `mise aliases get`
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsGet {
    /// The setting to show
    pub setting: String,
}

impl SettingsGet {
    pub fn run(self) -> eyre::Result<()> {
        let settings = Settings::try_get()?;
        let mut value = toml::Value::Table(settings.as_dict()?);
        let mut key = Some(self.setting.as_str());
        while let Some(k) = key {
            let k = k
                .split_once('.')
                .map(|(a, b)| (a, Some(b)))
                .unwrap_or((k, None));
            if let Some(v) = value.as_table().and_then(|t| t.get(k.0)) {
                key = k.1;
                value = v.clone()
            } else {
                bail!("Unknown setting: {}", self.setting);
            }
        }
        miseprintln!("{}", value);

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings get legacy_version_file</bold>
    true
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;
    use insta::assert_snapshot;

    #[test]
    fn test_settings_get() {
        reset();
        assert_cli_snapshot!("settings", "get", "legacy_version_file", @"true");
        assert_cli_snapshot!("settings", "get", "status.missing_tools", @r###""if_other_versions_installed""###);
    }

    #[test]
    fn test_settings_get_unknown() {
        let err = assert_cli_err!("settings", "get", "unknown");
        assert_snapshot!(err, @"Unknown setting: unknown");
    }
}
