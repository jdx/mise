use crate::cli::toml::top_toml_config;
use eyre::bail;
use std::path::PathBuf;

/// Display the value of a setting in a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct TomlSet {
    /// The path of the config to display
    pub key: String,

    /// The value to set the key to
    pub value: String,

    /// The path to the mise.toml file to edit
    ///
    /// If not provided, the nearest mise.toml file will be used
    pub file: Option<PathBuf>,
}

impl TomlSet {
    pub fn run(self) -> eyre::Result<()> {
        let mut file = self.file;
        if file.is_none() {
            file = top_toml_config();
        }
        if let Some(file) = file {
            let mut config: toml_edit::DocumentMut = std::fs::read_to_string(&file)?.parse()?;
            let mut value = config.as_item_mut();
            let parts = self.key.split('.').collect::<Vec<&str>>();
            for key in parts.iter().take(parts.len() - 1) {
                value = value
                    .as_table_mut()
                    .unwrap()
                    .entry(key)
                    .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
            }
            let last_key = parts.last().unwrap();
            // TODO: support data types other than strings
            value
                .as_table_mut()
                .unwrap()
                .insert(last_key, self.value.into());

            // TODO: validate by parsing the config
            std::fs::write(&file, config.to_string())?;
        } else {
            bail!("No mise.toml file found");
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise toml set tools.python 3.12</bold>
    $ <bold>mise toml set settings.always_keep_download true</bold>
    $ <bold>mise toml set env.TEST_ENV_VAR ABC</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_toml_set() {
        reset();
        assert_cli_snapshot!("toml", "set", "env.TEST_ENV_VAR", "ABC", @"");
        assert_cli_snapshot!("toml", "get", "env.TEST_ENV_VAR", @"ABC");

        assert_cli_snapshot!("toml", "set", "foo", "1", @"");
        assert_cli_snapshot!("toml", "get", "foo", @"1");

        assert_cli_snapshot!("toml", "set", "a.b.c.d.e.f", "2", @"");
        assert_cli_snapshot!("toml", "get", "a.b.c.d.e.f", @"2");
    }
}
