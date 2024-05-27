use std::path::Path;

use color_eyre::eyre::eyre;
use color_eyre::{Result, Section};
use toml_edit::{DocumentMut, Item, Value};

use crate::{file, parse_error};

#[derive(Debug, Default, Clone)]
pub struct MisePluginTomlScriptConfig {
    pub cache_key: Option<Vec<String>>,
    pub data: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct MisePluginToml {
    pub exec_env: MisePluginTomlScriptConfig,
    pub list_aliases: MisePluginTomlScriptConfig,
    pub list_bin_paths: MisePluginTomlScriptConfig,
    pub list_legacy_filenames: MisePluginTomlScriptConfig,
}

impl MisePluginToml {
    pub fn from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Default::default());
        }
        trace!("parsing: {}", path.display());
        let mut rf = Self::init();
        let body = file::read_to_string(path).suggestion("ensure file exists and can be read")?;
        rf.parse(&body)?;
        Ok(rf)
    }

    fn init() -> Self {
        Self {
            ..Default::default()
        }
    }

    fn parse(&mut self, s: &str) -> Result<()> {
        let doc: DocumentMut = s.parse().suggestion("ensure file is valid TOML")?;
        for (k, v) in doc.iter() {
            match k {
                "exec-env" => self.exec_env = self.parse_script_config(k, v)?,
                "list-aliases" => self.list_aliases = self.parse_script_config(k, v)?,
                "list-bin-paths" => self.list_bin_paths = self.parse_script_config(k, v)?,
                "list-legacy-filenames" => {
                    self.list_legacy_filenames = self.parse_script_config(k, v)?
                }
                // this is an old key used in rtx-python
                // this file is invalid, so just stop parsing entirely if we see it
                "legacy-filenames" => return Ok(()),
                _ => Err(eyre!("unknown key: {}", k))?,
            }
        }
        Ok(())
    }

    fn parse_script_config(&mut self, key: &str, v: &Item) -> Result<MisePluginTomlScriptConfig> {
        match v.as_table_like() {
            Some(table) => {
                let mut config = MisePluginTomlScriptConfig::default();
                for (k, v) in table.iter() {
                    let key = format!("{}.{}", key, k);
                    match k {
                        "cache-key" => config.cache_key = Some(self.parse_string_array(k, v)?),
                        "data" => match v.as_value() {
                            Some(v) => config.data = Some(self.parse_string(k, v)?),
                            _ => parse_error!(key, v, "string"),
                        },
                        _ => parse_error!(key, v, "one of: cache-key"),
                    }
                }
                Ok(config)
            }
            _ => parse_error!(key, v, "table"),
        }
    }

    fn parse_string_array(&mut self, k: &str, v: &Item) -> Result<Vec<String>> {
        match v.as_array() {
            Some(arr) => {
                let mut out = vec![];
                for v in arr {
                    out.push(self.parse_string(k, v)?);
                }
                Ok(out)
            }
            _ => parse_error!(k, v, "array"),
        }
    }

    fn parse_string(&mut self, k: &str, v: &Value) -> Result<String> {
        match v.as_str() {
            Some(v) => Ok(v.to_string()),
            _ => parse_error!(k, v, "string"),
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::formatdoc;
    use insta::assert_debug_snapshot;

    use crate::dirs;

    use super::*;

    #[test]
    fn test_fixture() {
        let cf = MisePluginToml::from_file(&dirs::HOME.join("fixtures/mise.plugin.toml")).unwrap();

        assert_debug_snapshot!(cf.exec_env);
    }

    #[test]
    fn test_exec_env() {
        let cf = parse(&formatdoc! {r#"
        [list-aliases]
        data = "test-aliases"
        [list-legacy-filenames]
        data = "test-legacy-filenames"
        [exec-env]
        cache-key = ["foo", "bar"]
        [list-bin-paths]
        cache-key = ["foo"]
        "#});

        assert_debug_snapshot!(cf.exec_env, @r###"
        MisePluginTomlScriptConfig {
            cache_key: Some(
                [
                    "foo",
                    "bar",
                ],
            ),
            data: None,
        }
        "###);
    }

    fn parse(s: &str) -> MisePluginToml {
        let mut cf = MisePluginToml::init();
        cf.parse(s).unwrap();
        cf
    }
}
