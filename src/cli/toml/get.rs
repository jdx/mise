use crate::cli::toml::top_toml_config;
use crate::file::display_path;
use eyre::bail;
use std::path::PathBuf;

/// Display the value of a setting in a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct TomlGet {
    /// The path of the config to display
    pub key: String,

    /// The path to the mise.toml file to edit
    ///
    /// If not provided, the nearest mise.toml file will be used
    pub file: Option<PathBuf>,
}

impl TomlGet {
    pub fn run(self) -> eyre::Result<()> {
        let mut file = self.file;
        if file.is_none() {
            file = top_toml_config();
        }
        if let Some(file) = file {
            let config: toml::Value = std::fs::read_to_string(&file)?.parse()?;
            let mut value = &config;
            for key in self.key.split('.') {
                value = value.get(key).ok_or_else(|| {
                    eyre::eyre!("Key not found: {} in {}", &self.key, display_path(&file))
                })?;
            }
            match value {
                toml::Value::String(s) => miseprintln!("{}", s),
                toml::Value::Integer(i) => miseprintln!("{}", i),
                toml::Value::Boolean(b) => miseprintln!("{}", b),
                toml::Value::Float(f) => miseprintln!("{}", f),
                toml::Value::Datetime(d) => miseprintln!("{}", d),
                toml::Value::Array(a) => {
                    miseprintln!("{}", toml::to_string(a)?);
                }
                toml::Value::Table(t) => {
                    miseprintln!("{}", toml::to_string(t)?);
                }
            }
        } else {
            bail!("No mise.toml file found");
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise toml get tools.python</bold>
    3.12
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_toml_get() {
        reset();
        assert_cli_snapshot!("toml", "get", "env.TEST_ENV_VAR", @"test-123");
    }
}
