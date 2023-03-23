use std::fs;
use std::path::Path;

use color_eyre::eyre::eyre;
use color_eyre::{Result, Section};
use toml_edit::{Document, Item, Value};

use crate::parse_error;

#[derive(Debug, Default, Clone)]
pub struct RtxPluginTomlScriptConfig {
    pub cache_key: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone)]
pub struct RtxPluginToml {
    pub exec_env: RtxPluginTomlScriptConfig,
    pub list_bin_paths: RtxPluginTomlScriptConfig,
}

impl RtxPluginToml {
    pub fn from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Default::default());
        }
        trace!("parsing: {}", path.display());
        let mut rf = Self::init();
        let body = fs::read_to_string(path).suggestion("ensure file exists and can be read")?;
        rf.parse(&body)?;
        Ok(rf)
    }

    fn init() -> Self {
        Self {
            ..Default::default()
        }
    }

    fn parse(&mut self, s: &str) -> Result<()> {
        let doc: Document = s.parse().suggestion("ensure file is valid TOML")?;
        for (k, v) in doc.iter() {
            match k {
                "exec-env" => self.exec_env = self.parse_script_config(k, v)?,
                "list-bin-paths" => self.list_bin_paths = self.parse_script_config(k, v)?,
                _ => Err(eyre!("unknown key: {}", k))?,
            }
        }
        Ok(())
    }

    fn parse_script_config(&mut self, key: &str, v: &Item) -> Result<RtxPluginTomlScriptConfig> {
        match v.as_table_like() {
            Some(table) => {
                let mut config = RtxPluginTomlScriptConfig::default();
                for (k, v) in table.iter() {
                    let key = format!("{}.{}", key, k);
                    match k {
                        "cache-key" => config.cache_key = Some(self.parse_string_array(k, v)?),
                        _ => parse_error!(key, v, "one of: cache-key")?,
                    }
                }
                Ok(config)
            }
            _ => parse_error!(key, v, "table")?,
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
            _ => parse_error!(k, v, "array")?,
        }
    }

    fn parse_string(&mut self, k: &str, v: &Value) -> Result<String> {
        match v.as_str() {
            Some(v) => Ok(v.to_string()),
            _ => parse_error!(k, v, "string")?,
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
        let cf = RtxPluginToml::from_file(&dirs::HOME.join("fixtures/rtx.plugin.toml")).unwrap();

        assert_debug_snapshot!(cf.exec_env);
    }

    #[test]
    fn test_exec_env() {
        let cf = parse(&formatdoc! {r#"
        [exec-env]
        cache-key = ["foo", "bar"]
        [list-bin-paths]
        cache-key = ["foo"]
        "#});

        assert_debug_snapshot!(cf.exec_env, @r###"
        RtxPluginTomlScriptConfig {
            cache_key: Some(
                [
                    "foo",
                    "bar",
                ],
            ),
        }
        "###);
    }

    fn parse(s: &str) -> RtxPluginToml {
        let mut cf = RtxPluginToml::init();
        cf.parse(s).unwrap();
        cf
    }
}
