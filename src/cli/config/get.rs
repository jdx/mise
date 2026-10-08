use crate::file::display_path;
use eyre::bail;
use std::path::PathBuf;

/// Print a value from one mise TOML file
///
/// Reads the highest-precedence loaded TOML file by default. Select another with
/// `--file`, `--global`, or `--system`. This reads stored values, not the merged or
/// template-expanded environment; use `mise env` for the resolved environment.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise config get tools.python
3.13"###,
        help = "Print the python request"
    ),
    example("mise config get", help = "Print the whole file"),
    verbatim_doc_comment
)]
pub(super) struct ConfigGet {
    /// Dotted key path to print, such as `tools.python`; omit to print the whole file
    #[usage(complete = complete_key)]
    pub key: Option<String>,

    /// Config file to read, or a directory whose config file to read
    ///
    /// For a directory, mise reads the TOML config file already there, such as
    /// `.mise.toml`, or `mise.toml` when there is none. Defaults to the
    /// highest-precedence loaded TOML file.
    #[usage(short, long, visible_alias = "path", value_hint = usage_rs::ValueHint::AnyPath)]
    pub file: Option<PathBuf>,

    /// Read the global config file
    #[usage(long, short = 'g', conflicts = ["file", "system"])]
    pub global: bool,

    /// Read the system config file
    #[usage(long, conflicts = ["file", "global"])]
    pub system: bool,
}

fn complete_key(
    partial: &<ConfigGet as usage_rs::spec::CommandArgs>::Partial,
    ctx: &usage_rs::complete::CompleteCtx<'_>,
) -> Vec<usage_rs::complete::Candidate<'static>> {
    super::keys::complete(ctx, partial.file.as_deref(), partial.global, partial.system)
}

impl ConfigGet {
    pub(super) fn run(self) -> eyre::Result<()> {
        let file = super::target_file(self.file, self.global, self.system)?;
        if let Some(file) = file {
            if !file.exists() {
                bail!("config file not found: {}", display_path(&file));
            }
            let content = std::fs::read_to_string(&file)?;
            let config: toml::Value = toml::de::from_str(&content)?;
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
                            toml::Value::String(s) => format!("\"{s}\""),
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
