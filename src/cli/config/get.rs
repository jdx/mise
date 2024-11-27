use crate::config::top_toml_config;
use crate::file::display_path;
use eyre::bail;
use std::path::PathBuf;

/// Display the value of a setting in a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ConfigGet {
    /// The path of the config to display
    pub key: Option<String>,

    /// The path to the mise.toml file to edit
    ///
    /// If not provided, the nearest mise.toml file will be used
    #[clap(short, long)]
    pub file: Option<PathBuf>,
}

impl ConfigGet {
    pub fn run(self) -> eyre::Result<()> {
        let mut file = self.file;
        if file.is_none() {
            file = top_toml_config();
        }
        if let Some(file) = file {
            let config: toml::Value = std::fs::read_to_string(&file)?.parse()?;
            let mut value = &config;
            if let Some(key) = &self.key {
                for k in key.split('.') {
                    value = value.get(k).ok_or_else(|| {
                        eyre::eyre!("Key not found: {} in {}", key, display_path(&file))
                    })?;
                }
            }

            match value {
                toml::Value::String(s) => miseprintln!("{}", s),
                toml::Value::Integer(i) => miseprintln!("{}", i),
                toml::Value::Boolean(b) => miseprintln!("{}", b),
                toml::Value::Float(f) => miseprintln!("{}", f),
                toml::Value::Datetime(d) => miseprintln!("{}", d),
                toml::Value::Array(a) => {
                    // seems that the toml crate does not have a way to serialize an array directly?
                    // workaround which only handle non-nested arrays
                    let elements: Vec<String> = a
                        .iter()
                        .map(|v| match v {
                            toml::Value::String(s) => format!("\"{}\"", s),
                            toml::Value::Integer(i) => i.to_string(),
                            toml::Value::Boolean(b) => b.to_string(),
                            toml::Value::Float(f) => f.to_string(),
                            toml::Value::Datetime(d) => d.to_string(),
                            toml::Value::Array(_) => "[...]".to_string(),
                            toml::Value::Table(_) => "{...}".to_string(),
                        })
                        .collect();
                    miseprintln!("[{}]", elements.join(", "));
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
