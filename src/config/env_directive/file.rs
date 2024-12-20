use crate::config::env_directive::EnvResults;
use crate::file::display_path;
use crate::{file, sops, Result};
use eyre::{bail, eyre, WrapErr};
use indexmap::IndexMap;
use rops::file::format::{JsonFileFormat, YamlFileFormat};
use std::path::{Path, PathBuf};

// use indexmap so source is after value for `mise env --json` output
type EnvMap = IndexMap<String, String>;

#[derive(serde::Serialize, serde::Deserialize)]
struct Env<V> {
    #[serde(default)]
    sops: IndexMap<String, V>,
    #[serde(flatten)]
    env: IndexMap<String, V>,
}

impl EnvResults {
    #[allow(clippy::too_many_arguments)]
    pub fn file(
        ctx: &mut tera::Context,
        tera: &mut tera::Tera,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        config_root: &Path,
        input: String,
    ) -> Result<IndexMap<PathBuf, EnvMap>> {
        let mut out = IndexMap::new();
        let s = r.parse_template(ctx, tera, source, &input)?;
        for p in xx::file::glob(normalize_path(config_root, s.into())).unwrap_or_default() {
            let env = out.entry(p.clone()).or_insert_with(IndexMap::new);
            let parse_template = |s: String| r.parse_template(ctx, tera, source, &s);
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            *env = match ext.as_str() {
                "json" => Self::json(&p, parse_template)?,
                "yaml" => Self::yaml(&p, parse_template)?,
                "toml" => unimplemented!("toml"),
                _ => Self::dotenv(&p)?,
            };
        }
        Ok(out)
    }

    fn json<PT>(p: &Path, parse_template: PT) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse json file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_json::Value> = serde_json::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let raw = sops::decrypt::<_, JsonFileFormat>(&raw, parse_template, "json")?;
                f = serde_json::from_str(&raw).wrap_err_with(errfn)?;
            }
            f.env
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k,
                        match v {
                            serde_json::Value::String(s) => s,
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::Bool(b) => b.to_string(),
                            _ => bail!("unsupported json value: {v:?}"),
                        },
                    ))
                })
                .collect()
        } else {
            Ok(EnvMap::new())
        }
    }

    fn yaml<PT>(p: &Path, parse_template: PT) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse yaml file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_yaml::Value> = serde_yaml::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let raw = sops::decrypt::<_, YamlFileFormat>(&raw, parse_template, "yaml")?;
                f = serde_yaml::from_str(&raw).wrap_err_with(errfn)?;
            }
            f.env
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k,
                        match v {
                            serde_yaml::Value::String(s) => s,
                            serde_yaml::Value::Number(n) => n.to_string(),
                            serde_yaml::Value::Bool(b) => b.to_string(),
                            _ => bail!("unsupported yaml value: {v:?}"),
                        },
                    ))
                })
                .collect()
        } else {
            Ok(EnvMap::new())
        }
    }

    fn dotenv(p: &Path) -> Result<EnvMap> {
        let errfn = || eyre!("failed to parse dotenv file: {}", display_path(p));
        let mut env = EnvMap::new();
        if let Ok(dotenv) = dotenvy::from_path_iter(p) {
            for item in dotenv {
                let (k, v) = item.wrap_err_with(errfn)?;
                env.insert(k, v);
            }
        }
        Ok(env)
    }
}
